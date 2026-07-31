//! Plugin discovery & loading (Plan 033 §4).
//!
//! At startup the shell scans `~/.config/ash/plugins/*/`, reads each plugin's
//! `plugin.at` manifest, and loads enabled/version-compatible plugins. Each
//! enabled plugin contributes content of up to four kinds:
//!
//! | contribution | how it's loaded |
//! |---|---|
//! | completions | `completions/*.at` → completion provider (see [`enabled_plugin_completion_dirs`]) |
//! | functions   | `functions.ash` sourced via `Shell::source_file` |
//! | smart       | `smart/<cmd>/` → SmartCommand loader's extra search dirs |
//! | config      | `config.at` (v1: placeholder, not merged) |

use std::path::{Path, PathBuf};

use miette::Result;

use crate::auto_config::ash_dir;
use crate::shell::Shell;

use super::manifest::{parse_plugin_manifest, Capabilities, PluginManifest};

/// Report of what happened during a plugin load pass. Printed to stderr.
#[derive(Debug, Default, Clone)]
pub struct PluginLoadReport {
    /// Names of plugins successfully loaded.
    pub loaded: Vec<String>,
    /// `(name_or_dir, reason)` for plugins skipped due to error/version.
    pub skipped: Vec<(String, String)>,
    /// Names of plugins skipped because `enabled: false`.
    pub disabled: Vec<String>,
    /// `(name, capabilities)` for plugins that declared capabilities (warning).
    pub capability_warnings: Vec<(String, Capabilities)>,
}

impl PluginLoadReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when nothing was loaded, skipped, disabled, or warned about.
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
            && self.skipped.is_empty()
            && self.disabled.is_empty()
            && self.capability_warnings.is_empty()
    }

    /// Print a human-readable summary to stderr.
    pub fn print_to_stderr(&self) {
        if self.is_empty() {
            return;
        }
        for name in &self.loaded {
            eprintln!("plugin: loaded {}", name);
        }
        for name in &self.disabled {
            eprintln!("plugin: disabled {} (enable with `ash plugin enable {}`)", name, name);
        }
        for (name, reason) in &self.skipped {
            eprintln!("plugin: skipped {} ({})", name, reason);
        }
        for (name, caps) in &self.capability_warnings {
            let mut parts = Vec::new();
            if caps.reads_fs {
                parts.push("reads_fs");
            }
            if caps.writes_fs {
                parts.push("writes_fs");
            }
            if caps.spawns_process {
                parts.push("spawns_process");
            }
            if caps.uses_network {
                parts.push("uses_network");
            }
            eprintln!(
                "plugin: warning — '{}' declares capabilities: {}",
                name,
                parts.join(", ")
            );
        }
    }
}

/// The plugins directory: `~/.config/ash/plugins/` (or platform equivalent).
/// Creates it if absent. Returns `None` only if no config dir can be resolved.
pub fn plugins_dir() -> Option<PathBuf> {
    ash_dir().map(|d| d.join("plugins"))
}

/// Load every enabled, version-compatible plugin under the plugins dir.
/// Sources plugin `functions.ash` into `shell` and records SmartCommand dirs
/// for the lazy loader. Completion contributions are loaded separately by the
/// completion provider's tier scan (see [`enabled_plugin_completion_dirs`]).
///
/// Called from `Repl::new` after `.ashrc` is sourced.
pub fn load_all_plugins(shell: &mut Shell) -> Result<PluginLoadReport> {
    let mut report = PluginLoadReport::new();
    let plugins_dir = match plugins_dir() {
        Some(d) => d,
        None => return Ok(report), // no config dir — nothing to do
    };

    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return Ok(report), // missing dir is normal
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let plugin_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let manifest_path = path.join("plugin.at");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(_) => {
                report
                    .skipped
                    .push((plugin_name, "no plugin.at manifest".to_string()));
                continue;
            }
        };
        let manifest = match parse_plugin_manifest(&content) {
            Ok(m) => m,
            Err(e) => {
                report
                    .skipped
                    .push((plugin_name, format!("invalid manifest: {e}")));
                continue;
            }
        };

        if !manifest.enabled {
            report.disabled.push(plugin_name);
            continue;
        }

        if let Some(min) = &manifest.min_ash_version {
            if !ash_version_meets(min) {
                report.skipped.push((
                    plugin_name,
                    format!("requires ash >= {} (have {})", min, current_ash_version()),
                ));
                continue;
            }
        }

        if !manifest.capabilities.is_empty() {
            report
                .capability_warnings
                .push((plugin_name.clone(), manifest.capabilities.clone()));
        }

        // Load contributions that need `&mut Shell` (functions). SmartCommand
        // dirs are collected lazily by the smart loader; completions by the
        // completion provider's tier scan.
        if manifest.contributions.functions {
            let funcs = path.join("functions.ash");
            if funcs.exists() {
                if let Err(e) = shell.source_file(&funcs) {
                    report
                        .skipped
                        .push((plugin_name.clone(), format!("functions.ash failed: {e}")));
                    continue;
                }
            }
        }
        if manifest.contributions.config {
            // v1 placeholder: config.at merge is not implemented. Warn so authors
            // know the contribution is declared but not yet applied.
            let cfg = path.join("config.at");
            if cfg.exists() {
                eprintln!(
                    "plugin: note — '{}' declares a config contribution; config merge is not yet implemented (v1)",
                    plugin_name
                );
            }
        }

        report.loaded.push(plugin_name);
    }

    Ok(report)
}

/// Return the `completions/` directory of every enabled, version-compatible
/// plugin. Called by the completion provider's tier scan so plugin specs load
/// at the highest precedence (above the user tier).
///
/// This is a file-scan (no `&mut Shell` needed) so the completion layer — which
/// does not have a Shell handle — can call it directly.
pub fn enabled_plugin_completion_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let plugins_dir = match plugins_dir() {
        Some(d) => d,
        None => return dirs,
    };
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return dirs,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = match read_manifest(&path.join("plugin.at")) {
            Some(m) => m,
            None => continue,
        };
        if !manifest.enabled || !manifest.contributions.completions {
            continue;
        }
        if let Some(min) = &manifest.min_ash_version {
            if !ash_version_meets(min) {
                continue;
            }
        }
        let completions = path.join("completions");
        if completions.is_dir() {
            dirs.push(completions);
        }
    }
    dirs
}

/// Return the `smart/` directory of every enabled, version-compatible plugin
/// that declares a smart contribution. Called by the SmartCommand lazy loader
/// (`smart_command::loader`) so `ash smart` picks up plugin commands.
pub fn enabled_plugin_smart_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let plugins_dir = match plugins_dir() {
        Some(d) => d,
        None => return dirs,
    };
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return dirs,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = match read_manifest(&path.join("plugin.at")) {
            Some(m) => m,
            None => continue,
        };
        if !manifest.enabled || !manifest.contributions.smart {
            continue;
        }
        if let Some(min) = &manifest.min_ash_version {
            if !ash_version_meets(min) {
                continue;
            }
        }
        let smart = path.join("smart");
        if smart.is_dir() {
            dirs.push(smart);
        }
    }
    dirs
}

fn read_manifest(path: &Path) -> Option<PluginManifest> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_plugin_manifest(&content).ok()
}

/// The running ash version (compile-time crate version).
fn current_ash_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// True when `current_ash_version()` satisfies the required `min` version.
/// Compares major.minor numerically (patch ignored); a malformed `min` is
/// treated as "any version satisfies" so a bad manifest never blocks startup.
fn ash_version_meets(min: &str) -> bool {
    let cur = match parse_semver(current_ash_version()) {
        Some(c) => c,
        None => return true, // can't parse our own version — be permissive
    };
    let req = match parse_semver(min) {
        Some(r) => r,
        None => return true, // malformed min — don't block loading
    };
    cur >= req
}

/// Parse a `"MAJOR.MINOR[.PATCH]"` string into `(major, minor)` (patch ignored).
fn parse_semver(s: &str) -> Option<(u64, u64)> {
    let mut parts = s.split('.');
    let major = parts.next()?.trim().parse::<u64>().ok()?;
    let minor = parts.next()?.trim().parse::<u64>().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a `plugin.at` into `dir` with the given fields.
    fn write_manifest(dir: &Path, name: &str, version: &str, extra: &str) {
        let content = format!(
            r#"plugin {{
    name    : "{name}"
    version : "{version}"
{extra}}}
"#
        );
        std::fs::write(dir.join("plugin.at"), content).unwrap();
    }

    /// A throwaway plugins dir under temp, with a unique suffix.
    fn temp_plugins_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ash_plugin_tests")
            .join(label);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn report_is_empty_default() {
        assert!(PluginLoadReport::new().is_empty());
    }

    #[test]
    fn report_not_empty_after_loaded() {
        let mut r = PluginLoadReport::new();
        r.loaded.push("x".into());
        assert!(!r.is_empty());
    }

    #[test]
    fn semver_parse_basic() {
        assert_eq!(parse_semver("0.1.0"), Some((0, 1)));
        assert_eq!(parse_semver("1.5"), Some((1, 5)));
        assert_eq!(parse_semver("not-a-version"), None);
    }

    #[test]
    fn version_meets_equal() {
        assert!(ash_version_meets(current_ash_version()));
    }

    #[test]
    fn version_meets_lower_requirement() {
        assert!(ash_version_meets("0.0.1"));
    }

    #[test]
    fn version_does_not_meet_higher_requirement() {
        // Require a much higher minor than the real crate version (0.1.0).
        assert!(!ash_version_meets("99.0.0"));
    }

    #[test]
    fn version_meets_malformed_min_is_permissive() {
        assert!(ash_version_meets("garbage"));
    }

    #[test]
    fn completion_dirs_includes_enabled_plugin() {
        let dir = temp_plugins_dir("completion_enabled");
        let p = dir.join("demo");
        std::fs::create_dir_all(p.join("completions")).unwrap();
        write_manifest(&p, "demo", "0.1.0", "    contributions : { completions : true }\n");

        // Override the plugins dir for the test by calling the scan against the
        // temp dir directly via a helper that mirrors enabled_plugin_completion_dirs.
        let dirs = scan_completion_dirs(&dir);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("completions"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completion_dirs_skips_disabled_plugin() {
        let dir = temp_plugins_dir("completion_disabled");
        let p = dir.join("demo");
        std::fs::create_dir_all(p.join("completions")).unwrap();
        write_manifest(
            &p,
            "demo",
            "0.1.0",
            "    enabled : false\n    contributions : { completions : true }\n",
        );

        let dirs = scan_completion_dirs(&dir);
        assert!(dirs.is_empty(), "disabled plugin must not contribute");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completion_dirs_skips_version_incompatible() {
        let dir = temp_plugins_dir("completion_version");
        let p = dir.join("demo");
        std::fs::create_dir_all(p.join("completions")).unwrap();
        write_manifest(
            &p,
            "demo",
            "0.1.0",
            "    min_ash_version : \"99.0.0\"\n    contributions : { completions : true }\n",
        );

        let dirs = scan_completion_dirs(&dir);
        assert!(dirs.is_empty(), "version-incompatible plugin skipped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn smart_dirs_includes_enabled_plugin() {
        let dir = temp_plugins_dir("smart_enabled");
        let p = dir.join("demo");
        std::fs::create_dir_all(p.join("smart")).unwrap();
        write_manifest(&p, "demo", "0.1.0", "    contributions : { smart : true }\n");

        let dirs = scan_smart_dirs(&dir);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("smart"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Scan a given plugins dir for enabled completion dirs (test-only variant
    /// so tests don't depend on the real `~/.config/ash`).
    fn scan_completion_dirs(plugins_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let entries = match std::fs::read_dir(plugins_dir) {
            Ok(e) => e,
            Err(_) => return dirs,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest = match read_manifest(&path.join("plugin.at")) {
                Some(m) => m,
                None => continue,
            };
            if !manifest.enabled || !manifest.contributions.completions {
                continue;
            }
            if let Some(min) = &manifest.min_ash_version {
                if !ash_version_meets(min) {
                    continue;
                }
            }
            let completions = path.join("completions");
            if completions.is_dir() {
                dirs.push(completions);
            }
        }
        dirs
    }

    /// Scan a given plugins dir for enabled smart dirs (test-only variant).
    fn scan_smart_dirs(plugins_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let entries = match std::fs::read_dir(plugins_dir) {
            Ok(e) => e,
            Err(_) => return dirs,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest = match read_manifest(&path.join("plugin.at")) {
                Some(m) => m,
                None => continue,
            };
            if !manifest.enabled || !manifest.contributions.smart {
                continue;
            }
            if let Some(min) = &manifest.min_ash_version {
                if !ash_version_meets(min) {
                    continue;
                }
            }
            let smart = path.join("smart");
            if smart.is_dir() {
                dirs.push(smart);
            }
        }
        dirs
    }
}
