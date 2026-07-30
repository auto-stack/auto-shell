//! AI completion layer (Plan 032 M2).
//!
//! Adds two LLM-backed completion sources on top of the static/dynamic engine
//! (Plan 021) and the context-aware ranker (M1.1):
//!
//! - **LLM subcommand completion**: when the static spec is thin (< 3
//!   candidates) at a subcommand position, ask the local model for likely
//!   subcommands/flags the static spec doesn't list.
//! - **Natural-language → pipeline**: when the first token isn't a known
//!   command/alias/path, ask the model to translate the typed phrase into an
//!   ash command/pipeline.
//!
//! ## Asynchronous strategy (reedline constraint)
//!
//! reedline's `Completer::complete` is synchronous with no async hook, so an
//! LLM round-trip can't block it. We mirror the proven pattern from
//! `frontend::suggest` (Plan 029 §7.3): the completer **fires a background
//! thread** that talks to the local model (Ollama via `aaid`), and writes the
//! result into a static cache. The **next** `complete()` call drains that
//! cache and merges the AI candidates in. Net effect: AI completion lags by
//! about one keystroke — an acceptable trade-off versus stalling input.
//!
//! ## Degradation
//!
//! Completion is high-frequency and must never lag the shell. If the daemon
//! is unavailable, the request times out (500 ms), or the model errors, the
//! background thread writes nothing — completion falls back to the pure
//! static + dynamic engine (Plan 021). AI is an enhancement, not a dependency.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use ash_core::completions::{Completion, CompletionKind};

use auto_ai_client::{AiClient, CompletionRequest};

/// Time budget for one AI completion request. Past this, the background
/// thread gives up and writes nothing (completion degrades to static).
/// 500 ms matches the design target: high-frequency interaction can't wait.
const AI_TIMEOUT: Duration = Duration::from_millis(500);

/// Global cache of the most recent AI completion result.
///
/// Keyed by the `line` snapshot at trigger time so a stale result from a
/// previous (different) line is never merged into the current one. `None`
/// means no result has landed (or it was already drained).
static AI_PENDING: Mutex<Option<(String, Vec<Completion>)>> = Mutex::new(None);

/// A `Send` snapshot of the completion context for the background thread.
///
/// The live [`ash_core::completions::CompletionContext`] borrows a `Box<Fn>`
/// and is constructed per-`complete()`; it (and the `Shell` it derives from)
/// is not `Send`. We copy the small, owned fields the AI prompt needs.
#[derive(Clone, Debug, Default)]
pub struct CtxSnapshot {
    pub current_dir: PathBuf,
    pub last_command: Option<String>,
    pub history: Vec<String>,
    pub aliases: HashMap<String, String>,
}

impl CtxSnapshot {
    /// Render the snapshot into the same compact context block the AI features
    /// inject into their system prompt (mirrors `ai_context::build_context_block`,
    /// but from an owned snapshot instead of a live `&Shell`).
    fn context_block(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("操作系统: {}", std::env::consts::OS));
        lines.push(format!("当前目录: {}", self.current_dir.display()));
        if let Some(last) = &self.last_command {
            lines.push(format!("上一条命令: {}", last));
        }
        if !self.aliases.is_empty() {
            let preview = self
                .aliases
                .iter()
                .take(5)
                .map(|(k, v)| format!("{}='{}'", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("用户别名({} 个): {}", self.aliases.len(), preview));
        }
        lines.join("\n")
    }
}

// ── Public trigger / drain API ──────────────────────────────────────────

/// Fire a background LLM request for subcommand/flag candidates the static
/// spec doesn't cover. Returns immediately; the result (if any) lands in the
/// cache and is picked up by the next `complete()` call via [`take_ai_pending`].
///
/// Only called when the static/dynamic resolver returned fewer than 3
/// candidates at a subcommand position (see the completer integration) —
/// saving local compute when the spec is already rich enough.
pub fn trigger_ai_subcommand(cmd: &str, prefix: &str, ctx: CtxSnapshot) {
    let cmd = cmd.to_string();
    let prefix = prefix.to_string();
    let snapshot_line = format!("{} {}", cmd, prefix);
    std::thread::spawn(move || {
        let result = fetch_subcommands(&cmd, &prefix, &ctx);
        if let Ok(completions) = result {
            if !completions.is_empty() {
                store(snapshot_line, completions);
            }
        }
        // On error/timeout: write nothing → degrade to static (by design).
    });
}

/// Fire a background LLM request to translate a natural-language phrase into
/// an ash command/pipeline. Returns immediately; result lands in the cache.
///
/// Called when the first token matches no known command/alias/path (see the
/// completer integration). Reuses the same system-prompt shape as the
/// standalone `ask_ai` (repl.rs) so NL behavior is consistent across the two
/// surfaces.
pub fn trigger_nl_to_pipeline(input: &str, ctx: CtxSnapshot) {
    let input = input.to_string();
    let snapshot_line = input.clone();
    std::thread::spawn(move || {
        let result = fetch_nl_pipeline(&input, &ctx);
        if let Ok(completion) = result {
            store(snapshot_line, vec![completion]);
        }
    });
}

/// Drain the pending AI candidates iff they were produced for `line`.
///
/// Returns `None` when there's nothing pending, the result errored, or the
/// cached line snapshot no longer matches what the user is typing (so a stale
/// result from a deleted prefix never leaks into the current completions).
/// Always clears the cache (take semantics).
pub fn take_ai_pending(line: &str) -> Option<Vec<Completion>> {
    let mut guard = AI_PENDING.lock().ok()?;
    let (cached_line, completions) = guard.take()?;
    // Only return the result if it was for the current line; otherwise drop it.
    // We compare against the snapshot key the trigger stored (which encodes
    // cmd+prefix for subcommands, or the raw input for NL).
    if matches_line(&cached_line, line) {
        Some(completions)
    } else {
        None
    }
}

/// Compare the cached snapshot key against the current line. The snapshot key
/// is a conservative prefix of the line (cmd + space + prefix, or the NL
/// phrase), so we accept the result when the line starts with the key.
fn matches_line(cached: &str, line: &str) -> bool {
    if cached.is_empty() {
        return false;
    }
    line.starts_with(cached) || cached.starts_with(line)
}

/// Store a result keyed by the trigger-time line snapshot.
fn store(line: String, completions: Vec<Completion>) {
    if let Ok(mut g) = AI_PENDING.lock() {
        *g = Some((line, completions));
    }
}

// ── Background-thread fetchers (each builds its own runtime) ────────────

/// Ask the local model for subcommand/flag candidates. Synchronous (runs on
/// the background thread). Filters the response to the typed prefix.
fn fetch_subcommands(
    cmd: &str,
    prefix: &str,
    ctx: &CtxSnapshot,
) -> Result<Vec<Completion>, String> {
    let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;
    let system = format!(
        "You complete subcommands/flags for the shell command `{cmd}` in Ash.\n\
         {context}\n\
         List up to 5 likely subcommands or flags for the prefix `{prefix}`.\n\
         Reply with one candidate per line, the bare name only (e.g. `checkout`),\n\
         no explanation. If you have nothing useful, reply with a single NONE.",
        context = ctx.context_block(),
    );
    let req = CompletionRequest::single("tier:min", &format!("{cmd} {prefix}"))
        .with_system(&system)
        .with_max_tokens(128)
        .with_temperature(0.2);

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {}", e))?;
    let resp = rt
        .block_on(async {
            tokio::time::timeout(AI_TIMEOUT, client.complete(&req)).await
        })
        .map_err(|_| "AI completion timed out".to_string())?
        .map_err(|e| format!("{}", e))?;
    if !resp.is_ok() {
        return Err(format!("AI error: {:?}", resp.error));
    }

    // Parse lines, keep only those matching the typed prefix, dedupe.
    let mut seen = std::collections::HashSet::new();
    let completions = resp
        .content
        .lines()
        .map(|l| l.trim().trim_start_matches('-').trim().to_string())
        .filter(|l| {
            !l.is_empty()
                && l.to_uppercase() != "NONE"
                && (prefix.is_empty() || l.starts_with(prefix))
        })
        .filter(|l| seen.insert(l.clone()))
        .take(5)
        .map(|label| Completion {
            display: label.clone(),
            replacement: label,
            description: Some("(AI 建议)".into()),
            kind: CompletionKind::AiSuggested,
            is_prefix_match: true,
        })
        .collect();
    Ok(completions)
}

/// Ask the local model to translate a natural-language phrase into an ash
/// command/pipeline. Synchronous (background thread).
fn fetch_nl_pipeline(input: &str, ctx: &CtxSnapshot) -> Result<Completion, String> {
    let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;
    let system = format!(
        "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
         {context}\n\
         The user will describe what they want to do in natural language.\n\
         Translate it into a SINGLE ash shell command (or pipeline).\n\
         Rules:\n\
         - Respond with ONLY the command, no explanation, no markdown.\n\
         - Use standard Unix commands (ls, grep, find, etc.).\n\
         - For Ash-specific features, use: ls | .size > 10.mb | sort .name\n\
         - If multiple steps are needed, use && to chain them.\n\
         - If you're unsure, give your best single-command guess.",
        context = ctx.context_block(),
    );
    let req = CompletionRequest::single("tier:min", input)
        .with_system(&system)
        .with_max_tokens(256)
        .with_temperature(0.3);

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {}", e))?;
    let resp = rt
        .block_on(async {
            tokio::time::timeout(AI_TIMEOUT, client.complete(&req)).await
        })
        .map_err(|_| "AI completion timed out".to_string())?
        .map_err(|e| format!("{}", e))?;
    if !resp.is_ok() {
        return Err(format!("AI error: {:?}", resp.error));
    }

    // Strip markdown code fences if the model wraps the answer.
    let cmd = resp
        .content
        .trim()
        .trim_start_matches("```bash\n")
        .trim_start_matches("```sh\n")
        .trim_start_matches("```\n")
        .trim_end_matches("\n```")
        .trim()
        .to_string();
    if cmd.is_empty() {
        return Err("AI returned empty translation".into());
    }
    Ok(Completion {
        display: cmd.clone(),
        replacement: cmd,
        description: Some("(自然语言翻译)".into()),
        kind: CompletionKind::AiSuggested,
        is_prefix_match: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> CtxSnapshot {
        CtxSnapshot {
            current_dir: PathBuf::from("/tmp"),
            last_command: Some("ls".into()),
            history: vec!["ls".into()],
            aliases: HashMap::new(),
        }
    }

    #[test]
    fn take_pending_is_none_when_empty() {
        let _ = take_ai_pending("anything");
        // Drain any leftover, then assert empty.
        let _ = take_ai_pending("anything");
        assert!(take_ai_pending("anything").is_none());
    }

    #[test]
    fn store_then_take_returns_for_matching_line() {
        // Manually store a result and drain it for a matching line.
        store(
            "git ch".to_string(),
            vec![Completion::with_kind(
                "checkout",
                "checkout",
                CompletionKind::AiSuggested,
            )],
        );
        let got = take_ai_pending("git checkout");
        assert!(got.is_some(), "should return for a line that matches the key");
        let comps = got.unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].replacement, "checkout");
        assert_eq!(comps[0].kind, CompletionKind::AiSuggested);
    }

    #[test]
    fn take_drains_so_not_returned_twice() {
        store("foo".to_string(), vec![Completion::new("a", "a")]);
        assert!(take_ai_pending("foo").is_some());
        assert!(take_ai_pending("foo").is_none(), "second take must be None");
    }

    #[test]
    fn stale_result_for_different_line_is_dropped() {
        // A result keyed on one line must NOT surface for an unrelated line.
        store("git push".to_string(), vec![Completion::new("a", "a")]);
        assert!(
            take_ai_pending("docker run").is_none(),
            "stale result should be dropped, not returned"
        );
    }

    #[test]
    fn matches_line_accepts_prefix_overlap() {
        assert!(matches_line("git ch", "git checkout"));
        assert!(matches_line("列出", "列出最大文件"));
        // Completely disjoint → no match.
        assert!(!matches_line("git", "docker"));
    }

    #[test]
    fn empty_cached_key_never_matches() {
        assert!(!matches_line("", "anything"));
    }

    #[test]
    fn context_block_includes_cwd_and_last_command() {
        let s = snap();
        let block = s.context_block();
        assert!(block.contains("当前目录: /tmp"));
        assert!(block.contains("上一条命令: ls"));
    }

    #[test]
    fn context_block_includes_aliases_preview() {
        let mut s = snap();
        s.aliases.insert("g".to_string(), "git".to_string());
        let block = s.context_block();
        assert!(block.contains("用户别名(1 个)"));
        assert!(block.contains("g='git'"));
    }
}
