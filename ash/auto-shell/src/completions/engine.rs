//! Plan 041 M7: the frontend-agnostic completion *engine*.
//!
//! This module sinks the completion orchestration logic that previously lived
//! inside `ash_tui::ShellCompleter::complete()` (bound to reedline's
//! `Vec<Suggestion>`). Now any frontend (TUI via reedline, GUI via Tauri) can
//! call [`complete`] and get back a `Vec<Completion>` — the core, dep-free type
//! — and do its own rendering.
//!
//! The engine layers three sources, in order:
//! 1. **External command spec** (`CompletionProvider::resolve`) — for commands
//!    with a loaded `--help`-derived spec (git/cargo/…). Includes AI subcommand
//!    candidates from the background layer.
//! 2. **Static completion** (`get_completions_with_context`) — registry
//!    signatures + file/path/variable completion. Includes AI NL→pipeline
//!    candidates.
//! 3. **Context-aware ranking** — reorders command-name completions by history
//!    frequency / repo context (only at the command-name position).
//!
//! `ShellCompleter` becomes a thin reedline adapter over this; the GUI adds a
//! `complete(line, cursor)` Tauri command that calls this directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ash_core::completions::{context_rank, help_parser, CompletionContext, CompletionProvider};

use super::ai_layer::{self, CtxSnapshot};
use super::{Completion, CompletionKind, CompletionSignature};

/// Owned context snapshot the engine needs to produce context-aware completions.
///
/// Mirrors `ash_tui::CompletionState` but lives in the core crate so the GUI
/// worker can build one without depending on ash-tui. Fields are `Clone`-cheap
/// snapshots taken under a lock by the caller.
#[derive(Clone, Debug, Default)]
pub struct CompletionCtx {
    /// Current working directory (for file/path completion + `--help` probes).
    pub current_dir: PathBuf,
    /// The last executed command line (for ranking coherence). `None` pre-first-run.
    pub last_command: Option<String>,
    /// Exit code of the last command (for ranking).
    pub last_exit_code: Option<i32>,
    /// A bounded window of recent history entries (for ranking + ghost text).
    pub history: Vec<String>,
    /// User aliases (so alias names complete at the command-name position).
    pub aliases: HashMap<String, String>,
}

impl CompletionCtx {
    /// Build a `Send` snapshot for the AI completion layer. The live context
    /// borrows a closure inside `CompletionContext` (not `Send`); the background
    /// thread needs an owned, copyable view.
    pub fn ai_snapshot(&self) -> CtxSnapshot {
        CtxSnapshot {
            current_dir: self.current_dir.clone(),
            last_command: self.last_command.clone(),
            history: self.history.clone(),
            aliases: self.aliases.clone(),
        }
    }
}

/// Produce completions for `line` at `cursor`, using the given registry
/// `signatures` and the live `provider` (external command specs). `ctx` carries
/// cwd / last command / history / aliases for context-aware ranking and AI.
///
/// Returns `Vec<Completion>` — the core, terminal-independent type. Each
/// frontend converts to its own render form (reedline `Suggestion` / JSON for
/// the GUI). The `span` (text range to replace) is the caller's responsibility
/// since it depends on the frontend's cursor model.
///
/// This is the sink of `ShellCompleter::complete()` (Plan 041 M7). `provider`
/// is `&mut` because `ensure_spec` may register a probed spec as a side effect.
pub fn complete(
    line: &str,
    cursor: usize,
    signatures: &[CompletionSignature],
    provider: &mut CompletionProvider,
    ctx: &CompletionCtx,
) -> Vec<Completion> {
    let pos = cursor.min(line.len());
    let before_cursor = &line[..pos];
    let trimmed = before_cursor.trim_end();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    // If we have a first word and it's an external command with a spec,
    // route to the CompletionProvider.
    if let Some(&cmd) = parts.first() {
        ensure_spec(cmd, provider, signatures, ctx);
        if provider.has_spec(cmd) {
            // Determine cursor part and prefix.
            let ends_with_space = before_cursor.ends_with(|c: char| c.is_whitespace());
            let (cursor_part, prefix) = if ends_with_space {
                (parts.len(), "")
            } else {
                let idx = parts.len().saturating_sub(1);
                (idx, parts.last().copied().unwrap_or(""))
            };

            let resolve_parts: Vec<&str> = if ends_with_space {
                let mut p = parts.clone();
                p.push("");
                p
            } else {
                parts.clone()
            };

            let comp_ctx = CompletionContext {
                current_dir: ctx.current_dir.clone(),
                command_executor: Box::new(execute_command),
                last_command: ctx.last_command.clone(),
                last_exit_code: ctx.last_exit_code,
                history: ctx.history.clone(),
                aliases: ctx.aliases.clone(),
            };

            let mut completions = provider.resolve(&resolve_parts, cursor_part, prefix, &comp_ctx);

            // Merge any AI subcommand candidates the previous keystroke's
            // background fetch produced for THIS exact line. Full-line equality
            // in the cache layer prevents stale results leaking in.
            let subcmd_key = format!("{cmd} {prefix}");
            merge_ai_pending(ai_layer::Slot::Subcommand, &subcmd_key, &mut completions);

            // When the static spec is thin at a subcommand position, ask the
            // local model for more candidates (fire-and-forget; never blocks).
            if ai_completion_enabled()
                && completions.len() < 3
                && !cmd.starts_with('-')
                && cursor_part >= 1
            {
                ai_layer::trigger_ai_subcommand(cmd, prefix, ctx.ai_snapshot());
            }

            if !completions.is_empty() {
                return completions;
            }
            // Provider found the spec but returned nothing — fall through.
        }
    }

    // Default: static completion (registry signatures + file/path completion).
    let mut completions =
        super::get_completions_with_context(before_cursor, signatures);

    // Merge any pending NL→pipeline translation fired on a previous keystroke.
    let phrase_key = before_cursor.trim();
    merge_ai_pending(ai_layer::Slot::NaturalLanguage, phrase_key, &mut completions);

    // At the command-name position with no known command/alias match, the user
    // may be typing a natural-language phrase — ask the model to translate it.
    if ai_completion_enabled()
        && is_command_name_position(before_cursor)
        && first_token_is_unknown(before_cursor, signatures, &ctx.aliases)
        && completions.iter().all(|c| {
            c.kind != CompletionKind::Command && c.kind != CompletionKind::External
        })
    {
        if !phrase_key.is_empty() {
            ai_layer::trigger_nl_to_pipeline(phrase_key, ctx.ai_snapshot());
        }
    }

    // Context-aware ranking — only for command-name completions (reordering
    // subcommand/flag/file lists would break their intentional order).
    if is_command_name_position(before_cursor) && !completions.is_empty() {
        let ranking_ctx = CompletionContext {
            current_dir: ctx.current_dir.clone(),
            command_executor: Box::new(|_, _| Ok(String::new())),
            last_command: ctx.last_command.clone(),
            last_exit_code: ctx.last_exit_code,
            history: ctx.history.clone(),
            aliases: ctx.aliases.clone(),
        };
        context_rank::rank(&mut completions, &ranking_ctx);
    }

    completions
}

// ── Helpers (moved verbatim from ShellCompleter; no reedline dependency) ─────

/// True when the cursor is completing the first token (a command name): there
/// is no interior whitespace before the cursor. Gates context-aware ranking.
fn is_command_name_position(before_cursor: &str) -> bool {
    !before_cursor.trim().contains(char::is_whitespace)
}

/// True when the first token of `before_cursor` is neither a known command
/// (registry signature) nor an alias — i.e. the user may be typing NL.
fn first_token_is_unknown(
    before_cursor: &str,
    signatures: &[CompletionSignature],
    aliases: &HashMap<String, String>,
) -> bool {
    let first = before_cursor.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return true;
    }
    !signatures.iter().any(|s| s.name == first) && !aliases.contains_key(first)
}

/// Plan 032 M2: whether AI completion is enabled (default true; disable via
/// `ai.completion: false` in config). Degrades cleanly without a daemon.
fn ai_completion_enabled() -> bool {
    let cfg = crate::auto_config::load();
    crate::auto_config::get_bool(&cfg, "ai", "completion").unwrap_or(true)
}

/// Plan 032 M2: drain any AI candidates the background thread produced for
/// `slot` at the exact `key` (full-line equality) and append them.
fn merge_ai_pending(slot: ai_layer::Slot, key: &str, completions: &mut Vec<Completion>) {
    if let Some(ai) = ai_layer::take_ai_pending(slot, key) {
        completions.extend(ai);
    }
}

/// Ensure a spec exists for `cmd`: cache hit → register; else probe
/// `cmd --help` → parse → write cache → register. Skips builtins/registered
/// commands. Best-effort. (Moved from `ShellCompleter::ensure_spec`.)
fn ensure_spec(
    cmd: &str,
    provider: &mut CompletionProvider,
    signatures: &[CompletionSignature],
    ctx: &CompletionCtx,
) {
    if provider.has_spec(cmd) {
        return;
    }
    // Don't probe shell builtins / registered commands.
    if crate::cmd::builtin::is_legacy_builtin(cmd)
        || is_shell_hardcoded_builtin(cmd)
        || signatures.iter().any(|s| s.name == cmd)
    {
        return;
    }
    // Don't probe script file paths (would trigger "Open with" on Windows).
    if is_likely_script_path(cmd) {
        return;
    }
    // 1. Cache tier.
    if let Some(spec) = super::spec_tiers::load_cache(cmd) {
        provider.register(spec);
        return;
    }
    // 2. Probe: run `cmd --help`, capture stdout regardless of exit code.
    let cwd = &ctx.current_dir;
    let help = capture_help(&format!("{cmd} --help"), cwd);
    if !help.trim().is_empty() {
        let spec = help_parser::parse_help(cmd, &help);
        let _ = super::spec_tiers::write_cache(cmd, &spec);
        provider.register(spec);
    }
}

/// The hardcoded builtins handled inside `Shell::execute_inner` (b/up/alias/
/// pushd/…) — never probe these for `--help`. Mirrors the list in
/// `ash-gui-vue/src-tauri/src/shell_worker.rs::is_shell_builtin` and the
/// dispatch in `shell.rs:execute_inner`.
fn is_shell_hardcoded_builtin(name: &str) -> bool {
    matches!(
        name,
        "cd" | "alias" | "unalias" | "source" | "." | "pushd" | "popd" | "dirs"
            | "jobs" | "fg" | "bg" | "suspend" | "def" | "hook" | "abbr" | "config"
            | "bind" | "up" | "u" | "b" | "set" | "export" | "unset" | "env"
            | "env.path" | "path" | "completions" | "use" | "exit" | "quit" | "q"
    )
}

/// True if `cmd` looks like a script path (contains / or \, or has a known
/// script extension) — don't probe it for `--help` (Plan 036).
fn is_likely_script_path(cmd: &str) -> bool {
    if cmd.contains('/') || cmd.contains('\\') {
        return true;
    }
    let lower = cmd.to_ascii_lowercase();
    [".ash", ".at", ".as", ".au", ".sh", ".bash"].iter().any(|ext| lower.ends_with(ext))
}

/// Run `cmd` via the platform shell, returning stdout regardless of exit code
/// (for probing `--help`, which often prints usage then exits non-zero).
fn capture_help(cmd: &str, cwd: &Path) -> String {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(cwd)
            .output()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(cwd)
            .output()
    };
    match result {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => String::new(),
    }
}

/// Execute an external command and capture its stdout (the `command_executor`
/// closure for `CompletionProvider`). Mirrors `ShellCompleter::execute_command`.
fn execute_command(cmd: &str, cwd: &Path) -> Result<String, String> {
    let output = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(cwd)
            .output()
            .map_err(|e| e.to_string())?
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(cwd)
            .output()
            .map_err(|e| e.to_string())?
    };
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
