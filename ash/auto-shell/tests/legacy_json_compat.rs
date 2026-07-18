//! Plan 028 M2.6: Regression — the existing `ash -c "..." [--json]` interface
//! from Plan 007 must keep working unchanged. Plan 028 is purely additive;
//! these tests guard against accidental breakage of the legacy agent path.

use std::process::Command;

/// Locate the built `ash` binary (same logic as agent_cli.rs).
fn ash_bin() -> String {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return b;
    }
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dir = std::path::Path::new(manifest)
        .parent()
        .unwrap()
        .join("target")
        .join("debug");
    let name = if cfg!(windows) { "ash.exe" } else { "ash" };
    dir.join(name).to_string_lossy().into_owned()
}

fn run_ash(args: &[&str]) -> (i32, String, String) {
    let bin = ash_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn '{}': {}", bin, e));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn legacy_c_flag_executes_command() {
    // `ash -c "echo hi"` (Plan 007 behavior, no --json) prints the output.
    let (code, stdout, _stderr) = run_ash(&["-c", "echo regression_check"]);
    assert_eq!(code, 0, "legacy -c should exit 0 on success");
    assert!(
        stdout.contains("regression_check"),
        "legacy -c should print echo output, got: {}",
        stdout
    );
}

#[test]
fn legacy_c_json_flag_still_works() {
    // `ash -c "echo hi" --json` must still produce output (Plan 007 JSON mode).
    // We don't assert the exact JSON shape (that's Plan 007's contract) — only
    // that it doesn't error and produces something non-empty, proving the
    // --json global flag wasn't broken by the new `agent` subcommand.
    let (code, stdout, stderr) = run_ash(&["-c", "echo json_mode", "--json"]);
    assert_eq!(code, 0, "legacy -c --json failed; stderr: {}", stderr);
    assert!(!stdout.is_empty(), "legacy -c --json produced no output");
}

#[test]
fn legacy_c_failure_exits_nonzero() {
    // A command that errors must still propagate a non-zero exit code (Plan 007).
    let (code, _stdout, _stderr) = run_ash(&["-c", "nonexistent_command_xyz_123"]);
    assert_ne!(code, 0, "legacy -c should exit nonzero on command failure");
}

#[test]
fn legacy_c_with_no_arg_is_usage_error() {
    // `ash -c` with no command must exit 2 (usage error), per main.rs.
    let (code, _stdout, stderr) = run_ash(&["-c"]);
    assert_eq!(code, 2, "ash -c with no arg should exit 2");
    assert!(stderr.contains("option requires an argument"));
}

#[test]
fn agent_subcommand_does_not_interfere_with_c() {
    // Sanity: the new `agent` subcommand branch is hit only for the literal
    // token "agent", not when "agent" appears inside a -c command string.
    // `ash -c "echo agent"` should echo the word "agent", not dispatch.
    let (code, stdout, _stderr) = run_ash(&["-c", "echo agent"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("agent") && !stdout.contains("subcommand"),
        "-c with 'agent' in the command string must not trigger agent dispatch, got: {}",
        stdout
    );
}
