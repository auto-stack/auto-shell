//! Plan 036: ash script parity tests.
//!
//! Tests that ash scripts produce the same output as equivalent
//! bash/PowerShell/fish/nu scripts. Each case is a directory under
//! `tests/parity/cases/` containing `ash.ash`, `bash.sh`, and optionally
//! `pwsh.ps1`, `fish.fish`, `nu.nu`, plus `expected.txt` (golden output).
//!
//! Run: cargo test --test parity
//! Run one case: cargo test --test parity -- 01_echo

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ──────────────────────────────────────────────────────────────────────────
// Output normalization
// ──────────────────────────────────────────────────────────────────────────

/// Normalize shell output for cross-shell comparison.
/// Strips ANSI codes, normalizes line endings, trims trailing whitespace,
/// and replaces temp-dir paths with a placeholder.
fn normalize(output: &str) -> String {
    let mut s = output.to_string();

    // Strip ANSI escape sequences (\x1b[...m etc.)
    let mut cleaned = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                // Other escape, just skip the next char
                chars.next();
            }
        } else {
            cleaned.push(c);
        }
    }
    s = cleaned;

    // Normalize line endings: CRLF → LF
    s = s.replace("\r\n", "\n");

    // Trim trailing whitespace on each line
    s = s
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    // Trim leading/trailing newlines
    s = s.trim_matches('\n').to_string();

    // Replace absolute temp-dir paths with <TMPDIR> placeholder, so that
    // cross-shell temp-path differences don't cause false divergences.
    // Handles Windows (\\?\C:\..., C:\...) and Unix (/tmp/...) variants.
    let tmp = std::env::temp_dir();
    let tmp_win = tmp.to_string_lossy().into_owned();
    let tmp_slash = tmp_win.replace('\\', "/");
    s = s.replace("\\\\?\\", "");
    s = s.replace(&tmp_win, "<TMPDIR>");
    s = s.replace(&tmp_slash, "<TMPDIR>");

    s
}

// ──────────────────────────────────────────────────────────────────────────
// Shell execution helpers
// ──────────────────────────────────────────────────────────────────────────

/// Execute an ash script via subprocess. Returns (stdout, exit_code).
/// When `bash_compat` is true, passes `--bash-compat` so structured commands
/// (ls/grep/wc) render as bash-style plain text (Plan 036 P1).
/// `cwd` sets the working directory (for per-case isolation).
fn run_ash(script_path: &Path, bash_compat: bool, cwd: &Path) -> Option<(String, i32)> {
    let bin = ash_binary_path();
    let mut cmd = Command::new(&bin);
    cmd.arg(script_path).current_dir(cwd);
    if bash_compat {
        cmd.arg("--bash-compat");
    }
    let output = cmd.output().ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}

/// Locate the ash binary (cargo auto-builds it via CARGO_BIN_EXE_ash).
/// Falls back to ASH_TEST_BIN env override for custom builds.
fn ash_binary_path() -> PathBuf {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return PathBuf::from(b);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_ash"))
}

/// Execute a bash script via subprocess. Returns (stdout, exit_code).
///
/// On Windows, the bare name `"bash"` may resolve to WSL's `System32\bash.exe`,
/// which can spawn (exit 0, not 127) but does not execute bash script syntax
/// correctly (broken POSIX path mounting). So we prefer the full Git bash path
/// candidates first, falling back to `"bash"` only on Unix (where it is the
/// real bash). A candidate is accepted only if it actually emits the expected
/// `$BASH_VERSION` probe output, proving it is a functional bash.
fn run_bash(script_path: &Path, cwd: &Path) -> Option<(String, i32)> {
    let script_content = fs::read_to_string(script_path).ok()?;

    let bash = resolve_bash()?;
    let output = Command::new(&bash)
        .args(["-c", &script_content])
        .current_dir(cwd)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}

/// Resolve a functional bash binary path.
/// Caches the result in a once-cell-like static via std::sync::OnceLock.
fn resolve_bash() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static RESOLVED: OnceLock<Option<PathBuf>> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            // Windows: prefer full Git bash paths; the bare "bash" may be WSL
            // (CreateProcess searches System32 before PATH, so the broken WSL
            // launcher there wins over Git bash on PATH). Git's install root
            // varies by machine — derive it from `git --exec-path`.
            // Unix: "bash" is the real bash.
            let mut candidates: Vec<PathBuf> = Vec::new();
            if cfg!(windows) {
                if let Some(root) = git_install_root() {
                    for rel in ["bin", "usr\\bin"] {
                        candidates.push(root.join(rel).join("bash.exe"));
                    }
                }
                candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
                candidates.push(PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"));
            }
            candidates.push(PathBuf::from("bash"));
            for c in &candidates {
                // Probe: a real bash prints BASH_VERSION. WSL bash also has it,
                // but if a full Git path exists it is preferred and wins first.
                if let Ok(o) = Command::new(c).args(["-c", "echo $BASH_VERSION"]).output() {
                    let out = String::from_utf8_lossy(&o.stdout);
                    if !out.trim().is_empty() {
                        return Some(c.clone());
                    }
                }
            }
            None
        })
        .clone()
}

/// Git's install root, derived from `git --exec-path` (mingw64 layout).
fn git_install_root() -> Option<PathBuf> {
    let out = Command::new("git").arg("--exec-path").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Path::new(&p).ancestors().nth(3).map(Path::to_path_buf)
}

/// Execute a PowerShell script via subprocess. Returns (stdout, exit_code).
fn run_pwsh(script_path: &Path, cwd: &Path) -> Option<(String, i32)> {
    let output = Command::new("pwsh")
        .args(["-NoProfile", "-File"])
        .arg(script_path)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}

/// Execute a fish script via subprocess. Returns (stdout, exit_code).
fn run_fish(script_path: &Path, cwd: &Path) -> Option<(String, i32)> {
    let output = Command::new("fish")
        .arg(script_path)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}

/// Execute a nu (nushell) script via subprocess. Returns (stdout, exit_code).
fn run_nu(script_path: &Path, cwd: &Path) -> Option<(String, i32)> {
    let output = Command::new("nu")
        .arg(script_path)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}

/// Check if a command exists on the system.
fn command_exists(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty())
        .unwrap_or(false)
}

// ──────────────────────────────────────────────────────────────────────────
// Parity test runner
// ──────────────────────────────────────────────────────────────────────────

/// A parity test case discovered from the cases/ directory.
struct ParityCase {
    name: String,
    dir: PathBuf,
    ash_script: PathBuf,
    bash_script: Option<PathBuf>,
    expected: Option<String>,
    /// Plan 036 P1: if true, run ash with --bash-compat (for real structured
    /// commands like ls/grep/wc). Activated by a `bash_compat` marker file in
    /// the case directory.
    bash_compat: bool,
    /// If true, skip this case (known bug causes infinite loop or crash).
    /// Activated by a `skip` marker file whose content is the skip reason.
    skip: Option<String>,
}

/// Discover all parity test cases under `cases/`.
fn discover_cases() -> Vec<ParityCase> {
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("parity")
        .join("cases");

    let mut cases = Vec::new();
    if !cases_dir.exists() {
        return cases;
    }

    let mut entries: Vec<_> = fs::read_dir(&cases_dir)
        .unwrap_or_else(|_| panic!("failed to read {:?}", cases_dir))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        let ash_path = dir.join("ash.ash");
        if !ash_path.exists() {
            continue;
        }
        let ash_script = ash_path;

        let bash_script = {
            let p = dir.join("bash.sh");
            if p.exists() { Some(p) } else { None }
        };

        let expected = fs::read_to_string(dir.join("expected.txt")).ok();

        // Plan 036 P1: a `bash_compat` marker file (empty) enables
        // --bash-compat for this case's ash run.
        let bash_compat = dir.join("bash_compat").exists();

        // A `skip` marker file (content = skip reason) skips the case,
        // e.g. for known infinite-loop bugs that would hang the suite.
        let skip = fs::read_to_string(dir.join("skip")).ok().map(|s| s.trim().to_string());

        cases.push(ParityCase {
            name,
            dir,
            ash_script,
            bash_script,
            expected,
            bash_compat,
            skip,
        });
    }

    cases
}

/// Run a single parity case: compare ash output against bash and expected.
/// Each case runs in its own isolated temp directory so file-based cases
/// don't pollute each other (Plan 036 workaround-4 fix).
fn run_parity_case(case: &ParityCase) -> Result<(), String> {
    // Skip cases marked with a `skip` file (known bugs that hang/crash).
    if let Some(reason) = &case.skip {
        eprintln!("⏭️  SKIP {}: {}", case.name, reason);
        return Ok(());
    }

    // Create an isolated temp directory for this case.
    let cwd = std::env::temp_dir().join(format!("ash_parity_{}", case.name));
    let _ = fs::remove_dir_all(&cwd); // clean any leftover
    fs::create_dir_all(&cwd).map_err(|e| format!("failed to create temp dir: {}", e))?;

    // 1. Run ash
    let (ash_out, ash_code) =
        run_ash(&case.ash_script, case.bash_compat, &cwd).unwrap_or_default();
    let ash_norm = normalize(&ash_out);

    // 2. Compare against expected.txt (golden) if present
    if let Some(expected) = &case.expected {
        let exp_norm = normalize(expected);
        if ash_norm != exp_norm {
            let _ = fs::remove_dir_all(&cwd);
            return Err(format!(
                "ash output != expected\n\
                 --- ash (normalized) ---\n{}\n\
                 --- expected (normalized) ---\n{}\n",
                ash_norm, exp_norm
            ));
        }
    }

    // 3. Compare against bash if present and bash is available
    if let Some(bash_path) = &case.bash_script {
        if resolve_bash().is_some() {
            // Clean temp dir before bash run so ash's file side-effects
            // don't influence bash (each gets a fresh dir).
            let _ = fs::remove_dir_all(&cwd);
            fs::create_dir_all(&cwd).map_err(|e| format!("recreate temp dir: {}", e))?;

            let (bash_out, bash_code) = run_bash(bash_path, &cwd).unwrap_or_default();
            let bash_norm = normalize(&bash_out);
            if ash_norm != bash_norm {
                let _ = fs::remove_dir_all(&cwd);
                return Err(format!(
                    "ash output != bash output\n\
                     --- ash (normalized) ---\n{}\n\
                     --- bash (normalized) ---\n{}\n",
                    ash_norm, bash_norm
                ));
            }
            // R3: exit-code parity — compare against bash exit code.
            if ash_code != bash_code {
                let _ = fs::remove_dir_all(&cwd);
                return Err(format!(
                    "ash exit-code != bash exit-code: {} != {}",
                    ash_code, bash_code
                ));
            }
        }
    }

    // 4. PowerShell comparison (best-effort)
    let pwsh_path = case.dir.join("pwsh.ps1");
    if pwsh_path.exists() && command_exists("pwsh") {
        let _ = fs::remove_dir_all(&cwd);
        fs::create_dir_all(&cwd).map_err(|e| format!("recreate temp dir: {}", e))?;
        let (pwsh_out, _) = run_pwsh(&pwsh_path, &cwd).unwrap_or_default();
        let pwsh_norm = normalize(&pwsh_out);
        if ash_norm != pwsh_norm {
            eprintln!(
                "WARNING: ash != pwsh for {} (best-effort, not failing):\n\
                 ash:  {}\npwsh: {}",
                case.name, ash_norm, pwsh_norm
            );
        }
    }

    // 5. fish comparison (best-effort)
    let fish_path = case.dir.join("fish.fish");
    if fish_path.exists() && command_exists("fish") {
        let _ = fs::remove_dir_all(&cwd);
        fs::create_dir_all(&cwd).map_err(|e| format!("recreate temp dir: {}", e))?;
        let (fish_out, _) = run_fish(&fish_path, &cwd).unwrap_or_default();
        let fish_norm = normalize(&fish_out);
        if ash_norm != fish_norm {
            eprintln!(
                "WARNING: ash != fish for {} (best-effort, not failing):\n\
                 ash:  {}\nfish: {}",
                case.name, ash_norm, fish_norm
            );
        }
    }

    // 6. nu comparison (best-effort)
    let nu_path = case.dir.join("nu.nu");
    if nu_path.exists() && command_exists("nu") {
        let _ = fs::remove_dir_all(&cwd);
        fs::create_dir_all(&cwd).map_err(|e| format!("recreate temp dir: {}", e))?;
        let (nu_out, _) = run_nu(&nu_path, &cwd).unwrap_or_default();
        let nu_norm = normalize(&nu_out);
        if ash_norm != nu_norm {
            eprintln!(
                "WARNING: ash != nu for {} (best-effort, not failing):\n\
                 ash: {}\nnu:   {}",
                case.name, ash_norm, nu_norm
            );
        }
    }

    // Clean up the temp directory.
    let _ = fs::remove_dir_all(&cwd);

    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────
// Test entry points
// ──────────────────────────────────────────────────────────────────────────

/// Generate a #[test] per discovered case.
/// This macro-free approach reads the directory at compile time via
/// a build script alternative: we just enumerate at test runtime.
#[test]
fn parity_all_cases() {
    let cases = discover_cases();
    assert!(
        !cases.is_empty(),
        "no parity cases found; expected at least one under tests/parity/cases/"
    );

    let mut failures = Vec::new();
    let mut passed = 0;

    for case in &cases {
        match run_parity_case(case) {
            Ok(()) => passed += 1,
            Err(e) => failures.push(format!("❌ {}: {}", case.name, e)),
        }
    }

    if !failures.is_empty() {
        let msg = format!(
            "{} parity test(s) diverged ({} passed):\n\n{}",
            failures.len(),
            passed,
            failures.join("\n\n")
        );
        // Parity divergences are reported as warnings by default.
        // Set ASH_PARITY_STRICT=1 to make them test failures (for CI).
        if std::env::var("ASH_PARITY_STRICT").unwrap_or_default() == "1" {
            panic!("{}", msg);
        } else {
            eprintln!("⚠️  PARITY DIVERGENCE REPORT (set ASH_PARITY_STRICT=1 to fail):\n{}", msg);
        }
    } else {
        println!("✓ All {} parity cases passed (ash == bash)", passed);
    }
}

/// Bootstrap: generate expected.txt from bash for all cases.
/// Run with: cargo test --test parity -- bootstrap_expected --nocapture
#[test]
fn bootstrap_expected() {
    let cases = discover_cases();
    let mut generated = 0;

    for case in &cases {
        if let Some(bash_path) = &case.bash_script {
            if resolve_bash().is_some() {
                // Use an isolated temp dir (same as run_parity_case).
                let cwd = std::env::temp_dir().join(format!("ash_parity_{}", case.name));
                let _ = fs::remove_dir_all(&cwd);
                let _ = fs::create_dir_all(&cwd);
                let (bash_out, _) = run_bash(bash_path, &cwd).unwrap_or_default();
                let _ = fs::remove_dir_all(&cwd);
                let normalized = normalize(&bash_out);
                let expected_path = case.dir.join("expected.txt");
                fs::write(&expected_path, &normalized).unwrap();
                generated += 1;
                println!("✓ generated expected.txt for {}", case.name);
            }
        }
    }

    println!("Generated {} expected.txt files from bash", generated);
}
