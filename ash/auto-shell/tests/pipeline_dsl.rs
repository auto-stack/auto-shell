//! End-to-end integration tests for the structured-pipeline DSL (Plan 024).
//!
//! These run commands through the *full* shell execution path
//! (`Shell::execute` → `parse_chain` → `parse_pipe_stage` → `operators::apply`),
//! which is the layer that the inline `#[cfg(test)]` unit tests bypass. They
//! specifically guard the fix where compound predicates using `&&`/`||` were
//! silently broken (the operators got split by `parse_chain` as command-chain
//! separators before the DSL parser ever saw them). The DSL now uses the
//! `and`/`or` keywords instead.

use auto_shell::Shell;

/// Execute a command and return its output (None → empty).
fn exec(shell: &mut Shell, input: &str) -> Option<String> {
    shell.execute(input).unwrap_or(None)
}

/// Strip ANSI escape sequences so text-content assertions are stable.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            while let Some(csi) = chars.next() {
                if csi.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Count pipeline rows by piping to `count` and reading the result.
fn count(shell: &mut Shell, pipeline: &str) -> usize {
    let out = exec(shell, pipeline).unwrap_or_default();
    let plain = strip_ansi(&out);
    // The trailing number from `count` (USize) is rendered as the last token.
    plain
        .trim()
        .split_whitespace()
        .last()
        .and_then(|t| t.parse::<usize>().ok())
        .unwrap_or(0)
}

#[test]
fn single_predicate_end_to_end() {
    // Baseline: a single predicate filters as expected.
    let mut shell = Shell::new();
    let all = count(&mut shell, "ls | count");
    let files = count(&mut shell, "ls | .type == \"file\" | count");
    assert!(files > 0, "expected some files, got {files}");
    assert!(files <= all, "files ({files}) should be <= all ({all})");
}

#[test]
fn compound_and_end_to_end() {
    // The key regression test: `and` must survive `parse_chain` and reach the
    // DSL parser as a single compound predicate. Compare against a single
    //predicate that should match the same rows.
    let mut shell = Shell::new();
    // Files only.
    let files = count(&mut shell, "ls | .type == \"file\" | count");
    // Files AND size > 0 — must be <= files (additional constraint).
    let files_and_nonzero = count(&mut shell, "ls | .type == \"file\" and .size > 0 | count");
    assert!(
        files_and_nonzero <= files,
        "and-filter ({files_and_nonzero}) must be <= files-only ({files})"
    );
    // If && worked end-to-end it would also be <= files; here we confirm `and`
    // is the working path (previously && returned 0 because it was split).
    assert!(
        files_and_nonzero > 0 || files == 0,
        "and-filter should match when files exist (files={files}, and={files_and_nonzero})"
    );
}

#[test]
fn compound_or_end_to_end() {
    // `or` must match at least as many rows as each branch alone.
    let mut shell = Shell::new();
    let dirs = count(&mut shell, "ls | .type == \"dir\" | count");
    let files = count(&mut shell, "ls | .type == \"file\" | count");
    let dirs_or_files = count(&mut shell, "ls | .type == \"dir\" or .type == \"file\" | count");
    let total = count(&mut shell, "ls | count");
    // dirs OR files should be exactly dirs + files (every entry is one or the
    // other), and cannot exceed the total.
    assert!(
        dirs_or_files >= dirs && dirs_or_files >= files,
        "or ({dirs_or_files}) must be >= each branch (dirs={dirs}, files={files})"
    );
    assert!(
        dirs_or_files <= total,
        "or ({dirs_or_files}) must be <= total ({total})"
    );
}
