//! Reedline Completer integration
//!
//! Provides integration between auto-shell's completion system and reedline's Tab completion.
//! The completer holds:
//! - A snapshot of CommandRegistry signatures for built-in commands
//! - A CompletionProvider for external command specs (git, cargo, etc.)
//! - Shared state (current_dir) updated by the REPL after each command

use crate::completions::{Completion, CompletionSignature};
use crate::completions::ai_layer::{self, CtxSnapshot};
use ash_core::completions::{
    context_rank, help_parser, CompletionContext, CompletionProvider,
};
use reedline::{Completer, Suggestion};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Shared completion state, updated by REPL after each command.
///
/// `ShellCompleter` reads a snapshot of this through its `Arc<Mutex<…>>`
/// handle on every `complete()` call. Plan 032 extends it beyond `current_dir`
/// to carry the AI-context plumbing (last command/exit code/history/aliases)
/// so the context-aware ranking and AI layers have what they need.
#[derive(Debug)]
pub struct CompletionState {
    pub current_dir: PathBuf,
    /// Plan 032 M0.3: the last executed command line (`None` before any run).
    pub last_command: Option<String>,
    /// Plan 032 M0.3: the exit code of the last command (`None` before any run).
    pub last_exit_code: Option<i32>,
    /// Plan 032 M0.3: a bounded snapshot of recent history entries.
    pub history: Vec<String>,
    /// Plan 032 M0.3: user aliases (snapshot of the shell's alias map).
    pub aliases: HashMap<String, String>,
}

impl CompletionState {
    pub fn new(current_dir: PathBuf) -> Self {
        Self {
            current_dir,
            last_command: None,
            last_exit_code: None,
            history: Vec::new(),
            aliases: HashMap::new(),
        }
    }
}

/// Owned snapshot of [`CompletionState`], taken under one lock so the
/// completer doesn't hold the mutex across `provider.resolve()` / AI work.
/// Plan 032 M0.3.
struct StateSnapshot {
    current_dir: PathBuf,
    last_command: Option<String>,
    last_exit_code: Option<i32>,
    history: Vec<String>,
    aliases: HashMap<String, String>,
}

impl StateSnapshot {
    fn from_state(s: &CompletionState) -> Self {
        // Default to "." so a missing cwd degrades to relative file completion
        // rather than panicking.
        let current_dir = if s.current_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            s.current_dir.clone()
        };
        Self {
            current_dir,
            last_command: s.last_command.clone(),
            last_exit_code: s.last_exit_code,
            history: s.history.clone(),
            aliases: s.aliases.clone(),
        }
    }

    /// Build a `Send` snapshot for the AI completion layer (Plan 032 M2). The
    /// live context borrows a closure and isn't `Send`; the background thread
    /// needs an owned, copyable view.
    fn ai_snapshot(&self) -> CtxSnapshot {
        CtxSnapshot {
            current_dir: self.current_dir.clone(),
            last_command: self.last_command.clone(),
            history: self.history.clone(),
            aliases: self.aliases.clone(),
        }
    }
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self {
            current_dir: PathBuf::from("."),
            last_command: None,
            last_exit_code: None,
            history: Vec::new(),
            aliases: HashMap::new(),
        }
    }
}

/// Reedline completer for auto-shell
pub struct ShellCompleter {
    signatures: Vec<CompletionSignature>,
    provider: CompletionProvider,
    state: Arc<Mutex<CompletionState>>,
}

impl ShellCompleter {
    pub fn new(
        signatures: Vec<CompletionSignature>,
        mut provider: CompletionProvider,
        state: Arc<Mutex<CompletionState>>,
    ) -> Self {
        // Plan 315: overlay generated → user tier specs on top of built-ins
        // (user wins). Loaded once at startup; runtime probe handles the rest.
        Self::load_tier_specs(&mut provider);
        Self {
            signatures,
            provider,
            state,
        }
    }

    /// Load `generated/` then `user/` tier specs into the provider (override order:
    /// user > generated > built-in).
    fn load_tier_specs(provider: &mut CompletionProvider) {
        if let Some(dir) = crate::completions::spec_tiers::generated_dir() {
            for spec in crate::completions::spec_tiers::load_dir(&dir) {
                provider.register(spec);
            }
        }
        if let Some(dir) = crate::completions::spec_tiers::user_dir() {
            for spec in crate::completions::spec_tiers::load_dir(&dir) {
                provider.register(spec);
            }
        }
    }

    /// Ensure a spec exists for `cmd` (Plan 315 runtime path):
    /// cache hit → register; else probe `cmd --help` → parse → write cache → register.
    /// Skips builtins/registered commands (don't probe `echo` etc.). Best-effort:
    /// any failure just leaves no spec (falls back to file completion).
    fn ensure_spec(&mut self, cmd: &str) {
        if self.provider.has_spec(cmd) {
            return;
        }
        // Don't probe shell builtins / registered commands.
        if crate::cmd::builtin::is_legacy_builtin(cmd)
            || self.signatures.iter().any(|s| s.name == cmd)
        {
            return;
        }
        // Plan 036: Don't probe script file paths — running `./script.ash --help`
        // would trigger the Windows "Open with" dialog instead of producing help
        // output. Paths (containing / or \) and known script extensions are
        // excluded from the `--help` probe.
        if is_likely_script_path(cmd) {
            return;
        }
        // 1. Cache tier.
        if let Some(spec) = crate::completions::spec_tiers::load_cache(cmd) {
            self.provider.register(spec);
            return;
        }
        // 2. Probe: run `cmd --help` and capture stdout regardless of exit code
        //    (many tools' --help exits non-zero while still printing usage).
        let cwd = self
            .state
            .lock()
            .map(|s| s.current_dir.clone())
            .unwrap_or_else(|_| PathBuf::from("."));
        let help = Self::capture_help(&format!("{} --help", cmd), &cwd);
        if !help.trim().is_empty() {
            let spec = help_parser::parse_help(cmd, &help);
            // Persist to cache (even if empty — acts as a "don't re-probe" marker).
            let _ = crate::completions::spec_tiers::write_cache(cmd, &spec);
            self.provider.register(spec);
        }
    }

    /// Run `cmd` via the platform shell, returning stdout regardless of exit code
    /// (for probing `--help`, which often prints usage to stdout then exits 1).
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

    /// Convert our Completion to reedline Suggestion
    fn completion_to_suggestion(completion: Completion) -> Suggestion {
        let value = completion.replacement.clone();
        // Use the real per-item description (e.g. "Reverse sort order" for
        // -r). Previously this used `completion.display`, which equals the
        // replacement, so descriptions were always dropped — flags/options
        // showed up as a bare list with no explanation.
        let description = completion.description.filter(|d| !d.is_empty() && d != &value);

        // Pass metadata via extra field:
        //   extra[0] = CompletionKind tag (for AshMenu coloring)
        //   extra[1] = "fuzzy" if non-prefix match
        let mut extra = Vec::new();
        extra.push(kind_tag(completion.kind));
        if !completion.is_prefix_match {
            extra.push("fuzzy".to_string());
        }

        Suggestion {
            value,
            description,
            extra: Some(extra),
            span: reedline::Span {
                start: 0,
                end: completion.replacement.len(),
            },
            append_whitespace: false,
            style: None,
            match_indices: None,
        }
    }

    /// Execute an external command and capture its stdout.
    /// Used as the command_executor closure for CompletionProvider.
    fn execute_command(cmd: &str, cwd: &Path) -> Result<String, String> {
        #[cfg(windows)]
        let output = std::process::Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(cwd)
            .output()
            .map_err(|e| e.to_string())?;

        #[cfg(not(windows))]
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(cwd)
            .output()
            .map_err(|e| e.to_string())?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

fn kind_tag(kind: crate::completions::CompletionKind) -> String {
    use crate::completions::CompletionKind;
    match kind {
        CompletionKind::Command => "command",
        CompletionKind::External => "external",
        CompletionKind::File => "file",
        CompletionKind::Directory => "directory",
        CompletionKind::Variable => "variable",
        CompletionKind::Flag => "flag",
        CompletionKind::Subcommand => "subcommand",
        CompletionKind::AiSuggested => "ai",
    }
    .to_string()
}

impl Completer for ShellCompleter {
    /// Complete the input line at the given position
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let trimmed = line[..pos].trim_end();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        // Calculate the span to replace: from the last word boundary to cursor position
        let start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let end = pos;

        // Plan 032: snapshot the shared completion state ONCE up front so both
        // the provider path and the ranking layer (M1.1) share one coherent
        // view of the user's context (cwd/last command/history/aliases).
        let snapshot = self
            .state
            .lock()
            .map(|s| StateSnapshot::from_state(&s))
            .unwrap_or_default();

        // If we have a first word and it's an external command with a spec,
        // route to the CompletionProvider
        if let Some(&cmd) = parts.first() {
            // Plan 315: ensure a spec is loaded for this command (cache/probe).
            self.ensure_spec(cmd);
            if self.provider.has_spec(cmd) {
                // Determine cursor part and prefix
                let ends_with_space = line[..pos].ends_with(|c: char| c.is_whitespace());
                let (cursor_part, prefix) = if ends_with_space {
                    // Cursor is after a space — completing a new token
                    (parts.len(), "")
                } else {
                    // Cursor is inside a token
                    let idx = parts.len().saturating_sub(1);
                    (idx, parts.last().copied().unwrap_or(""))
                };

                // Build parts with an empty slot for the cursor if needed
                let resolve_parts: Vec<&str> = if ends_with_space {
                    let mut p = parts.clone();
                    p.push("");
                    p
                } else {
                    parts.clone()
                };

                let ctx = CompletionContext {
                    current_dir: snapshot.current_dir.clone(),
                    command_executor: Box::new(Self::execute_command),
                    last_command: snapshot.last_command.clone(),
                    last_exit_code: snapshot.last_exit_code,
                    history: snapshot.history.clone(),
                    aliases: snapshot.aliases.clone(),
                };

                let mut completions = self.provider.resolve(
                    &resolve_parts,
                    cursor_part,
                    prefix,
                    &ctx,
                );

                // Plan 032 M2. merge any AI subcommand candidates the previous
                // keystroke's background fetch produced for THIS exact line
                // (`<cmd> <prefix>`). Strict full-line equality in the cache
                // layer guarantees a stale result from a shorter prefix can't
                // leak in once the user moves past the subcommand position.
                let subcmd_key = format!("{} {}", cmd, prefix);
                merge_ai_pending(ai_layer::Slot::Subcommand, &subcmd_key, &mut completions);

                // Plan 032 M2: when the static spec is thin at a subcommand
                // position, ask the local model for more candidates. Fire-and-
                // forget on a background thread; this call never blocks — the
                // result surfaces on the next keystroke. The cache layer
                // dedupes in-flight requests for the same (slot, key).
                if ai_completion_enabled()
                    && completions.len() < 3
                    && !cmd.starts_with('-')
                    && cursor_part >= 1
                {
                    ai_layer::trigger_ai_subcommand(cmd, prefix, snapshot.ai_snapshot());
                }

                if !completions.is_empty() {
                    return completions
                        .into_iter()
                        .map(|comp| {
                            let mut suggestion = Self::completion_to_suggestion(comp);
                            suggestion.span = reedline::Span { start, end };
                            suggestion
                        })
                        .collect();
                }

                // Provider found the spec but returned nothing — fall through
                // to file completion below
            }
        }

        // Default: use built-in completion system (registry signatures + file/path completion)
        let mut completions = crate::completions::get_completions_with_context(
            line,
            &self.signatures,
        );

        // Plan 032 M2: merge any pending NL→pipeline translation fired on a
        // previous keystroke at the command-name spot. The cache key is the
        // trimmed phrase (exactly what trigger_nl_to_pipeline stored), matched
        // by full equality — so an NL result only surfaces while the user is
        // still typing that same phrase.
        let phrase_key = line[..pos].trim();
        merge_ai_pending(ai_layer::Slot::NaturalLanguage, phrase_key, &mut completions);

        // Plan 032 M2: if we're at the command-name position and the first
        // token matches no known command/alias, the user may be typing a
        // natural-language phrase — ask the model to translate it. Only fire
        // when there are no useful local candidates (avoids spending a model
        // call when the static engine already has answers).
        if ai_completion_enabled()
            && is_command_name_position(line, pos)
            && first_token_is_unknown(line, &self.signatures, &snapshot.aliases)
            && completions.iter().all(|c| {
                c.kind != crate::completions::CompletionKind::Command
                    && c.kind != crate::completions::CompletionKind::External
            })
        {
            if !phrase_key.is_empty() {
                ai_layer::trigger_nl_to_pipeline(phrase_key, snapshot.ai_snapshot());
            }
        }

        // Plan 032 M1.1: context-aware ranking. Only reorder when we're
        // completing a COMMAND NAME (first token) — reordering subcommand/
        // flag/file lists would break their intentional order, and the
        // heuristics (history frequency / repo context / command coherence)
        // are only meaningful for top-level command names.
        if is_command_name_position(line, pos) && !completions.is_empty() {
            let ranking_ctx = CompletionContext {
                current_dir: snapshot.current_dir,
                command_executor: Box::new(|_, _| Ok(String::new())),
                last_command: snapshot.last_command,
                last_exit_code: snapshot.last_exit_code,
                history: snapshot.history,
                aliases: snapshot.aliases,
            };
            context_rank::rank(&mut completions, &ranking_ctx);
        }

        completions
            .into_iter()
            .map(|comp| {
                let mut suggestion = Self::completion_to_suggestion(comp);
                suggestion.span = reedline::Span { start, end };
                suggestion
            })
            .collect()
    }
}

/// Plan 032 M1.1: true when the cursor is completing the first token of the
/// line (a command name), i.e. there is no whitespace before the cursor yet
/// (or only leading whitespace). Used to gate context-aware ranking so it
/// never reorders subcommand/flag/file completions.
fn is_command_name_position(line: &str, pos: usize) -> bool {
    let before = &line[..pos];
    // Command name position = no interior whitespace before the cursor.
    !before.trim().contains(char::is_whitespace)
}

/// Plan 032 M2: whether AI completion is enabled. Defaults to `true` — AI is
/// an enhancement that degrades cleanly to the static engine when no daemon is
/// running, so we don't make users opt in. Disable via `ai.completion: false`
/// in the config.
fn ai_completion_enabled() -> bool {
    let cfg = crate::auto_config::load();
    crate::auto_config::get_bool(&cfg, "ai", "completion").unwrap_or(true)
}

/// Plan 032 M2: drain any AI candidates the background thread produced for
/// `slot` at the exact `key` (full-line equality) and append them to
/// `completions` (after local candidates, since they are suggestions rather
/// than authoritative). No-op when nothing is pending for this slot or the
/// cached key doesn't equal `key`. Position-aware: only the slot matching the
/// current cursor position is consulted, so a subcommand result can never be
/// served at a parameter position (or vice versa).
fn merge_ai_pending(slot: ai_layer::Slot, key: &str, completions: &mut Vec<Completion>) {
    if let Some(ai) = ai_layer::take_ai_pending(slot, key) {
        completions.extend(ai);
    }
}

/// Plan 032 M2: true when the first token of `line` is not a known command
/// (neither a built-in signature nor a registered alias). This is the gate for
/// natural-language→pipeline translation: only fire when the user typed
/// something that isn't already a recognized command word.
fn first_token_is_unknown(
    line: &str,
    signatures: &[CompletionSignature],
    aliases: &HashMap<String, String>,
) -> bool {
    let Some(first) = line.split_whitespace().next() else {
        return false; // empty line — nothing to translate
    };
    let is_builtin = signatures.iter().any(|s| s.name == first);
    let is_alias = aliases.contains_key(first);
    !is_builtin && !is_alias
}

/// Plan 036: Returns true if `cmd` looks like a script file path rather than
/// a CLI command in PATH. Probing `./script.ash --help` for completions would
/// trigger the Windows "Open with" dialog instead of producing help text.
fn is_likely_script_path(cmd: &str) -> bool {
    // Path separators — definitely a file path, not a command name
    if cmd.contains('/') || cmd.contains('\\') {
        return true;
    }
    // Common script extensions — running these without a proper handler
    // would open a file dialog or produce garbage.
    let lower = cmd.to_ascii_lowercase();
    lower.ends_with(".ash")
        || lower.ends_with(".sh")
        || lower.ends_with(".bash")
        || lower.ends_with(".zsh")
        || lower.ends_with(".ps1")
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".py")
        || lower.ends_with(".rb")
        || lower.ends_with(".js")
        || lower.ends_with(".at")
        || lower.ends_with(".as")
        || lower.ends_with(".au")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signatures() -> Vec<CompletionSignature> {
        use crate::completions::CompletionArgument;
        vec![
            CompletionSignature {
                name: "ls".into(),
                description: "List directory contents".into(),
                arguments: vec![
                    CompletionArgument {
                        name: "all".into(),
                        description: "Show all files".into(),
                        required: false,
                        is_flag: true,
                        short: Some('a'),
                        is_option: false,
                    },
                    CompletionArgument {
                        name: "long".into(),
                        description: "Long listing".into(),
                        required: false,
                        is_flag: true,
                        short: Some('l'),
                        is_option: false,
                    },
                ],
            },
            CompletionSignature {
                name: "grep".into(),
                description: "Search for patterns".into(),
                arguments: vec![],
            },
        ]
    }

    fn test_completer() -> ShellCompleter {
        ShellCompleter::new(
            test_signatures(),
            CompletionProvider::new(),
            Arc::new(Mutex::new(CompletionState::new(PathBuf::from(".")))),
        )
    }

    #[test]
    fn test_shell_completer_commands() {
        let mut completer = test_completer();
        let suggestions = completer.complete("l", 1);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.value == "ls"));
    }

    #[test]
    fn test_shell_completer_flags() {
        let mut completer = test_completer();
        let suggestions = completer.complete("ls --", 5);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.value == "--all"));
        assert!(suggestions.iter().any(|s| s.value == "--long"));
    }

    #[test]
    fn test_shell_completer_short_flags() {
        let mut completer = test_completer();
        let suggestions = completer.complete("ls -", 4);
        assert!(!suggestions.is_empty());
        // Should include both -a, -l and --all, --long
        assert!(suggestions.iter().any(|s| s.value == "-a"));
        assert!(suggestions.iter().any(|s| s.value == "-l"));
    }

    #[test]
    fn test_shell_completer_kind_tag_in_extra() {
        let mut completer = test_completer();
        let suggestions = completer.complete("ls --a", 6);
        let flag = suggestions.iter().find(|s| s.value == "--all").unwrap();
        assert_eq!(flag.extra.as_ref().unwrap()[0], "flag");
    }

    #[test]
    fn test_provider_routing_for_external_commands() {
        use ash_core::completions::{CompletionSpec, SubcommandSpec, FlagSpec as CoreFlagSpec};

        let mut provider = CompletionProvider::new();
        provider.register(
            CompletionSpec::new("git")
                .desc("Git version control")
                .subcommand(
                    SubcommandSpec::new("checkout")
                        .desc("Switch branches")
                        .flag(CoreFlagSpec::both("b", "branch").desc("Create new branch")),
                )
        );

        let mut completer = ShellCompleter::new(
            test_signatures(),
            provider,
            Arc::new(Mutex::new(CompletionState::new(PathBuf::from(".")))),
        );

        // "git " should show subcommands
        let suggestions = completer.complete("git ", 4);
        let names: Vec<&str> = suggestions.iter().map(|s| s.value.as_str()).collect();
        assert!(names.contains(&"checkout"));
    }

    // ── Plan 032 M0.3: CompletionState carries AI context plumbing ──────

    #[test]
    fn completion_state_new_defaults_ai_fields() {
        let s = CompletionState::new(PathBuf::from("/tmp"));
        assert_eq!(s.current_dir, PathBuf::from("/tmp"));
        assert!(s.last_command.is_none());
        assert!(s.last_exit_code.is_none());
        assert!(s.history.is_empty());
        assert!(s.aliases.is_empty());
    }

    #[test]
    fn state_snapshot_extracts_all_fields() {
        // The snapshot is the bridge from CompletionState → CompletionContext.
        // If this round-trips, the context-aware ranking/AI layers see what
        // the REPL wrote into the shared state.
        let mut state = CompletionState::new(PathBuf::from("/repo"));
        state.last_command = Some("git status".to_string());
        state.last_exit_code = Some(1);
        state.history = vec!["ls".into(), "cd src".into(), "git status".into()];
        state.aliases = {
            let mut m = HashMap::new();
            m.insert("g".to_string(), "git".to_string());
            m
        };

        let snap = StateSnapshot::from_state(&state);
        assert_eq!(snap.current_dir, PathBuf::from("/repo"));
        assert_eq!(snap.last_command.as_deref(), Some("git status"));
        assert_eq!(snap.last_exit_code, Some(1));
        assert_eq!(snap.history.len(), 3);
        assert_eq!(snap.aliases.get("g").map(String::as_str), Some("git"));
    }

    #[test]
    fn state_snapshot_defaults_empty_cwd_to_dot() {
        // A missing cwd must degrade to relative file completion, not panic.
        let mut state = CompletionState::new(PathBuf::new());
        state.current_dir = PathBuf::new();
        let snap = StateSnapshot::from_state(&state);
        assert_eq!(snap.current_dir, PathBuf::from("."));
    }

    // ── Plan 032 M2.3: AI layer integration + degradation ───────────────

    #[test]
    fn first_token_is_unknown_recognizes_builtins_and_aliases() {
        let sigs = test_signatures(); // ls, grep
        let mut aliases = HashMap::new();
        aliases.insert("g".to_string(), "git".to_string());

        // Known builtins → NOT unknown.
        assert!(!first_token_is_unknown("ls", &sigs, &aliases));
        assert!(!first_token_is_unknown("grep foo", &sigs, &aliases));
        // Known alias → NOT unknown.
        assert!(!first_token_is_unknown("g", &sigs, &aliases));
        // Unknown command → unknown (candidate for NL translation).
        assert!(first_token_is_unknown("列出最大文件", &sigs, &aliases));
        assert!(first_token_is_unknown("zzz", &sigs, &aliases));
        // Empty line → not unknown (nothing to translate).
        assert!(!first_token_is_unknown("", &sigs, &aliases));
    }

    #[test]
    fn complete_does_not_panic_without_daemon() {
        // The hallmark degradation guarantee: with no aaid daemon running
        // (the case in CI / this test), completion must not panic and must
        // still return the static/dynamic engine's candidates. The AI
        // background threads spawn, fail to connect, and write nothing.
        let mut completer = test_completer();
        // A known command — exercises the built-in path.
        let suggestions = completer.complete("l", 1);
        assert!(!suggestions.is_empty(), "static completion must still work");
        assert!(suggestions.iter().any(|s| s.value == "ls"));
    }

    #[test]
    fn complete_with_unknown_phrase_does_not_panic() {
        // Typing a natural-language phrase at the command spot fires a NL
        // translation background thread. With no daemon it degrades silently.
        // The call itself must return without hanging or panicking.
        let mut completer = test_completer();
        let _ = completer.complete("zzz未知命令", 12);
        // No assertion on content — we only assert it didn't panic/block.
    }

    #[test]
    fn ai_completion_enabled_defaults_true() {
        // AI completion is on by default (it degrades cleanly without a daemon).
        // We can't easily set config in a unit test, so just confirm the helper
        // returns a bool and doesn't panic.
        let _ = ai_completion_enabled();
    }

    // ── Plan 032 M2: end-to-end AI-merge in complete() (no daemon needed) ─
    // These inject a *finished* AI result into the cache (simulating the
    // background thread having completed) via `ai_layer::store`, then exercise
    // the real `complete()` → `merge_ai_pending` → `Suggestion` path. This is
    // the seam that was entirely missing in the first cut — the merge behavior
    // was never actually executed by any test.

    #[test]
    fn complete_merges_nl_translation_at_command_name_position() {
        // Inject an NL result for the phrase "列出文件" (as if the background
        // thread from the previous keystroke just finished). At the command-
        // name position, complete() should surface it as a suggestion.
        crate::completions::ai_layer::store(
            crate::completions::ai_layer::Slot::NaturalLanguage,
            "列出文件".to_string(),
            vec![Completion::with_kind(
                "ls",
                "ls",
                crate::completions::CompletionKind::AiSuggested,
            )],
        );
        let mut completer = test_completer();
        // Typing the same phrase at the command spot — the cache key
        // ("列出文件", trimmed) matches, so the AI candidate merges in.
        let suggestions = completer.complete("列出文件", 12);
        assert!(
            suggestions.iter().any(|s| s.value == "ls"),
            "NL translation should merge into suggestions at command-name position"
        );
    }

    #[test]
    fn stale_subcommand_result_does_not_leak_into_parameter_position() {
        // Bug #2 regression, end-to-end. Seed the Subcommand slot with a result
        // keyed on "git c" (as if requested while typing `git c`). Then complete
        // at a PARAMETER position on a line whose prefix is "git c" — the stale
        // candidates must NOT appear, because the merge key is the full
        // "<cmd> <prefix>" and won't equal "git checkout main"'s subcommand key.
        use ash_core::completions::{CompletionSpec, SubcommandSpec};
        let mut provider = CompletionProvider::new();
        provider.register(
            CompletionSpec::new("git").subcommand(
                SubcommandSpec::new("checkout").desc("Switch branches"),
            ),
        );
        let mut completer = ShellCompleter::new(
            test_signatures(),
            provider,
            Arc::new(Mutex::new(CompletionState::new(PathBuf::from(".")))),
        );
        crate::completions::ai_layer::store(
            crate::completions::ai_layer::Slot::Subcommand,
            "git c".to_string(), // the stale key
            vec![Completion::with_kind(
                "checkout",
                "checkout",
                crate::completions::CompletionKind::AiSuggested,
            )],
        );
        // Now at "git checkout main" — the subcommand merge key would be
        // "git main" (cmd=git, prefix=main), NOT "git c". The stale "git c"
        // entry must not surface.
        let suggestions = completer.complete("git checkout main", 17);
        assert!(
            !suggestions.iter().any(|s| {
                s.value == "checkout"
                    && s.extra
                        .as_deref()
                        .map_or(false, |e| e.iter().any(|tag| tag == "ai"))
            }),
            "stale 'git c' subcommand result must not leak into parameter position: {:?}",
            suggestions
        );
    }

    #[test]
    fn complete_merges_subcommand_candidates_when_key_matches() {
        // Positive case: when the Subcommand slot holds a result keyed exactly
        // on the current "<cmd> <prefix>", complete() merges it. This proves
        // the happy path of the per-position merge actually works end-to-end.
        use ash_core::completions::{CompletionSpec, SubcommandSpec};
        let mut provider = CompletionProvider::new();
        provider.register(
            CompletionSpec::new("git").subcommand(
                SubcommandSpec::new("checkout").desc("Switch branches"),
            ),
        );
        let mut completer = ShellCompleter::new(
            test_signatures(),
            provider,
            Arc::new(Mutex::new(CompletionState::new(PathBuf::from(".")))),
        );
        // The subcommand merge key is "git c" (cmd "git", prefix "c"). Seed it.
        crate::completions::ai_layer::store(
            crate::completions::ai_layer::Slot::Subcommand,
            "git c".to_string(),
            vec![Completion::with_kind(
                "cherry-pick",
                "cherry-pick",
                crate::completions::CompletionKind::AiSuggested,
            )],
        );
        let suggestions = completer.complete("git c", 5);
        assert!(
            suggestions.iter().any(|s| s.value == "cherry-pick"),
            "AI subcommand candidate should merge when the key matches: {:?}",
            suggestions
        );
    }
}
