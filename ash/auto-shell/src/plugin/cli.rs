//! `ash plugin` subcommand handler (Plan 033 §5).
//!
//! Invoked from `main.rs` when the first CLI arg is `plugin`. Supports:
//! - `ash plugin list` — list installed plugins
//! - `ash plugin show <name>` — show a plugin's manifest
//! - `ash plugin install <git-url|--local path> [--name <n>]`
//! - `ash plugin enable|disable <name>` — toggle `enabled`
//! - `ash plugin remove <name>` — delete a plugin directory
//! - `ash plugin update [<name>|--all]` — `git pull`
//!
//! The remaining argv after `plugin` is passed here as `args`.

use std::path::{Path, PathBuf};

use miette::Result;

use crate::auto_config::ash_dir;

use super::manifest::{parse_plugin_manifest, PluginManifest};

/// Dispatch the `ash plugin` subcommand. `args` is everything after `plugin`
/// (e.g. `["list"]` or `["show", "demo"]`).
pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_usage();
        return Ok(());
    }
    match args[0].as_str() {
        "list" => cmd_list(&args[1..]),
        "show" => {
            let rest = &args[1..];
            if rest.is_empty() {
                eprintln!("ash plugin show: missing plugin name");
                eprintln!("  usage: ash plugin show <name>");
                std::process::exit(2);
            }
            cmd_show(&rest[0])
        }
        "install" => {
            let rest = &args[1..];
            cmd_install(rest)
        }
        "enable" => {
            let rest = &args[1..];
            if rest.is_empty() {
                eprintln!("ash plugin enable: missing plugin name");
                std::process::exit(2);
            }
            cmd_set_enabled(&rest[0], true)
        }
        "disable" => {
            let rest = &args[1..];
            if rest.is_empty() {
                eprintln!("ash plugin disable: missing plugin name");
                std::process::exit(2);
            }
            cmd_set_enabled(&rest[0], false)
        }
        "remove" => {
            let rest = &args[1..];
            if rest.is_empty() {
                eprintln!("ash plugin remove: missing plugin name");
                std::process::exit(2);
            }
            cmd_remove(&rest[0])
        }
        "update" => cmd_update(&args[1..]),
        "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => {
            eprintln!("ash plugin: unknown subcommand '{}'", other);
            print_usage();
            std::process::exit(2);
        }
    }
}

/// The plugins directory: `~/.config/ash/plugins/`. Creates it if absent.
fn plugins_dir() -> Option<PathBuf> {
    let dir = ash_dir()?.join("plugins");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// Read & parse the manifest for the plugin installed under `plugins/<name>/`.
fn load_installed(name: &str) -> Option<PluginManifest> {
    let path = plugins_dir()?.join(name).join("plugin.at");
    let content = std::fs::read_to_string(&path).ok()?;
    parse_plugin_manifest(&content).ok()
}

/// List installed plugins (name, version, description, enabled state).
fn cmd_list(args: &[String]) -> Result<()> {
    let only_enabled = args.iter().any(|a| a == "--enabled");
    let dir = match plugins_dir() {
        Some(d) => d,
        None => {
            println!("No plugins installed.");
            return Ok(());
        }
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => {
            println!("No plugins installed.");
            return Ok(());
        }
    };
    let mut found: Vec<(String, PluginManifest)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let manifest = match std::fs::read_to_string(path.join("plugin.at"))
            .ok()
            .and_then(|s| parse_plugin_manifest(&s).ok())
        {
            Some(m) => m,
            None => continue,
        };
        found.push((name, manifest));
    }
    if found.is_empty() {
        println!("No plugins installed.");
        println!("Install one with: ash plugin install <git-url>");
        return Ok(());
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    // Apply the --enabled filter before printing so the header isn't shown when
    // nothing matches.
    let visible: Vec<_> = found
        .into_iter()
        .filter(|(_, m)| !only_enabled || m.enabled)
        .collect();
    if visible.is_empty() {
        println!("No enabled plugins.");
        return Ok(());
    }
    println!("Plugins:");
    for (name, m) in visible {
        let state = if m.enabled { "enabled" } else { "disabled" };
        let desc = m.description.as_deref().unwrap_or("");
        let author = m
            .author
            .as_deref()
            .map(|a| format!("  by {a}"))
            .unwrap_or_default();
        println!("  {} v{} [{}]  {}{}", name, m.version, state, desc, author);
    }
    Ok(())
}

/// Show one plugin's full manifest.
fn cmd_show(name: &str) -> Result<()> {
    let manifest = match load_installed(name) {
        Some(m) => m,
        None => {
            eprintln!("ash plugin show: no such plugin '{}'", name);
            std::process::exit(1);
        }
    };
    // The directory name is the plugin's identity (what enable/disable/remove
    // operate on). Show it first; note the manifest name if it differs.
    println!("plugin      : {}", name);
    if manifest.name != name {
        println!("            (manifest name: {})", manifest.name);
    }
    println!("version     : {}", manifest.version);
    if let Some(a) = &manifest.author {
        println!("author      : {}", a);
    }
    if let Some(d) = &manifest.description {
        println!("description : {}", d);
    }
    if let Some(h) = &manifest.homepage {
        println!("homepage    : {}", h);
    }
    let c = &manifest.contributions;
    println!(
        "contributions : completions={}, functions={}, smart={}, config={}",
        c.completions, c.functions, c.smart, c.config
    );
    if c.config {
        // config.at merge is a v1 placeholder — make clear it isn't applied yet.
        println!("            (note: config contribution declared but not merged in v1)");
    }
    let cap = &manifest.capabilities;
    if cap.is_empty() {
        println!("capabilities : (none declared)");
    } else {
        println!(
            "capabilities : reads_fs={}, writes_fs={}, spawns_process={}, uses_network={}",
            cap.reads_fs, cap.writes_fs, cap.spawns_process, cap.uses_network
        );
    }
    if let Some(m) = &manifest.min_ash_version {
        println!("min_ash_version : {}", m);
    }
    println!("enabled     : {}", manifest.enabled);
    if let Some(dir) = plugins_dir() {
        println!("location    : {}", dir.join(name).display());
    }
    Ok(())
}

/// Derive a plugin name from a git URL (last path segment, minus `.git`).
fn derive_name_from_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    trimmed
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("plugin")
        .to_string()
}

/// `ash plugin install`. Supports a git URL or `--local <path>` (copy).
fn cmd_install(args: &[String]) -> Result<()> {
    let mut local = false;
    let mut name_override: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--local" => {
                local = true;
            }
            "--name" => {
                i += 1;
                if i < args.len() {
                    name_override = Some(args[i].clone());
                }
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let source = match positional.first() {
        Some(s) => s.as_str(),
        None => {
            eprintln!("ash plugin install: missing source (git url or --local path)");
            eprintln!("  usage: ash plugin install <git-url> [--name <n>]");
            eprintln!("         ash plugin install --local <path> [--name <n>]");
            std::process::exit(2);
        }
    };

    let name = name_override
        .clone()
        .unwrap_or_else(|| derive_name_from_url(source));

    let dir = plugins_dir().ok_or_else(|| miette::miette!("cannot resolve ash config dir"))?;
    let target = dir.join(&name);
    if target.exists() {
        eprintln!(
            "ash plugin install: '{}' already installed at {}",
            name,
            target.display()
        );
        std::process::exit(1);
    }

    if local {
        install_local(Path::new(source), &target, &name)?;
    } else {
        install_git(source, &target, &name)?;
    }

    // Validate the install: plugin.at must exist and parse. A broken manifest
    // means this isn't a real plugin — clean up rather than leave junk behind.
    let manifest_path = target.join("plugin.at");
    let manifest = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| parse_plugin_manifest(&s).ok())
    {
        Some(m) => m,
        None => {
            eprintln!(
                "ash plugin install: '{}' has no valid plugin.at manifest — removing",
                name
            );
            let _ = std::fs::remove_dir_all(&target);
            std::process::exit(1);
        }
    };

    // Warn (don't block) if the plugin requires a newer ash than we are.
    if let Some(min) = &manifest.min_ash_version {
        if !crate::plugin::loader::ash_version_meets(min) {
            eprintln!(
                "⚠ plugin '{}' requires ash >= {} (you have {}); it will be skipped at load time",
                name,
                min,
                crate::plugin::loader::current_ash_version()
            );
        }
    }

    println!("✓ installed {} v{}", name, manifest.version);
    Ok(())
}

/// Copy a local directory tree into the plugins dir.
fn install_local(src: &Path, target: &Path, name: &str) -> Result<()> {
    if !src.is_dir() {
        eprintln!(
            "ash plugin install --local: '{}' is not a directory",
            src.display()
        );
        std::process::exit(1);
    }
    std::fs::create_dir_all(target)
        .map_err(|e| miette::miette!("create {}: {}", target.display(), e))?;
    copy_dir_recursive(src, target)
        .map_err(|e| miette::miette!("copy {}: {}", src.display(), e))?;
    let _ = name;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// `git clone --depth 1 <url> <target>`. Cleans up the target on failure.
fn install_git(url: &str, target: &Path, name: &str) -> Result<()> {
    let status = match std::process::Command::new("git")
        .args(["clone", "--depth", "1", url, &target.to_string_lossy()])
        .status()
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ash plugin install: failed to run git: {}", e);
            eprintln!("  (git must be installed and on your PATH)");
            let _ = std::fs::remove_dir_all(target);
            std::process::exit(1);
        }
    };
    if !status.success() {
        eprintln!("ash plugin install: git clone failed for {}", name);
        let _ = std::fs::remove_dir_all(target);
        std::process::exit(1);
    }
    Ok(())
}

/// Toggle a plugin's `enabled` field by rewriting its `plugin.at`.
fn cmd_set_enabled(name: &str, enabled: bool) -> Result<()> {
    let dir = match plugins_dir() {
        Some(d) => d,
        None => {
            eprintln!("ash plugin {}: cannot resolve plugins dir", name);
            std::process::exit(1);
        }
    };
    let manifest_path = dir.join(name).join("plugin.at");
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("ash plugin {}: no such plugin", name);
            std::process::exit(1);
        }
    };
    let mut manifest = match parse_plugin_manifest(&content) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ash plugin {}: invalid manifest: {}", name, e);
            std::process::exit(1);
        }
    };
    if manifest.enabled == enabled {
        let state = if enabled {
            "already enabled"
        } else {
            "already disabled"
        };
        println!("plugin {} {}", name, state);
        return Ok(());
    }
    manifest.enabled = enabled;
    let new_text = manifest.to_manifest_text();
    std::fs::write(&manifest_path, new_text)
        .map_err(|e| miette::miette!("write {}: {}", manifest_path.display(), e))?;
    let verb = if enabled { "enabled" } else { "disabled" };
    println!("✓ {} {}", verb, name);
    Ok(())
}

/// Remove a plugin directory.
fn cmd_remove(name: &str) -> Result<()> {
    let dir = match plugins_dir() {
        Some(d) => d,
        None => {
            eprintln!("ash plugin remove: cannot resolve plugins dir");
            std::process::exit(1);
        }
    };
    let target = dir.join(name);
    if !target.exists() {
        eprintln!("ash plugin remove: no such plugin '{}'", name);
        std::process::exit(1);
    }
    std::fs::remove_dir_all(&target)
        .map_err(|e| miette::miette!("remove {}: {}", target.display(), e))?;
    println!("✓ removed {}", name);
    Ok(())
}

/// `git pull` for one plugin, or `--all` for every git plugin.
fn cmd_update(args: &[String]) -> Result<()> {
    let all = args.iter().any(|a| a == "--all");
    let dir = match plugins_dir() {
        Some(d) => d,
        None => {
            eprintln!("ash plugin update: cannot resolve plugins dir");
            return Ok(());
        }
    };
    let targets: Vec<String> = if all {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(n) = entry.file_name().to_str() {
                        names.push(n.to_string());
                    }
                }
            }
        }
        names.sort();
        names
    } else if let Some(name) = args.first() {
        vec![name.clone()]
    } else {
        eprintln!("ash plugin update: specify a name or --all");
        std::process::exit(2);
    };

    for name in targets {
        let target = dir.join(&name);
        if !target.join(".git").exists() {
            eprintln!("plugin {}: not a git repository, skipping", name);
            continue;
        }
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&target)
            .args(["pull", "--ff-only"])
            .status();
        match status {
            Ok(s) if s.success() => println!("✓ updated {}", name),
            Ok(_) => eprintln!("plugin {}: git pull failed", name),
            Err(e) => eprintln!("plugin {}: git pull error: {}", name, e),
        }
    }
    Ok(())
}

fn print_usage() {
    eprintln!("usage: ash plugin <command> [args]");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  install <git-url> [--name <n>]   install a plugin from a git repo");
    eprintln!("  install --local <path> [--name <n>]   install a plugin from a local dir");
    eprintln!("  list [--enabled]                  list installed plugins");
    eprintln!("  show <name>                       show a plugin's manifest");
    eprintln!("  enable <name>                     enable a plugin");
    eprintln!("  disable <name>                    disable a plugin");
    eprintln!("  remove <name>                     remove a plugin");
    eprintln!("  update <name> | --all             git pull plugin(s)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn derive_name_from_https_url() {
        assert_eq!(
            derive_name_from_url("https://github.com/zhaopuming/ash-git-extras"),
            "ash-git-extras"
        );
    }

    #[test]
    fn derive_name_strips_dot_git() {
        assert_eq!(derive_name_from_url("git@github.com:foo/bar.git"), "bar");
    }

    #[test]
    fn derive_name_trailing_slash() {
        assert_eq!(derive_name_from_url("https://example.com/repo/"), "repo");
    }

    #[test]
    fn copy_dir_recursive_copies_files_and_subdirs() {
        let tmp = std::env::temp_dir().join("ash_plugin_copy_test");
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("sub").join("b.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();
        assert!(dst.join("a.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert!(dst.join("sub").join("b.txt").exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cmd_list_empty_dir_prints_none() {
        // cmd_list reads the real plugins dir; just ensure it returns Ok.
        cmd_list(&[]).unwrap();
    }

    #[test]
    fn version_check_is_permissive_on_garbage() {
        // A malformed min_ash_version must never block (the loader/installer
        // treat it as "any version satisfies").
        assert!(crate::plugin::loader::ash_version_meets("not-a-version"));
        assert!(crate::plugin::loader::ash_version_meets("0.0.1"));
        assert!(!crate::plugin::loader::ash_version_meets("99.0.0"));
    }
}
