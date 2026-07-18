//! Plan 028 M2.5: End-to-end tests for `ash agent ...` CLI.
//!
//! These spawn the `ash` binary as a subprocess (pre-built via `cargo build`)
//! and assert on its stdout JSON. We locate the binary via the `ASH_TEST_BIN`
//! env var if set, otherwise fall back to the workspace target dir.

use std::process::Command;

/// Locate the built `ash` binary. Tries (in order):
///   1. `$ASH_TEST_BIN` env var (set by CI / local developer)
///   2. `target/debug/ash` (or `ash.exe` on Windows) relative to the workspace
fn ash_bin() -> String {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return b;
    }
    // CARGO_MANIFEST_DIR is the auto-shell crate root (ash/auto-shell).
    // The workspace target dir is ash/target/ (workspace root is ash/).
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dir = std::path::Path::new(manifest)
        .parent() // ash/
        .unwrap()
        .join("target")
        .join("debug");
    // On Windows the binary is ash.exe; on Unix it's ash.
    let name = if cfg!(windows) { "ash.exe" } else { "ash" };
    dir.join(name).to_string_lossy().into_owned()
}

/// Run `ash agent <args>` and return (exit_code, stdout, stderr).
fn run_agent(args: &[&str]) -> (i32, String, String) {
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
fn describe_tools_returns_valid_json_envelope() {
    let (code, stdout, _stderr) = run_agent(&["agent", "describe-tools", "--format", "compact"]);
    assert_eq!(code, 0, "agent describe-tools failed");
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout not valid JSON: {}\n--- stdout ---\n{}", e, stdout));
    assert_eq!(v["schema_version"], "1");
    assert!(
        v["tool_count"].as_u64().unwrap() >= 70,
        "too few tools: {}",
        v["tool_count"]
    );
    assert!(v["tools"].is_array());
}

#[test]
fn describe_tools_filter_returns_subset() {
    let (_code, stdout, _stderr) =
        run_agent(&["agent", "describe-tools", "--filter", "ls", "--format", "compact"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    let names: Vec<&str> = v["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().any(|n| *n == "ls"), "filter should include ls");
    assert!(
        names.iter().all(|n| n.starts_with("ls")),
        "filter should only keep ls* tools, got: {:?}",
        names
    );
}

#[test]
fn describe_policy_returns_capability_summary() {
    let (code, stdout, _stderr) = run_agent(&["agent", "describe-policy"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert!(v["policy"].is_object());
    // The summary must be capability-only: no sandbox path leaks.
    let json_str = stdout.clone();
    assert!(
        !json_str.contains("/sandbox") || !v["policy"]["sandboxed"].as_bool().unwrap_or(false),
        "summary leaked a sandbox path"
    );
}

#[test]
fn run_echo_returns_success_envelope() {
    let (code, stdout, stderr) = run_agent(&["agent", "run", "echo hello_e2e"]);
    assert_eq!(code, 0, "agent run echo failed; stderr: {}", stderr);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not JSON");
    assert_eq!(v["schema_version"], "1");
    assert_eq!(v["status"], "success");
    assert_eq!(v["data"]["kind"], "text");
    assert!(
        v["data"]["value"].as_str().unwrap().contains("hello_e2e"),
        "expected echo output in value, got: {}",
        v["data"]["value"]
    );
    assert_eq!(v["command_echo"], "echo hello_e2e");
}

#[test]
fn run_nonexistent_command_returns_failed_envelope() {
    let (code, stdout, _stderr) = run_agent(&["agent", "run", "this_cmd_does_not_exist_xyz"]);
    // Non-zero exit on execution failure.
    assert_ne!(code, 0, "expected non-zero exit for failing command");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not JSON");
    assert_eq!(v["status"], "failed");
    assert!(v["error"]["kind"].is_string(), "error.kind missing");
}

#[test]
fn check_dangerous_command_is_denied() {
    let (code, stdout, _stderr) = run_agent(&["agent", "check", "rm -rf /"]);
    assert_eq!(code, 0, "check itself should succeed (it reports the decision)");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert_eq!(v["allowed"], false);
    assert_eq!(v["decision"], "deny");
    assert!(v["denied_reasons"].is_array());
}

#[test]
fn check_safe_command_is_allowed() {
    let (code, stdout, _stderr) = run_agent(&["agent", "check", "ls"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert_eq!(v["allowed"], true);
    assert_eq!(v["decision"], "allow");
}

#[test]
fn run_format_text_returns_plain_text() {
    let (code, stdout, _stderr) = run_agent(&["agent", "run", "echo plain_text_mode", "--format", "text"]);
    assert_eq!(code, 0);
    // In text mode, output is NOT JSON-wrapped.
    assert!(
        !stdout.contains("\"schema_version\""),
        "text mode should not emit JSON envelope, got: {}",
        stdout
    );
    assert!(stdout.contains("plain_text_mode"));
}

#[test]
fn no_subcommand_prints_usage_and_exits_nonzero() {
    let (code, _stdout, stderr) = run_agent(&["agent"]);
    assert_ne!(code, 0, "no-subcommand should exit nonzero");
    assert!(
        stderr.contains("usage:") || stderr.contains("subcommand"),
        "expected usage text on stderr, got: {}",
        stderr
    );
}

#[test]
fn unknown_subcommand_exits_nonzero() {
    let (code, _stdout, stderr) = run_agent(&["agent", "bogus_subcommand"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("unknown subcommand"),
        "expected unknown-subcommand error, got: {}",
        stderr
    );
}
