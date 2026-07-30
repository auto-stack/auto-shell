//! Plan 031 M0.1 — end-to-end regression for the silent-data-loss Stream bug.
//!
//! Before the fix, an `ExternalStream` (output of an external command) feeding
//! a structured-pipeline DSL stage (`count`/`uniq`/`sort`/`reverse`/...) was
//! silently dropped to an empty array by `Shell::execute_pipeline_with_auto`'s
//! DSL dispatch. So `printf 'a\nb\nc\n' | count` returned `0` instead of `3`.
//!
//! These run the *full* shell execution path and assert data is no longer
//! lost. The unit tests in `ash-core` (`atom_pipeline::tests::dsl_input_*`)
//! cover the `into_dsl_input` conversion directly (cross-platform via `sort`);
//! the cases below additionally guard the end-to-end dispatch wiring.

use auto_shell::Shell;

/// Execute a command and return its output (None → empty).
fn exec(shell: &mut Shell, input: &str) -> Option<String> {
    shell.execute(input).unwrap_or(None)
}

/// A cross-platform external command that emits `count` lines to stdout, used
/// as a reliable producer of an `ExternalStream`. Returns the full pipeline
/// text ready to feed `Shell::execute`.
fn external_producer(lines: &[&str]) -> String {
    #[cfg(unix)]
    {
        let body = lines.join("\\n"); // literal \n for printf's format string
        format!("printf '{body}\\n'")
    }
    #[cfg(windows)]
    {
        // `findstr` is present on all modern Windows; search a needle that
        // matches every line so it acts as a pass-through emitter. We echo via
        // `cmd /c` to guarantee a real external process (ash's `echo` is a
        // builtin and would NOT produce an ExternalStream).
        let joined = lines.join(" & echo ");
        format!("cmd /c \"echo {joined}\"")
    }
    #[cfg(not(any(unix, windows)))]
    {
        panic!("unsupported platform for stream_bug_fix test");
    }
}

#[test]
fn external_to_count_does_not_lose_rows() {
    let mut shell = Shell::new();
    let producer = external_producer(&["apple", "banana", "cherry"]);
    let pipeline = format!("{producer} | count");
    let out = exec(&mut shell, &pipeline).unwrap_or_default();
    let n: usize = out
        .trim()
        .split_whitespace()
        .last()
        .and_then(|t| t.parse::<usize>().ok())
        .unwrap_or(0);
    assert_eq!(n, 3, "`{pipeline}` should count 3 rows, got {n} (data loss?)");
}

#[test]
fn external_to_uniq_keeps_rows() {
    let mut shell = Shell::new();
    let producer = external_producer(&["a", "a", "b", "c"]);
    let pipeline = format!("{producer} | uniq");
    let out = exec(&mut shell, &pipeline).unwrap_or_default();
    // uniq on the array yields the deduplicated rows; none of the original
    // rows may vanish entirely (the pre-fix bug produced empty output).
    assert!(out.contains('a'), "`{pipeline}` lost row 'a': {out:?}");
    assert!(out.contains('b'), "`{pipeline}` lost row 'b': {out:?}");
    assert!(out.contains('c'), "`{pipeline}` lost row 'c': {out:?}");
}
