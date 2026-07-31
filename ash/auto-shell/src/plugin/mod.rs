//! Plugin ecosystem — data-only directory packages + git distribution (Plan 033).
//!
//! A plugin is a directory under `~/.config/ash/plugins/<name>/` containing a
//! `plugin.at` manifest plus optional content (completion specs, AutoLang
//! functions, SmartCommands). `ash plugin install <git-url>` clones a plugin;
//! ash loads all enabled plugins at startup.
//!
//! ## Module layout
//! - [`manifest`] — `PluginManifest` + `.at` parsing/serialization
//! - [`loader`] — discover & load plugins at startup (the 4 contribution types)
//! - [`cli`] — the `ash plugin` subcommand handler
//!
//! See `designs/033-plugin-ecosystem.md` for the design.

pub mod cli;
pub mod loader;
pub mod manifest;

pub use loader::{load_all_plugins, PluginLoadReport};
pub use manifest::{
    parse_plugin_manifest, Capabilities, PluginContributions, PluginError, PluginManifest,
};
