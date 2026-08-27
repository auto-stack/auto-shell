//! Reedline Completer integration
//!
//! Provides integration between auto-shell's completion system and reedline's Tab completion.
//! The completer holds:
//! - A snapshot of CommandRegistry signatures for built-in commands
//! - A CompletionProvider for external command specs (git, cargo, etc.)
//! - Shared state (current_dir) updated by the REPL after each command

use auto_shell::completions::{Completion, CompletionSignature};
use ash_core::completions::CompletionProvider;
use reedline::{Completer, Suggestion};
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// user > generated > built-in). Plan 033: plugin `completions/` dirs load
    /// last, so installed plugins take the highest precedence.
    fn load_tier_specs(provider: &mut CompletionProvider) {
        if let Some(dir) = auto_shell::completions::spec_tiers::generated_dir() {
            for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
                provider.register(spec);
            }
        }
        if let Some(dir) = auto_shell::completions::spec_tiers::user_dir() {
            for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
                provider.register(spec);
            }
        }
        // Plan 033: plugin-contributed completion specs (highest precedence).
        for dir in auto_shell::plugin::loader::enabled_plugin_completion_dirs() {
            for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
                provider.register(spec);
            }
        }
    }

    /// Convert our Completion to reedline Suggestion. (Plan 041 M7: this is the
    /// only reedline-specific transform left; all orchestration is in the engine.)
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
}

fn kind_tag(kind: auto_shell::completions::CompletionKind) -> String {
    use auto_shell::completions::CompletionKind;
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
    /// Complete the input line at the given position.
    ///
    /// Plan 041 M7: this is now a thin reedline adapter. All completion
    /// orchestration (provider resolve / AI merge / static completion / ranking)
    /// lives in [`auto_shell::completions::engine::complete`], shared with the
    /// GUI. Here we only: snapshot state → call the engine → convert
    /// `Completion` → reedline `Suggestion` with the right span.
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        // The span to replace: from the last word boundary to the cursor.
        let start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let end = pos;

        // Snapshot the shared completion state once (cwd/last cmd/history/aliases).
        let snapshot = self
            .state
            .lock()
            .map(|s| StateSnapshot::from_state(&s))
            .unwrap_or_default();

        let ctx = auto_shell::completions::engine::CompletionCtx {
            current_dir: snapshot.current_dir,
            last_command: snapshot.last_command,
            last_exit_code: snapshot.last_exit_code,
            history: snapshot.history,
            aliases: snapshot.aliases,
        };

        let completions = auto_shell::completions::engine::complete(
            line,
            pos,
            &self.signatures,
            &mut self.provider,
            &ctx,
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signatures() -> Vec<CompletionSignature> {
        use auto_shell::completions::CompletionArgument;
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
    // (Plan 041 M7: the first_token_is_unknown / ai_completion_enabled unit
    // tests were removed — those helpers sank to the engine module, and the
    // integration tests below exercise the full complete() path that uses them.)

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

    // ── Plan 032 M2: end-to-end AI-merge in complete() (no daemon needed) ─
    // These inject a *finished* AI result into the cache (simulating the
    // background thread having completed) via `ai_layer::store`, then exercise
    // the real `complete()` → `merge_ai_pending` → `Suggestion` path. This is
    // the seam that was entirely missing in the first cut — the merge behavior
    // was never actually executed by any test.

    #[test]
    fn complete_merges_nl_translation_at_command_name_position() {
        // Touch the process-global AI cache → serialize against other such tests.
        let _g = auto_shell::completions::ai_layer::test_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Inject an NL result for the phrase "列出文件" (as if the background
        // thread from the previous keystroke just finished). At the command-
        // name position, complete() should surface it as a suggestion.
        auto_shell::completions::ai_layer::store(
            auto_shell::completions::ai_layer::Slot::NaturalLanguage,
            "列出文件".to_string(),
            vec![Completion::with_kind(
                "ls",
                "ls",
                auto_shell::completions::CompletionKind::AiSuggested,
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
        // Touch the process-global AI cache → serialize against other such tests.
        let _g = auto_shell::completions::ai_layer::test_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        auto_shell::completions::ai_layer::store(
            auto_shell::completions::ai_layer::Slot::Subcommand,
            "git c".to_string(), // the stale key
            vec![Completion::with_kind(
                "checkout",
                "checkout",
                auto_shell::completions::CompletionKind::AiSuggested,
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
        // Touch the process-global AI cache → serialize against other such tests.
        let _g = auto_shell::completions::ai_layer::test_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        auto_shell::completions::ai_layer::store(
            auto_shell::completions::ai_layer::Slot::Subcommand,
            "git c".to_string(),
            vec![Completion::with_kind(
                "cherry-pick",
                "cherry-pick",
                auto_shell::completions::CompletionKind::AiSuggested,
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
