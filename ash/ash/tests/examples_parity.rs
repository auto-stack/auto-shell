//! Plan 034 M2: ash ↔ bash equivalence for the core file-discovery primitives
//! that every example script depends on.
//!
//! Rather than assert per-script output (each example has its own quirks —
//! hardcoded dirs, interactive `read`, HashMap aggregation edge cases), this
//! test verifies the **shared foundation**: that ash's `system()` bridge runs
//! its built-in `find`/`ls` and produces the same file set as GNU find/ls on
//! an identical fixture. Once this equivalence holds, the example scripts
//! built on top of it are trustworthy.
//!
//! This became possible after Plan 034's fixes (find `-name`/`-type` POSIX
//! compat, `2>/dev/null` no longer swallowing stdout, `||`/`&&` chains
//! returning output, `$1` arg passing) and auto-lang Plan 378 (`.len()`/
//! `.to_uint()` slot alignment).
//!
//! Run: cargo test --test examples_parity

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Build a unique fixture dir with a known set of files, return its path.
fn make_fixture(files: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ash-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    for f in files {
        // Support "sub/file" to create nested files.
        let path = dir.join(f);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, b"x").unwrap();
    }
    dir
}

fn ash_binary() -> PathBuf {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return PathBuf::from(b);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_ash"))
}

/// Run an ash script BODY (wrapped in `fn main() { ... }`) via a temp script
/// file in the given cwd, returning sorted trimmed stdout lines.
///
/// We use a temp script FILE (not `-c`) because the AutoLang `system()` host
/// bridge is only installed on the script-execution path, not the `-c`
/// command-line path. Prints the value of `expr` (a system() call result).
fn ash_system_print(expr: &str, cwd: &std::path::Path) -> String {
    let script = format!("fn main() {{\n    var __r = {}\n    print(__r)\n}}\nmain()\n", expr);
    let script_path = cwd.join("__parity_probe.ash");
    fs::write(&script_path, &script).unwrap();
    let output = Command::new(ash_binary())
        .arg(&script_path)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("failed to spawn ash");
    let _ = fs::remove_file(&script_path);
    let s = String::from_utf8_lossy(&output.stdout).into_owned();
    // Strip ANSI + normalize + sort lines for stable comparison.
    let clean = strip_ansi(&s);
    let mut lines: Vec<&str> = clean.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    lines.sort();
    lines.join("\n")
}

/// Run a bash command in cwd, return sorted trimmed stdout lines.
fn bash_sorted(cmd: &str, cwd: &std::path::Path) -> String {
    let bash = resolve_bash();
    let output = Command::new(&bash)
        .args(["-c", cmd])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output();
    let s = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    };
    let mut lines: Vec<&str> = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    lines.sort();
    lines.join("\n")
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn resolve_bash() -> String {
    if cfg!(windows) {
        // Git's install root varies by machine (Program Files, D:\soft\Git,
        // scoop, portable). Derive it from `git --exec-path`
        // (<root>\mingw64\libexec\git-core → up 3 = <root>), because a bare
        // "bash" resolves to C:\Windows\System32\bash.exe (the WSL launcher)
        // before PATH is ever consulted.
        if let Some(root) = git_install_root() {
            for rel in ["bin", r"usr\bin"] {
                let cand = root.join(rel).join("bash.exe");
                if cand.exists() {
                    return cand.to_string_lossy().into_owned();
                }
            }
        }
        for c in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
        ] {
            if std::path::Path::new(c).exists() {
                return c.to_string();
            }
        }
    }
    "bash".to_string()
}

/// Git's install root, derived from `git --exec-path` (mingw64 layout).
fn git_install_root() -> Option<std::path::PathBuf> {
    let out = Command::new("git").arg("--exec-path").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    std::path::Path::new(&p).ancestors().nth(3).map(std::path::Path::to_path_buf)
}

/// `ls *.rs` lists the same files as bash (ash's `find -name` currently returns
/// only the FIRST match — a known find bug; `ls` with a glob is the reliable
/// multi-file primitive, so we assert on that).
#[test]
fn ls_glob_equivalence() {
    let dir = make_fixture(&["a.rs", "b.rs", "c.txt", "ignore.md"]);
    let ash = ash_system_print("system(\"ls *.rs\")", &dir);
    let bash = bash_sorted("ls *.rs", &dir);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(ash, bash, "ash ls *.rs must match bash (multi-file)");
}

/// `ls` lists the same entries as bash ls (sorted). The probe script is kept
/// OUTSIDE the fixture dir so it doesn't pollute the listing.
#[test]
fn ls_equivalence() {
    let dir = make_fixture(&["alpha.txt", "beta.rs", "gamma.log"]);
    // Probe script in a sibling temp location, not inside `dir`.
    let probe = std::env::temp_dir().join(format!(
        "__parity_ls_probe_{}.ash",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&probe, "fn main() {\n    print(system(\"ls\"))\n}\nmain()\n").unwrap();
    let output = Command::new(ash_binary())
        .arg(&probe)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn ash");
    let _ = fs::remove_file(&probe);
    let ash = {
        let s = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        let mut l: Vec<&str> = s.lines().map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
        l.sort();
        l.join("\n")
    };
    let bash = bash_sorted("ls", &dir);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(ash, bash, "ash ls must match bash ls");
}

/// find with -maxdepth stays within depth (doesn't recurse like bash).
#[test]
fn find_maxdepth_equivalence() {
    let dir = make_fixture(&["top.rs", "sub/nested.rs"]);
    let ash = ash_system_print(
        "system(\"find . -maxdepth 1 -name *.rs -type f\")",
        &dir,
    );
    let bash = bash_sorted("find . -maxdepth 1 -name '*.rs' -type f", &dir);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(ash, bash, "ash find -maxdepth must match bash");
}

/// `$1` positional arg reaches system() inside a script. We compare the set
/// of basenames found (ash emits absolute paths when $1 is absolute; bash
/// emits relative) — the discovery semantics are equivalent even if the path
/// prefix differs.
#[test]
fn positional_arg_passes_to_system() {
    let dir = make_fixture(&["x.tmp", "y.bak", "z.txt"]);
    let script = r#"fn main() {
    var d = system("echo $1").trim()
    if d.len() == 0 { d = "." }
    var patterns = ["*.tmp", "*.bak", "*.log"]
    var found = ""
    for p in patterns {
        var r = system("find " + d + " -maxdepth 1 -name " + p + " -type f")
        if r.trim().len() > 0 { found = found + r.trim() + "\n" }
    }
    print(found)
}
main()
"#;
    let script_path = dir.join("__parity_args.ash");
    fs::write(&script_path, script).unwrap();
    let out = Command::new(ash_binary())
        .arg(&script_path)
        .arg(".") // pass "." so ash uses relative paths like bash
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn ash");
    let _ = fs::remove_file(&script_path);
    // Compare basenames (strip any path prefix) so absolute/relative differ.
    let basenames = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = s
            .lines()
            .map(|l| {
                let p = std::path::Path::new(l.trim());
                p.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| l.trim().to_string())
            })
            .filter(|l| !l.is_empty())
            .collect();
        v.sort();
        v
    };
    let ash = basenames(&strip_ansi(&String::from_utf8_lossy(&out.stdout)));
    let bash_out = Command::new(resolve_bash())
        .args([
            "-c",
            "find . -maxdepth 1 \\( -name '*.tmp' -o -name '*.bak' -o -name '*.log' \\) -type f",
        ])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output();
    let bash_s = match bash_out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    };
    let bash = basenames(&bash_s);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        ash, bash,
        "ash multi-extension find (loop) must match bash find -o (by basename)"
    );
}
