//! Plan 031 M0.3 — regression coverage for the format converter commands.
//!
//! Guards that `from_*`/`to_*` commands:
//!   1. round-trip structured data (parse then serialize is stable), and
//!   2. no longer silently drop data when fed an `ExternalStream` (the same
//!      class of bug M0.1 fixed for DSL stages — here via the format commands'
//!      `run_atom` which previously routed through the lossy
//!      `atom_to_pipeline_data` bridge).
//!
//! These run the full shell path so they exercise `run_atom`, not just the
//! free-function parsers covered by inline unit tests.

use auto_shell::Shell;

fn exec(shell: &mut Shell, input: &str) -> Option<String> {
    shell.execute(input).unwrap_or(None)
}

/// An external producer that emits a single JSON array line, used to feed an
/// `ExternalStream` into a `from_*` command. Kept to one line to avoid
/// entangling with NDJSON / trailing-newline parsing (a separate concern).
#[cfg(unix)]
fn external_json_array() -> String {
    // printf with no trailing newline → a clean `[1,2,3]` stream.
    "printf '[1,2,3]'".to_string()
}
#[cfg(windows)]
fn external_json_array() -> String {
    // `cmd /c` runs a real external process; `<nul set /p=` prints without a
    // trailing newline/CRLF.
    "cmd /c \"<nul set /p=[1,2,3]\"".to_string()
}

#[test]
fn from_json_parses_text_input() {
    let mut shell = Shell::new();
    // Use a JSON array (no inner double-quotes, which ash's echo arg parsing
    // would otherwise strip).
    let out = exec(&mut shell, "echo [1,2,3] | from_json").unwrap_or_default();
    assert!(out.contains('1') && out.contains('3'), "from_json lost data: {out:?}");
}

#[test]
fn from_json_external_stream_is_not_empty() {
    // External command → ExternalStream → from_json must not silently empty.
    let mut shell = Shell::new();
    let producer = external_json_array();
    let out = exec(&mut shell, &format!("{producer} | from_json")).unwrap_or_default();
    assert!(
        out.contains('1') && out.contains('3'),
        "from_json dropped external stream: {out:?}"
    );
}

#[test]
fn to_json_serializes_structured_input() {
    let mut shell = Shell::new();
    // ls produces a structured FileList; to_json must emit JSON (contains '{').
    let out = exec(&mut shell, "ls | to_json").unwrap_or_default();
    assert!(out.contains('{') || out.contains('['), "to_json produced no JSON: {out:?}");
}

#[test]
fn json_roundtrip_through_commands() {
    let mut shell = Shell::new();
    let out = exec(&mut shell, "echo [1,2,3] | from_json | to_json").unwrap_or_default();
    assert!(out.contains('1') && out.contains('3'), "json roundtrip lost data: {out:?}");
}

#[test]
fn from_toml_parses_text_input() {
    let mut shell = Shell::new();
    let out = exec(&mut shell, "echo 'name = \"alice\"' | from_toml").unwrap_or_default();
    assert!(out.contains("alice"), "from_toml lost name: {out:?}");
}

#[test]
fn from_yaml_parses_text_input() {
    let mut shell = Shell::new();
    let out = exec(&mut shell, "echo 'name: alice' | from_yaml").unwrap_or_default();
    assert!(out.contains("alice"), "from_yaml lost name: {out:?}");
}
