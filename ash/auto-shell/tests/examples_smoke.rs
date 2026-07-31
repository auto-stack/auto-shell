//! Plan 034 M1: smoke tests for the `examples/` script library.
//!
//! Every script under `examples/<name>/<name>.ash` is run (no args) via the
//! ash subprocess to guarantee it at least **parses and doesn't panic**. This
//! is the regression net that was missing — previously the 30+ example
//! scripts had zero test coverage, so a VM change could silently break them.
//!
//! ## Pass criteria
//! A script passes smoke when it does NOT hard-crash. We deliberately do NOT
//! require exit code 0: many scripts are environment-dependent (deploy-ai
//! needs a buildable project, git-batch needs git repos, csvsum needs a CSV
//! file) and correctly call `exit(1)` with a clear message when their
//! preconditions aren't met. That's graceful degradation, not a crash.
//!
//! What fails smoke (the only things we catch):
//! - ash parse errors (`unexpected token`) — the script can't even run
//! - panics / stack overflows — a real VM or script bug
//!
//! Run: cargo test --test examples_smoke
//! Run one: cargo test --test examples_smoke -- bigfiles

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Max wall-clock time per script. Some example scripts prompt for input
/// (cleanup's "确认删除?") or watch a process (watch-proc); with stdin
/// closed they should exit, but if one loops we must not hang the whole
/// suite. 15s is generous — healthy scripts finish in <2s.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(15);

/// One example script to smoke-test.
struct Example {
    name: String,
    script: PathBuf,
}

/// Discover all `examples/*/*.ash` scripts.
/// `examples/` lives at the repo root, i.e. `<crate>/../../examples`.
fn discover_examples() -> Vec<Example> {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&examples_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(inner) = fs::read_dir(&path) {
                    for f in inner.flatten() {
                        let fp = f.path();
                        if fp.extension().map_or(false, |e| e == "ash") {
                            let name = fp
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("?")
                                .to_string();
                            out.push(Example {
                                name,
                                script: fp,
                            });
                        }
                    }
                }
            }
        }
    }
    // Stable order for readable test output.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Locate the ash binary (cargo auto-builds it via CARGO_BIN_EXE_ash).
/// Falls back to ASH_TEST_BIN env override for custom builds.
fn ash_binary_path() -> PathBuf {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return PathBuf::from(b);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_ash"))
}

/// Run an ash script with no args in an isolated empty temp dir, killing it
/// if it runs longer than [`SCRIPT_TIMEOUT`]. Returns (combined stdout+stderr,
/// exit_code).
///
/// We spawn the subprocess on a worker thread and wait on a channel with a
/// timeout; if the worker doesn't finish in time, the test reports a timeout
/// (exit code -2) instead of hanging the whole suite. This matters because
/// some example scripts prompt for input (cleanup) or loop (watch-proc) and
/// would otherwise block forever with stdin closed.
///
/// **Isolated cwd**: scripts run in a fresh empty temp dir, NOT the cargo
/// test cwd. This is critical — otherwise `buildtest` would run a real
/// `cargo build` inside the ash workspace (slow, >15s timeout, and pollutes
/// the build), and `filestats`/`loccount` would scan source files. In an
/// empty dir, env-dependent scripts fail fast and predictably.
fn run_ash(script: &Path) -> (String, i32) {
    let script = script.to_path_buf();
    // Per-run isolated cwd (empty dir), unique so parallel runs don't collide.
    let cwd = std::env::temp_dir().join(format!(
        "ash-smoke-{}-{}",
        std::process::id(),
        script.file_stem().and_then(|s| s.to_str()).unwrap_or("?"),
    ));
    let _ = fs::create_dir_all(&cwd);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = Command::new(ash_binary_path())
            .arg(&script)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .output();
        let _ = tx.send(result);
        // Best-effort cleanup of the temp dir.
        let _ = fs::remove_dir_all(&cwd);
    });

    match rx.recv_timeout(SCRIPT_TIMEOUT) {
        Ok(output) => match output {
            Ok(o) => {
                let mut combined = String::from_utf8_lossy(&o.stdout).into_owned();
                combined.push_str(&String::from_utf8_lossy(&o.stderr));
                (combined, o.status.code().unwrap_or(-1))
            }
            Err(e) => (format!("failed to spawn ash: {}", e), -1),
        },
        Err(_) => (
            format!("<timed out after {}s>", SCRIPT_TIMEOUT.as_secs()),
            -2,
        ),
    }
}

/// Is this an ash-level parse error (vs. a child-process error from `system()`)?
/// ash's own parser emits `unexpected token`; PowerShell/other child errors
/// carry `CategoryInfo`. We only treat the former as a smoke failure.
fn is_ash_parse_error(output: &str) -> bool {
    output.contains("unexpected token") && !output.contains("CategoryInfo")
}

/// Does the output look like a hard crash (VM panic / stack overflow)?
/// These are real bugs — distinct from a script's deliberate `exit(1)`.
fn is_vm_crash(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("panic")
        || lower.contains("stack overflow")
        || lower.contains("thread '")
        || lower.contains("fatal runtime error")
}

#[test]
fn examples_smoke_no_crash() {
    let examples = discover_examples();
    assert!(
        !examples.is_empty(),
        "no example scripts found — discover_examples is looking in the wrong place"
    );

    let mut failures: Vec<String> = Vec::new();
    let mut tested = 0;
    for ex in &examples {
        tested += 1;
        let (output, code) = run_ash(&ex.script);
        // Pass = no ash parse error AND no VM crash AND no timeout. We
        // intentionally allow non-zero exit codes: environment-dependent
        // scripts (deploy-ai, git-batch, csvsum, ...) legitimately `exit(1)`
        // when their preconditions aren't met — that's graceful, not a crash.
        let timed_out = code == -2;
        let crashed = is_ash_parse_error(&output) || is_vm_crash(&output);
        if crashed || timed_out {
            failures.push(format!(
                "{} (exit {}): {}",
                ex.name,
                code,
                output.lines().take(3).collect::<Vec<_>>().join(" | ")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} example scripts failed smoke test (parse error / crash / timeout):\n{}",
        failures.len(),
        tested,
        failures.join("\n")
    );
}
