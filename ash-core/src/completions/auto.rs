//! AutoLang and shell variable completion
//!
//! Provides completion for shell variables ($name, ${name}).

use crate::completions::{Completion, CompletionKind};

/// Complete shell variables (`$name`, `${name}`).
///
/// Plan 032 M3.2: now offers the user's *real* environment variables (via
/// `std::env::vars`), merged with a small always-present fallback list so the
/// common vars (`PATH`/`HOME`/…) still show even if unset in the current
/// process. Previously this returned a hardcoded 11-name list.
///
/// Only completes when the input contains `$`. Candidates are the live
/// environment variables plus a fixed fallback set, filtered to the typed
/// prefix and deduplicated.
pub fn complete_auto(input: &str) -> Vec<Completion> {
    // Only complete if input starts with $
    if !input.contains('$') {
        return Vec::new();
    }

    // Find the last $ to complete
    let last_dollar_idx = input.rfind('$').unwrap_or(0);
    let var_part = &input[last_dollar_idx + 1..];

    // Check if it's braced syntax ${...}
    let is_braced = var_part.starts_with('{');
    let partial = if is_braced {
        var_part.trim_start_matches('{')
    } else {
        var_part
    };

    // Gather candidate variable names: real env vars + a fallback list of
    // common ones (so PATH/HOME still appear even if somehow unset).
    let mut names: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
    for &fallback in COMMON_VARS {
        names.push(fallback.to_string());
    }
    names.sort();
    names.dedup();

    let mut completions = Vec::new();
    for var in names {
        if var.starts_with(partial) {
            let replacement = if is_braced {
                // Build ${VAR} manually: "${" + VAR + "}" → "${VAR}"
                let mut result = "${".to_string();
                result.push_str(&var);
                result.push('}');
                result
            } else {
                format!("${}", var)
            };

            completions.push(Completion::with_kind(var, replacement, CompletionKind::Variable));
        }
    }

    completions
}

/// Always-present variable names offered even when unset in this process.
/// Keeps the common-case UX stable regardless of the host environment.
const COMMON_VARS: &[&str] = &[
    "PATH", "HOME", "USER", "SHELL", "PWD", "TERM",
    "EDITOR", "VISUAL", "PAGER", "LANG", "LC_ALL",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_auto_dollar() {
        let completions = complete_auto("$P");
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.display == "PATH"));
    }

    #[test]
    fn test_complete_auto_braced() {
        let completions = complete_auto("${HO");
        println!("Completions: {:?}", completions);
        assert!(!completions.is_empty());
        // Check that replacement contains the properly formatted ${HOME}
        assert!(completions.iter().any(|c| c.replacement.contains("${HOME") && c.replacement.ends_with('}')));
    }

    #[test]
    fn test_complete_no_dollar() {
        let completions = complete_auto("PATH");
        assert!(completions.is_empty());
    }

    #[test]
    fn test_complete_partial_match() {
        let completions = complete_auto("$U");
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.display == "USER"));
    }

    #[test]
    fn test_complete_no_match() {
        // A prefix that matches neither the fallback list nor (in practice) any
        // env var present in the test runner. Using a long, unique prefix makes
        // this robust against the host's real environment.
        let completions = complete_auto("$ASH_032_DEFINITELY_NOT_SET_VAR_");
        assert!(completions.is_empty());
    }

    // ── Plan 032 M3.2: real environment variables ───────────────────────

    #[test]
    fn real_env_var_is_completed() {
        // Set a uniquely-named env var and confirm it surfaces as a completion.
        // (Tests run single-threaded by default, so this is safe.)
        let key = "ASH_032_TEST_VAR";
        std::env::set_var(key, "1");
        let completions = complete_auto("$ASH_032_TEST");
        std::env::remove_var(key);
        assert!(
            completions.iter().any(|c| c.display == "ASH_032_TEST_VAR"),
            "real env var should be completed, got: {:?}",
            completions
        );
    }

    #[test]
    fn fallback_vars_appear_even_if_unset() {
        // PATH is in the fallback list; it must complete regardless of env.
        let completions = complete_auto("$PAT");
        assert!(completions.iter().any(|c| c.display == "PATH"));
    }

    #[test]
    fn real_and_fallback_are_deduplicated() {
        // If a var is both real and in the fallback list (e.g. PATH), it must
        // appear exactly once.
        let completions = complete_auto("$PATH");
        let count = completions.iter().filter(|c| c.display == "PATH").count();
        assert_eq!(count, 1, "PATH should appear exactly once: {:?}", completions);
    }
}
