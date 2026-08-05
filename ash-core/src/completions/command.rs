//! Command name completion
//!
//! Provides completion for built-in shell commands using CompletionSignature
//! metadata from the CommandRegistry. Falls back to a minimal hardcoded list
//! when no signatures are available (e.g., in tests without a registry).

use crate::completions::types::CompletionSignature;
use crate::completions::{Completion, CompletionKind};

/// Minimal fallback list when no CompletionSignatures are available.
const FALLBACK_COMMANDS: &[&str] = &[
    "ls", "cd", "pwd", "mkdir", "rm", "mv", "cp",
    "sort", "uniq", "head", "tail", "wc", "grep",
    "echo", "help", "exit",
];

/// Hardcoded builtins handled inside `Shell::execute_inner` (not in the
/// CommandRegistry). These must be added to command-name completions on top of
/// the registry signatures, or they'd be invisible to Tab completion (e.g.
/// `b`/bookmark, `up`, `alias`, `pushd`). Mirrors the dispatch in
/// `auto-shell/src/shell.rs:execute_inner` (lines 608-711). Plan 041 M8.
const HARDCODED_BUILTINS: &[(&str, &str)] = &[
    ("cd", "Change directory"),
    ("alias", "Set or list aliases"),
    ("unalias", "Remove an alias"),
    ("source", "Execute a file in the current shell"),
    ("pushd", "Push directory onto the stack"),
    ("popd", "Pop directory from the stack"),
    ("dirs", "List the directory stack"),
    ("jobs", "List background jobs"),
    ("fg", "Bring a job to the foreground"),
    ("bg", "Resume a job in the background"),
    ("suspend", "Suspend the shell"),
    ("def", "Define an Auto function"),
    ("hook", "Manage shell hooks"),
    ("abbr", "Manage abbreviations"),
    ("config", "View/edit shell config"),
    ("bind", "Manage key bindings"),
    ("up", "Go up N directories"),
    ("u", "Alias for `up`"),
    ("b", "Bookmark command (add/del/list/jump)"),
    ("set", "Set a variable"),
    ("export", "Export an environment variable"),
    ("unset", "Unset a variable"),
    ("env", "Show/set environment"),
    ("env.path", "Show the PATH entries"),
    ("path", "Show/set the PATH"),
    ("completions", "Manage completion specs"),
    ("use", "Import a module"),
    ("exit", "Exit the shell"),
    ("quit", "Exit the shell"),
    ("q", "Exit the shell"),
];

/// Complete command names from registry signatures + hardcoded builtins.
///
/// When `signatures` is non-empty, uses the full registry data (77+ commands
/// with descriptions) **plus** the hardcoded builtins from [`HARDCODED_BUILTINS`]
/// (which aren't in the registry). Otherwise falls back to a minimal list.
pub fn complete_command(input: &str, signatures: &[CompletionSignature]) -> Vec<Completion> {
    let trimmed = input.trim();

    // Determine prefix: after pipe, use the text after the pipe
    let prefix = if let Some(pipe_idx) = trimmed.rfind('|') {
        trimmed[pipe_idx + 1..].trim()
    } else {
        trimmed
    };

    if !signatures.is_empty() {
        complete_from_signatures(prefix, signatures)
    } else {
        complete_from_fallback(prefix)
    }
}

fn complete_from_signatures(prefix: &str, signatures: &[CompletionSignature]) -> Vec<Completion> {
    let mut completions = Vec::new();
    // Track names already added so a hardcoded builtin that's also in the
    // registry doesn't appear twice (registry description wins).
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for sig in signatures {
        if sig.name.starts_with(prefix) || prefix.is_empty() {
            completions.push(Completion::with_description(
                sig.name.clone(),
                sig.name.clone(),
                sig.description.clone(),
                CompletionKind::Command,
            ));
            seen.insert(sig.name.as_str());
        }
    }

    // Plan 041 M8: add hardcoded builtins (b/up/alias/pushd/…) that aren't in
    // the registry but are handled by execute_inner. Without this they'd be
    // invisible to Tab completion even though they work when typed.
    for (name, desc) in HARDCODED_BUILTINS {
        if !seen.contains(*name) && (name.starts_with(prefix) || prefix.is_empty()) {
            completions.push(Completion::with_description(
                (*name).to_string(),
                (*name).to_string(),
                (*desc).to_string(),
                CompletionKind::Command,
            ));
        }
    }

    completions
}

fn complete_from_fallback(prefix: &str) -> Vec<Completion> {
    let mut completions = Vec::new();

    for &cmd in FALLBACK_COMMANDS {
        if cmd.starts_with(prefix) || prefix.is_empty() {
            completions.push(Completion::with_kind(cmd, cmd, CompletionKind::Command));
        }
    }

    completions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completions::types::CompletionArgument;

    fn test_signatures() -> Vec<CompletionSignature> {
        vec![
            CompletionSignature {
                name: "ls".into(),
                description: "List directory contents".into(),
                arguments: vec![CompletionArgument {
                    name: "all".into(),
                    description: "Show all files".into(),
                    required: false,
                    is_flag: true,
                    short: Some('a'),
                    is_option: false,
                }],
            },
            CompletionSignature {
                name: "grep".into(),
                description: "Search for patterns".into(),
                arguments: vec![],
            },
            CompletionSignature {
                name: "cd".into(),
                description: "Change directory".into(),
                arguments: vec![],
            },
        ]
    }

    #[test]
    fn test_complete_command_empty() {
        let sigs = test_signatures(); // ls, grep, cd
        let completions = complete_command("", &sigs);
        // Plan 041 M8: signatures (3) + hardcoded builtins not already in sigs.
        // `cd` is in both, so it's not double-counted. The exact count is
        // signatures.len() + HARDCODED_BUILTINS.len() - duplicates (cd).
        let expected = sigs.len() + HARDCODED_BUILTINS.len() - 1; // -1 for cd dup
        assert_eq!(completions.len(), expected);
        // The hardcoded builtins (b/up/alias/…) must be present.
        assert!(completions.iter().any(|c| c.display == "b"));
        assert!(completions.iter().any(|c| c.display == "up"));
        assert!(completions.iter().any(|c| c.display == "alias"));
        // No duplicates (cd appears in both sigs and HARDCODED_BUILTINS).
        let cd_count = completions.iter().filter(|c| c.display == "cd").count();
        assert_eq!(cd_count, 1, "cd should appear once, not twice");
    }

    #[test]
    fn test_complete_command_partial() {
        let sigs = test_signatures();
        let completions = complete_command("l", &sigs);
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].display, "ls");
        assert_eq!(completions[0].description.as_deref(), Some("List directory contents"));
    }

    #[test]
    fn test_complete_command_no_match() {
        let sigs = test_signatures();
        let completions = complete_command("xyz", &sigs);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_complete_command_after_pipe() {
        let sigs = test_signatures();
        let completions = complete_command("echo test | gr", &sigs);
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.display == "grep"));
    }

    #[test]
    fn test_fallback_no_signatures() {
        let completions = complete_command("ls", &[]);
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.display == "ls"));
    }
}
