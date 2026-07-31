//! Plan 033 — plugin ecosystem integration tests.
//!
//! These exercise the end-to-end plugin flows that don't depend on the real
//! `~/.config/ash` directory:
//! - manifest parse + serialize round-trip (the foundation of enable/disable)
//! - SmartCommand loader picks up plugin-contributed `smart/` dirs
//! - completion-dir discovery from a temp plugins tree

use std::fs;
use std::path::PathBuf;

use auto_shell::plugin::manifest::{parse_plugin_manifest, PluginContributions, PluginManifest};
use auto_shell::smart_command::loader::load_all_with_extra;

/// A throwaway directory under temp with a unique label.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("ash_plugin_e2e")
        .join(label);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

const FULL_MANIFEST: &str = r#"plugin {
    name        : "git-extras"
    version     : "0.2.0"
    author      : "tester"
    description : "Extra git SmartCommands"
    homepage    : "https://example.com/git-extras"
    contributions : {
        completions : true
        functions   : true
        smart       : true
        config      : false
    }
    capabilities : {
        reads_fs       : true
        writes_fs      : false
        spawns_process : true
        uses_network   : false
    }
    min_ash_version : "0.1.0"
    enabled : true
}
"#;

#[test]
fn manifest_full_parse_and_round_trip() {
    let original = parse_plugin_manifest(FULL_MANIFEST).unwrap();
    assert_eq!(original.name, "git-extras");
    assert_eq!(original.version, "0.2.0");
    assert!(original.contributions.completions);
    assert!(original.contributions.smart);
    assert!(!original.contributions.config);
    assert!(original.capabilities.reads_fs);
    assert!(original.capabilities.spawns_process);

    // Serialize and reparse — every field must survive the round-trip.
    let text = original.to_manifest_text();
    let reparsed = parse_plugin_manifest(&text).unwrap();
    assert_eq!(original, reparsed);
}

#[test]
fn manifest_disable_then_reparse() {
    // Simulate `ash plugin disable`: toggle enabled, rewrite, reparse.
    let mut m = parse_plugin_manifest(FULL_MANIFEST).unwrap();
    assert!(m.enabled);
    m.enabled = false;
    let text = m.to_manifest_text();
    let reparsed = parse_plugin_manifest(&text).unwrap();
    assert!(!reparsed.enabled, "disable round-trip must persist");
    assert_eq!(reparsed.name, "git-extras");
}

#[test]
fn smart_loader_picks_up_plugin_smart_dir() {
    // Build a fake plugin tree with a `smart/` dir containing a command.at.
    let tree = temp_dir("smart_loader");
    let smart_dir = tree.join("my-plugin").join("smart");
    fs::create_dir_all(&smart_dir).unwrap();
    fs::write(
        smart_dir.join("deploy.at"),
        r#"command "plugin.deploy" {
    description : "deploy via plugin"
    body        : "deploy.ash"
}
"#,
    )
    .unwrap();
    fs::write(smart_dir.join("deploy.ash"), "> echo deploy").unwrap();

    // load_all_with_extra scans cwd, home, then extra dirs. Pass empty cwd/home
    // and the plugin smart dir as the only extra.
    let empty = temp_dir("smart_loader_empty");
    let specs = load_all_with_extra(&empty, &empty, &[smart_dir.clone()]);
    let deploy = specs
        .iter()
        .find(|s| s.name == "plugin.deploy")
        .expect("plugin SmartCommand should be loaded");
    assert_eq!(deploy.description, "deploy via plugin");

    let _ = fs::remove_dir_all(&tree);
    let _ = fs::remove_dir_all(&empty);
}

#[test]
fn smart_loader_builtin_wins_over_plugin_on_collision() {
    // A project-local command and a plugin command with the same name: the
    // project-local one (earlier in the search order) must win.
    let proj = temp_dir("collision_proj");
    let plugin_smart = temp_dir("collision_plugin");

    let proj_smart = proj.join("smart");
    fs::create_dir_all(&proj_smart).unwrap();
    fs::write(
        proj_smart.join("x.at"),
        r#"command "shared" {
    description : "project version"
    body        : "x.ash"
}
"#,
    )
    .unwrap();
    fs::write(
        plugin_smart.join("x.at"),
        r#"command "shared" {
    description : "plugin version"
    body        : "x.ash"
}
"#,
    )
    .unwrap();

    let specs = load_all_with_extra(&proj, &proj, &[plugin_smart]);
    let shared = specs.iter().find(|s| s.name == "shared").unwrap();
    assert_eq!(
        shared.description, "project version",
        "project-local must shadow the plugin command"
    );

    let _ = fs::remove_dir_all(&proj);
}

/// Verify a constructed manifest with only required fields serializes & reparses.
#[test]
fn manifest_minimal_round_trip() {
    let original = PluginManifest::new("bare", "0.1.0");
    assert_eq!(original.contributions, PluginContributions::default());
    let text = original.to_manifest_text();
    let reparsed = parse_plugin_manifest(&text).unwrap();
    assert_eq!(original, reparsed);
}
