//! SmartCommand discovery — load specs from search paths (Plan 029 §A.4).
//!
//! Search order (first hit wins; same name in a later dir is ignored):
//! 1. `$CWD/smart/` — project-local commands
//! 2. `~/.config/ash/smart/` — user-global commands
//!
//! Each directory is scanned for `*.at` files, each parsed into a
//! [`SmartCommandSpec`] with its `source_path` set (so the executor can
//! resolve the body script relative to it).

use std::path::{Path, PathBuf};

use super::config::{parse_at, SmartCommandSpec};

/// Load every SmartCommand spec from the search paths, with project-local
/// taking precedence over user-global on name collisions.
///
/// `cwd` is the current working directory (search root for project-local);
/// `home` is the user home (search root for user-global). Both are passed in
/// so tests can control them without depending on env lookups.
pub fn load_all_from(cwd: &Path, home: &Path) -> Vec<SmartCommandSpec> {
    let dirs = [
        cwd.join("smart"),
        home.join(".config").join("ash").join("smart"),
    ];
    let mut specs: Vec<SmartCommandSpec> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for dir in dirs {
        for spec in load_dir(&dir) {
            if seen.insert(spec.name.clone()) {
                specs.push(spec);
            }
        }
    }
    specs
}

/// Convenience: load using the real cwd + home dir.
pub fn load_all() -> Vec<SmartCommandSpec> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    load_all_from(&cwd, &home)
}

/// Parse every `*.at` file in `dir` (non-recursive). Missing dir → empty.
/// Malformed files are skipped with a warning to stderr (one bad file doesn't
/// break the rest).
fn load_dir(dir: &Path) -> Vec<SmartCommandSpec> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(), // missing dir is normal
    };
    let mut specs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("at") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: cannot read {}: {}", path.display(), e);
                continue;
            }
        };
        match parse_at(&content) {
            Ok(mut spec) => {
                spec.source_path = Some(path.clone());
                specs.push(spec);
            }
            Err(e) => {
                eprintln!("warning: skipping malformed {}: {}", path.display(), e);
            }
        }
    }
    // Stable order by name for deterministic `ash smart list` output.
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_at(dir: &Path, name: &str, cmd: &str, desc: &str, body: &str) {
        let content = format!(
            r#"command "{cmd}" {{
    description : "{desc}"
    body        : "{body}"
}}
"#
        );
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn load_from_missing_dirs_is_empty() {
        let tmp = std::env::temp_dir().join("ash_smart_missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let specs = load_all_from(&tmp, &tmp);
        assert!(specs.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_project_local() {
        let tmp = std::env::temp_dir().join("ash_smart_local");
        let _ = fs::remove_dir_all(&tmp);
        let smart_dir = tmp.join("smart");
        fs::create_dir_all(&smart_dir).unwrap();
        write_at(&smart_dir, "deploy.at", "deploy", "deploy app", "deploy.ash");

        let specs = load_all_from(&tmp, &tmp);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "deploy");
        assert_eq!(specs[0].source_path.as_ref().unwrap(), &smart_dir.join("deploy.at"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn project_local_overrides_user_global() {
        let proj = std::env::temp_dir().join("ash_smart_proj");
        let home = std::env::temp_dir().join("ash_smart_home");
        for d in [&proj, &home] {
            let _ = fs::remove_dir_all(d);
        }
        // Project version.
        let proj_smart = proj.join("smart");
        fs::create_dir_all(&proj_smart).unwrap();
        write_at(&proj_smart, "x.at", "shared", "project version", "proj.ash");
        // User-global version (should be shadowed).
        let user_smart = home.join(".config").join("ash").join("smart");
        fs::create_dir_all(&user_smart).unwrap();
        write_at(&user_smart, "x.at", "shared", "user version", "user.ash");
        // Plus a user-only command.
        write_at(&user_smart, "extra.at", "extra", "user only", "extra.ash");

        let specs = load_all_from(&proj, &home);
        assert_eq!(specs.len(), 2, "shared + extra");
        let shared = specs.iter().find(|s| s.name == "shared").unwrap();
        assert_eq!(
            shared.description, "project version",
            "project-local must win on collision"
        );
        assert!(specs.iter().any(|s| s.name == "extra"));
        for d in [&proj, &home] {
            let _ = fs::remove_dir_all(d);
        }
    }

    #[test]
    fn sorted_by_name() {
        let tmp = std::env::temp_dir().join("ash_smart_sort");
        let _ = fs::remove_dir_all(&tmp);
        let smart_dir = tmp.join("smart");
        fs::create_dir_all(&smart_dir).unwrap();
        write_at(&smart_dir, "z.at", "zebra", "z", "z.ash");
        write_at(&smart_dir, "a.at", "alpha", "a", "a.ash");
        write_at(&smart_dir, "m.at", "mid", "m", "m.ash");

        let specs = load_all_from(&tmp, &tmp);
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zebra"]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn malformed_file_skipped_others_loaded() {
        let tmp = std::env::temp_dir().join("ash_smart_malformed");
        let _ = fs::remove_dir_all(&tmp);
        let smart_dir = tmp.join("smart");
        fs::create_dir_all(&smart_dir).unwrap();
        fs::write(smart_dir.join("bad.at"), "not a command").unwrap();
        write_at(&smart_dir, "good.at", "good", "ok", "good.ash");

        let specs = load_all_from(&tmp, &tmp);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "good");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ignores_non_at_files() {
        let tmp = std::env::temp_dir().join("ash_smart_nonat");
        let _ = fs::remove_dir_all(&tmp);
        let smart_dir = tmp.join("smart");
        fs::create_dir_all(&smart_dir).unwrap();
        fs::write(smart_dir.join("readme.md"), "# commands").unwrap();
        fs::write(smart_dir.join("body.ash"), "> echo hi").unwrap();
        write_at(&smart_dir, "real.at", "real", "r", "real.ash");

        let specs = load_all_from(&tmp, &tmp);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "real");
        let _ = fs::remove_dir_all(&tmp);
    }
}
