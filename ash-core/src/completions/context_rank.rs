//! Context-aware completion ranking (Plan 032 M1.1).
//!
//! A pure-local heuristic that reorders completion candidates by how relevant
//! they are to the user's current context — the working directory, recent
//! history, and the last command. This layer runs *after* the static/dynamic
//! resolver produces candidates and *before* they are turned into menu
//! suggestions. It makes **no AI calls** (zero added latency): it only weighs
//! the already-available [`CompletionContext`].
//!
//! Heuristics (additive score, higher = more relevant):
//! - **History frequency**: a command that appears often in the recent history
//!   is more likely what the user wants (`+0.5` per occurrence).
//! - **Repository context**: inside a git repo, `git`-prefixed candidates get a
//!   boost (`+2.0`).
//! - **Command coherence**: if the last command relates to a candidate
//!   (e.g. `cargo build` → `cargo test`), boost it (`+1.0`).
//!
//! The sort is **stable** in spirit: candidates with equal scores keep their
//! resolver order (the resolver already orders by prefix-match quality).

use super::{Completion, CompletionContext};
use std::cmp::Ordering;
use std::path::Path;

/// Reorder `completions` in place by context relevance (higher score first).
///
/// Safe to call on an empty slice. Does not mutate entries — only their order.
pub fn rank(completions: &mut [Completion], ctx: &CompletionContext) {
    if completions.len() <= 1 {
        return;
    }
    completions.sort_by(|a, b| {
        // Higher score first; equal scores preserve input order (sort_by is
        // stable, so the resolver's prefix-match ordering wins on ties).
        let sa = context_score(candidate_key(&a.replacement), ctx);
        let sb = context_score(candidate_key(&b.replacement), ctx);
        sb.partial_cmp(&sa).unwrap_or(Ordering::Equal)
    });
}

/// Map a candidate's replacement to the "command word" we score against.
///
/// Completion candidates arrive in different shapes: a bare command (`git`),
/// a subcommand (`checkout`), a flag (`--force`). Scoring the full replacement
/// against history/last-command heuristics works best on the leading command
/// word, so we take the first whitespace-delimited token. For flags we fall
/// back to the whole string (flags have no command word).
fn candidate_key(replacement: &str) -> &str {
    // Strip a leading flag prefix so e.g. `--force` is keyed as `--force`
    // (no first-token stripping needed) — but a `git checkout` subcommand
    // replacement is keyed by its own name. For bare names this is a no-op.
    replacement.split_whitespace().next().unwrap_or(replacement)
}

/// Compute the relevance score of `cmd` against the context.
fn context_score(cmd: &str, ctx: &CompletionContext) -> f64 {
    let mut score: f64 = 0.0;

    // History frequency — how often does this command appear recently?
    // Match on the leading word so `git` credits both `git` and `git status`.
    let freq = ctx
        .history
        .iter()
        .filter(|h| leading_word(h) == cmd || leading_word(h).starts_with(cmd))
        .count();
    score += freq as f64 * 0.5;

    // Repository context — inside a git repo, git-ish candidates win.
    if is_git_repo(&ctx.current_dir) && looks_git_related(cmd) {
        score += 2.0;
    }

    // Coherence with the last command — reward the next logical step.
    if let Some(last) = &ctx.last_command {
        if are_related(last, cmd) {
            score += 1.0;
        }
    }

    score
}

/// Extract the leading command word from a history entry (e.g. `git status` →
/// `git`). Aliases aren't expanded here; the raw leading token is good enough
/// for frequency counting.
fn leading_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// True if `dir` (looks like) a git working tree — presence of a `.git` entry.
fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Heuristic: does `cmd` relate to git? Covers `git` itself plus a few
/// git-adjacent helpers users often invoke inside repos.
fn looks_git_related(cmd: &str) -> bool {
    cmd == "git" || cmd.starts_with("git")
}

/// Heuristic: is `cmd` a likely follow-up to `last`? Rewards same-tool
/// repetition (`cargo build` → `cargo test`) and a small set of known
/// coherent transitions. Intentionally cheap and conservative — false
/// positives only nudge ordering, never hide candidates.
fn are_related(last: &str, cmd: &str) -> bool {
    let last_word = leading_word(last);
    if last_word.is_empty() {
        return false;
    }
    // Same leading tool → coherent (cargo build → cargo test/run).
    if cmd == last_word || last_word == cmd {
        return true;
    }
    // Known coherent transitions.
    matches!(
        (last_word, cmd),
        ("cd", "ls")
            | ("ls", "cd")
            | ("git", "git")
            | ("make", "./a.out")
            | ("cargo", "cargo")
            | ("npm", "npm")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn ctx_in(dir: &str, history: &[&str], last: Option<&str>) -> CompletionContext {
        CompletionContext {
            current_dir: PathBuf::from(dir),
            command_executor: Box::new(|_, _| Ok(String::new())),
            last_command: last.map(String::from),
            last_exit_code: None,
            history: history.iter().map(|s| s.to_string()).collect(),
            aliases: HashMap::new(),
        }
    }

    fn comp(label: &str) -> Completion {
        Completion::with_kind(label, label, crate::completions::CompletionKind::Command)
    }

    /// Create a unique temp dir that looks like a git repo (has a `.git`
    /// entry), so git-context tests don't depend on the test runner's cwd.
    fn mk_git_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "ash-032-gitrepo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[test]
    fn empty_and_single_are_noops() {
        let mut empty: Vec<Completion> = vec![];
        let ctx = ctx_in("/tmp", &[], None);
        rank(&mut empty, &ctx);
        assert!(empty.is_empty());

        let mut one = vec![comp("ls")];
        rank(&mut one, &ctx);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn history_frequency_promotes_frequent_command() {
        // ls appears 3x, grep 1x in recent history → ls ranks first.
        let mut completions = vec![comp("grep"), comp("ls")];
        let ctx = ctx_in("/tmp", &["ls", "ls", "ls", "grep"], None);
        rank(&mut completions, &ctx);
        assert_eq!(completions[0].replacement, "ls");
        assert_eq!(completions[1].replacement, "grep");
    }

    #[test]
    fn git_repo_context_promotes_git() {
        // Inside a git repo, `git` (boosted) outranks `grep` even though both
        // have zero history. Use a temp dir with a `.git` marker so the test
        // doesn't depend on where `cargo test` happens to run.
        let repo = mk_git_dir();
        let mut completions = vec![comp("grep"), comp("git")];
        let ctx = ctx_in(repo.to_str().unwrap_or("."), &[], None);
        rank(&mut completions, &ctx);
        assert_eq!(completions[0].replacement, "git");
    }

    #[test]
    fn last_command_coherence_promotes_related() {
        // After `cargo build`, `cargo` is coherent → outranks `ls`.
        let mut completions = vec![comp("ls"), comp("cargo")];
        let ctx = ctx_in("/tmp", &[], Some("cargo build"));
        rank(&mut completions, &ctx);
        assert_eq!(completions[0].replacement, "cargo");
    }

    #[test]
    fn non_git_dir_does_not_promote_git() {
        // In a plain dir with no .git and no history, ordering is preserved
        // (stable sort, equal scores). We can't assert exact equality of
        // score, but git must NOT leap ahead purely on the repo heuristic.
        use std::env;
        let tmp = env::temp_dir(); // not a git repo
        let mut completions = vec![comp("aaa"), comp("git"), comp("zzz")];
        let ctx = ctx_in(tmp.to_str().unwrap_or("/tmp"), &[], None);
        rank(&mut completions, &ctx);
        // No boost applied → relative order unchanged by ranking heuristics.
        assert_eq!(
            completions.iter().map(|c| c.replacement.as_str()).collect::<Vec<_>>(),
            vec!["aaa", "git", "zzz"]
        );
    }

    #[test]
    fn candidate_key_strips_to_first_word() {
        // A subcommand replacement like "git status" is keyed by "git" so it
        // benefits from git-repo boosting.
        let repo = mk_git_dir();
        let mut completions = vec![comp("grep"), comp("git status")];
        let ctx = ctx_in(repo.to_str().unwrap_or("."), &[], None);
        rank(&mut completions, &ctx);
        assert_eq!(completions[0].replacement, "git status");
    }
}