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
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ash_core::completions::{Completion, CompletionKind};

use auto_ai_client::{AiClient, CompletionRequest};

/// Time budget for one AI completion request. Past this, the background
/// thread gives up and writes nothing (completion degrades to static).
/// 500 ms matches the design target: high-frequency interaction can't wait.
const AI_TIMEOUT: Duration = Duration::from_millis(500);

/// Which completion position an AI result belongs to.
///
/// The two AI sources fire at *different* cursor positions (subcommand vs
/// command name) and their results must never cross — a subcommand candidate
/// like `checkout` is nonsense at a parameter position, and vice versa. We tag
/// every cached result with its slot and only merge it at the matching
/// position, so a stale result can't leak across positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiSlot {
    /// Subcommand/flag candidates, fired when the static spec is thin at a
    /// subcommand position (cursor past the command word).
    Subcommand,
    /// Natural-language → pipeline translation, fired at the command-name
    /// position when the first token matches no known command/alias.
    NaturalLanguage,
}

/// A cached AI completion result plus the exact line it was requested for.
struct AiEntry {
    /// The full `line` passed to `complete()` at trigger time. We match
    /// against the *current* full line with strict equality (not a prefix
    /// test) so a result requested for `"git c"` is NOT served while the user
    /// is editing `"git checkout main"` — that would inject stale subcommand
    /// candidates into the parameter position.
    key: String,
    completions: Vec<Completion>,
}

/// Global cache of the most recent AI completion result, **per slot**.
///
/// Subcommand and natural-language results live in independent slots so they
/// can't overwrite each other (they fire at different positions and serve
/// different completions). An entry is only ever returned for a request whose
/// full line exactly matches its `key`.
static AI_PENDING: Mutex<[Option<AiEntry>; 2]> = Mutex::new([None, None]);

/// In-flight request tracking: prevents the thread-storm where every keystroke
/// at a thin-spec position spawns a new background thread for the *same*
/// (slot, key). A key enters the set on trigger and leaves when the thread
/// finishes (success or failure), so at most one outstanding request per
/// (slot, key) exists at a time.
///
/// Wrapped in `OnceLock` because `HashSet::new()` is not a const fn, so it
/// can't initialize a `static Mutex` directly.
static IN_FLIGHT: OnceLock<Mutex<HashSet<(usize, String)>>> = OnceLock::new();

/// Borrow the in-flight set, initializing it once on first use.
fn in_flight() -> &'static Mutex<HashSet<(usize, String)>> {
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Process-global test serialization lock. `AI_PENDING` and `IN_FLIGHT` are
/// process-global statics shared by every test that touches the AI cache
/// (both in this module and in `completions_reedline`'s integration tests).
/// Cargo runs tests multi-threaded by default, so any two such tests running
/// concurrently would clobber each other's state. Tests take this lock first
/// to force serial execution. `pub(crate)` so cross-module integration tests
/// share the same lock. Only built under `cfg(test)`.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Borrow the test lock (mirrors the `in_flight()` access pattern).
#[cfg(test)]
pub(crate) fn test_lock() -> &'static Mutex<()> {
    &TEST_LOCK
}

/// Map a slot to its index in `AI_PENDING` / key in `IN_FLIGHT`.
fn slot_index(slot: AiSlot) -> usize {
    match slot {
        AiSlot::Subcommand => 0,
        AiSlot::NaturalLanguage => 1,
    }
}

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
/// `Subcommand` cache slot and is picked up by the next `complete()` call at
/// the same subcommand position via [`take_ai_pending`].
///
/// Only called when the static/dynamic resolver returned fewer than 3
/// candidates at a subcommand position (see the completer integration) —
/// saving local compute when the spec is already rich enough. Skips the spawn
/// entirely if an identical (cmd, prefix) request is already in flight.
pub fn trigger_ai_subcommand(cmd: &str, prefix: &str, ctx: CtxSnapshot) {
    let cmd = cmd.to_string();
    let prefix = prefix.to_string();
    // The key is the exact line this request serves; it must match the full
    // `line` later passed to take_ai_pending. We reconstruct the line the
    // completer had: `<cmd> <prefix>` (see complete()'s subcommand branch).
    let key = format!("{} {}", cmd, prefix);
    if !begin_in_flight(AiSlot::Subcommand, &key) {
        return; // an identical request is already running — don't storm.
    }
    std::thread::spawn(move || {
        let result = fetch_subcommands(&cmd, &prefix, &ctx);
        match result {
            Ok(completions) if !completions.is_empty() => {
                store(AiSlot::Subcommand, key.clone(), completions);
            }
            _ => {} // error/timeout/empty → degrade to static (by design).
        }
        end_in_flight(AiSlot::Subcommand, &key);
    });
}

/// Fire a background LLM request to translate a natural-language phrase into
/// an ash command/pipeline. Returns immediately; result lands in the
/// `NaturalLanguage` cache slot.
///
/// Called when the first token matches no known command/alias/path (see the
/// completer integration). Reuses the same system-prompt shape as the
/// standalone `ask_ai` (repl.rs) so NL behavior is consistent across the two
/// surfaces. Skips the spawn if an identical phrase request is in flight.
pub fn trigger_nl_to_pipeline(input: &str, ctx: CtxSnapshot) {
    let input = input.to_string();
    let key = input.clone();
    if !begin_in_flight(AiSlot::NaturalLanguage, &key) {
        return;
    }
    std::thread::spawn(move || {
        let result = fetch_nl_pipeline(&input, &ctx);
        if let Ok(completion) = result {
            store(AiSlot::NaturalLanguage, key.clone(), vec![completion]);
        }
        end_in_flight(AiSlot::NaturalLanguage, &key);
    });
}

/// Drain the pending AI candidates for `slot` iff they were produced for the
/// *exact* `line` (full-line equality — never a prefix test).
///
/// Returns `None` when there's nothing pending for this slot, the result
/// errored, or the cached key doesn't equal the current line. **Only a match
/// clears the slot** — a mismatch leaves the entry in place so a result that
/// arrived slightly early isn't destroyed before the line catches up (the user
/// may be mid-edit toward the matching line).
pub fn take_ai_pending(slot: AiSlot, line: &str) -> Option<Vec<Completion>> {
    let idx = slot_index(slot);
    let mut guard = AI_PENDING.lock().ok()?;
    let Some(entry) = guard[idx].as_ref() else {
        return None;
    };
    if entry.key != line {
        // Non-matching key: LEAVE the entry so it isn't destroyed by an
        // unrelated keystroke. It will be overwritten by a newer result for
        // this slot, or matched on a later keystroke.
        return None;
    }
    // Exact match: take it (so it's shown exactly once).
    guard[idx].take().map(|e| e.completions)
}

/// Re-export the slot type for the completer to name the position it's
/// merging at.
pub use AiSlot as Slot;

/// Mark `(slot, key)` as in flight; returns false if already in flight (caller
/// should skip the spawn). Guards the thread-storm: at most one outstanding
/// request per (slot, key) at a time.
fn begin_in_flight(slot: AiSlot, key: &str) -> bool {
    in_flight()
        .lock()
        .ok()
        .map(|mut s| s.insert((slot_index(slot), key.to_string())))
        .unwrap_or(true) // on lock poison, proceed (best-effort, not a correctness gate)
}

/// Clear the in-flight marker when a request finishes.
fn end_in_flight(slot: AiSlot, key: &str) {
    if let Ok(mut s) = in_flight().lock() {
        s.remove(&(slot_index(slot), key.to_string()));
    }
}

/// Store a result in `slot`, keyed by the trigger-time full line.
///
/// `pub(crate)` so the completer's integration tests can inject a finished
/// result (simulating the background thread having completed) without needing
/// a live daemon — this is the seam that makes `complete()` → `Suggestion`
/// AI-merge behavior unit-testable.
pub(crate) fn store(slot: AiSlot, key: String, completions: Vec<Completion>) {
    let idx = slot_index(slot);
    if let Ok(mut g) = AI_PENDING.lock() {
        g[idx] = Some(AiEntry { key, completions });
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

    fn ai(label: &str) -> Completion {
        Completion::with_kind(label, label, CompletionKind::AiSuggested)
    }

    /// Force this test to run serially against other tests that touch the
    /// process-global AI cache/in-flight state (see `test_lock`). Under cargo's
    /// default multi-threaded runner, concurrent tests would clobber each
    /// other's global state.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        test_lock().lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clear both slots so tests start from a known-empty state.
    fn clear_cache() {
        if let Ok(mut g) = AI_PENDING.lock() {
            *g = [None, None];
        }
        if let Ok(mut s) = in_flight().lock() {
            s.clear();
        }
    }

    #[test]
    fn take_pending_is_none_when_empty() {
        let _g = serial();
        clear_cache();
        assert!(take_ai_pending(AiSlot::Subcommand, "anything").is_none());
        assert!(take_ai_pending(AiSlot::NaturalLanguage, "anything").is_none());
    }

    #[test]
    fn store_then_take_returns_for_exact_matching_key() {
        let _g = serial();
        clear_cache();
        store(
            AiSlot::Subcommand,
            "git c".to_string(),
            vec![ai("checkout"), ai("commit")],
        );
        // Exact key match → returned.
        let got = take_ai_pending(AiSlot::Subcommand, "git c").unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].replacement, "checkout");
        assert_eq!(got[0].kind, CompletionKind::AiSuggested);
    }

    #[test]
    fn take_drains_on_match_so_not_returned_twice() {
        let _g = serial();
        clear_cache();
        store(AiSlot::Subcommand, "foo".to_string(), vec![ai("a")]);
        assert!(take_ai_pending(AiSlot::Subcommand, "foo").is_some());
        assert!(
            take_ai_pending(AiSlot::Subcommand, "foo").is_none(),
            "second take after a match must be None"
        );
    }

    // ── Regression: bug #2 (prefix-match injection) ──────────────────────
    // The OLD code used `line.starts_with(cached) || cached.starts_with(line)`,
    // so a result keyed on "git c" would surface for "git checkout main" and
    // inject stale subcommand candidates into the parameter position. The fix
    // is strict full-line equality: only an exact key match returns results.

    #[test]
    fn prefix_overlap_does_not_match() {
        let _g = serial();
        clear_cache();
        store(AiSlot::Subcommand, "git c".to_string(), vec![ai("checkout")]);
        // "git c" is a prefix of "git checkout main", but they're NOT equal →
        // no match. This is the headline regression test for bug #2.
        assert!(
            take_ai_pending(AiSlot::Subcommand, "git checkout main").is_none(),
            "prefix-overlapping key must NOT match (strict equality)"
        );
    }

    #[test]
    fn stale_result_for_different_line_is_not_returned() {
        let _g = serial();
        clear_cache();
        store(AiSlot::Subcommand, "git push".to_string(), vec![ai("a")]);
        assert!(
            take_ai_pending(AiSlot::Subcommand, "docker run").is_none(),
            "unrelated line must not get a stale result"
        );
    }

    // ── Regression: bug #3 (non-destructive drain) ───────────────────────
    // The OLD take cleared the slot unconditionally, so an unrelated keystroke
    // could destroy a result that hadn't been served yet. The fix: a
    // non-matching take leaves the entry in place.

    #[test]
    fn non_matching_take_leaves_entry_for_later_match() {
        let _g = serial();
        clear_cache();
        store(AiSlot::Subcommand, "git c".to_string(), vec![ai("checkout")]);
        // A take with a different line must NOT clear the entry.
        assert!(take_ai_pending(AiSlot::Subcommand, "other").is_none());
        // The entry survives and is served when the right line arrives.
        let got = take_ai_pending(AiSlot::Subcommand, "git c");
        assert!(got.is_some(), "entry must survive a non-matching take");
        assert_eq!(got.unwrap()[0].replacement, "checkout");
    }

    // ── Regression: bug #1/#3 (slot isolation) ───────────────────────────
    // Subcommand and natural-language results live in independent slots, so
    // they can't overwrite each other even though both fire from complete().

    #[test]
    fn slots_are_independent() {
        let _g = serial();
        clear_cache();
        store(
            AiSlot::Subcommand,
            "git c".to_string(),
            vec![ai("checkout")],
        );
        store(
            AiSlot::NaturalLanguage,
            "列出文件".to_string(),
            vec![ai("ls")],
        );
        // Both coexist; draining one doesn't touch the other.
        let sub = take_ai_pending(AiSlot::Subcommand, "git c").unwrap();
        assert_eq!(sub[0].replacement, "checkout");
        let nl = take_ai_pending(AiSlot::NaturalLanguage, "列出文件").unwrap();
        assert_eq!(nl[0].replacement, "ls");
    }

    // ── Regression: bug #1 (in-flight dedup) ─────────────────────────────
    // begin_in_flight / end_in_flight gate spawning: a second begin for the
    // same (slot, key) while the first is in flight returns false.

    #[test]
    fn in_flight_dedup_prevents_double_begin() {
        let _g = serial();
        clear_cache();
        let key = "git c";
        assert!(
            begin_in_flight(AiSlot::Subcommand, key),
            "first begin for (slot,key) should succeed"
        );
        assert!(
            !begin_in_flight(AiSlot::Subcommand, key),
            "second begin while in flight must be rejected"
        );
        // A different key on the same slot is allowed.
        assert!(begin_in_flight(AiSlot::Subcommand, "git p"));
        end_in_flight(AiSlot::Subcommand, key);
        // After end, the slot is free again.
        assert!(
            begin_in_flight(AiSlot::Subcommand, key),
            "begin must succeed again after end"
        );
        end_in_flight(AiSlot::Subcommand, key);
        end_in_flight(AiSlot::Subcommand, "git p");
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
