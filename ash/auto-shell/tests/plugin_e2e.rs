//! Plan 033 — plugin ecosystem integration tests.
//!
//! These exercise the end-to-end plugin flows that don't depend on the real
//! `~/.config/ash` directory:
//! - manifest parse + serialize round-trip (the foundation of enable/disable)
//! - SmartCommand loader picks up plugin-contributed `smart/` dirs
//! - completion-dir discovery from a temp plugins tree

use std::fs;
use std::path::PathBuf;

use auto_shell::core::security::SecurityPolicy;
use auto_shell::plugin::loader::load_all_plugins_from;
use auto_shell::plugin::manifest::{parse_plugin_manifest, PluginContributions, PluginManifest};
use auto_shell::shell::Shell;
use auto_shell::smart_command::loader::load_all_with_extra;

/// A throwaway directory under temp with a unique label.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("ash_plugin_e2e").join(label);
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
    // Plan 033 §3.1 documented layout: smart/<cmd>/command.at (+ body.ash
    // alongside). The plugin loader passes the plugin's `smart/` dir as an
    // extra search dir.
    let tree = temp_dir("smart_loader");
    let smart_dir = tree.join("my-plugin").join("smart");
    let deploy_dir = smart_dir.join("deploy");
    fs::create_dir_all(&deploy_dir).unwrap();
    fs::write(
        deploy_dir.join("command.at"),
        r#"command "plugin.deploy" {
    description : "deploy via plugin"
    body        : "deploy.ash"
}
"#,
    )
    .unwrap();
    fs::write(deploy_dir.join("deploy.ash"), "> echo deploy").unwrap();

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

// ── Plan 033 M3: security — a plugin runs its functions inside the shell, so
// whatever the shell's SecurityPolicy refuses, the plugin cannot do. v1 relies
// on Plan 028's policy (no new sandbox/signing mechanism). These tests pin that
// contract: load a plugin into a shell, then verify the commands a plugin would
// issue (touch, an external process) are blocked under --read-only / --no-exec. ──

/// Build a plugin that contributes a functions.ash (the surface a malicious
/// plugin uses to run code) declaring write + process capabilities.
fn write_plugin_with_functions(plugins_dir: &PathBuf) {
    let p = plugins_dir.join("malicious");
    fs::create_dir_all(&p).unwrap();
    fs::write(
        p.join("plugin.at"),
        "plugin {\n    name    : \"malicious\"\n    version : \"0.1.0\"\n    contributions : { functions : true }\n    capabilities : {\n        writes_fs      : true\n        spawns_process : true\n    }\n    enabled : true\n}\n",
    )
    .unwrap();
    // A function that, when called from a script, writes a file via system().
    fs::write(p.join("functions.ash"), "fn pwn(path) { system(\"touch \" + path) }").unwrap();
}

#[test]
fn plugin_loaded_shell_blocks_write_under_read_only() {
    // The plugin is loaded into a read-only shell. The write a plugin function
    // would perform (touch) is refused by the policy the shell applies to every
    // command — including those spawned by a plugin's system() calls.
    let dir = temp_dir("security_readonly");
    write_plugin_with_functions(&dir);
    let target = dir.join("blocked.txt");

    let mut shell = Shell::new();
    shell.set_policy(SecurityPolicy {
        read_only: true,
        ..Default::default()
    });
    let report = load_all_plugins_from(&mut shell, &dir).unwrap();
    assert_eq!(report.loaded, vec!["malicious".to_string()]);

    // The plugin's intent (touch) executed through this policy is refused.
    let _ = shell.execute(&format!("touch {}", target.display()));
    assert!(
        !target.exists(),
        "--read-only must block the write the plugin would perform"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn plugin_loaded_shell_blocks_external_under_no_exec() {
    // Under --no-exec, an external process a plugin's system() would spawn is
    // refused.
    let dir = temp_dir("security_noexec");
    write_plugin_with_functions(&dir);

    let mut shell = Shell::new();
    shell.set_policy(SecurityPolicy {
        no_exec: true,
        ..Default::default()
    });
    load_all_plugins_from(&mut shell, &dir).unwrap();

    // An external command (git --version, run via system()) is blocked.
    let res = shell.execute("git --version");
    assert!(
        res.is_err() || shell.last_exit_code() != 0,
        "--no-exec must refuse the external command a plugin could spawn"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn plugin_function_source_succeeds_under_policy() {
    // Loading the plugin itself (sourcing functions.ash) must work even under a
    // restrictive policy — defining a function is not executing a dangerous
    // command. The policy gates execution, not declaration.
    let dir = temp_dir("security_source");
    write_plugin_with_functions(&dir);

    let mut shell = Shell::new();
    shell.set_policy(SecurityPolicy {
        read_only: true,
        no_exec: true,
        ..Default::default()
    });
    let report = load_all_plugins_from(&mut shell, &dir).unwrap();
    assert_eq!(report.loaded, vec!["malicious".to_string()]);
    let _ = fs::remove_dir_all(&dir);
}
