//! AutoShell - A modern shell environment using AutoLang
//!
//! This library provides the core functionality for the AutoShell REPL,
//! command execution, and pipeline system.
//!
//! ## Architecture
//!
//! - `ash-core` crate — Pure logic, zero terminal dependencies
//! - `frontend/` — Terminal-dependent code (will become `ash-tui` crate)
//! - `cmd/`, `completions/`, `data/`, `shell/` — Mixed layer, migrating

// Core layer: re-export ash-core crate as `core` module for backward compatibility
pub use ash_core as core;

// Frontend layer. Plan 030 M0: the module is always declared, but its
// terminal-dependent submodules (renderer/repl/term/completions_reedline) are
// gated behind `frontend-tui` inside frontend/mod.rs. The dep-free submodules
// (ai/ai_context/ask/suggest) stay available without the feature.
pub mod frontend;

// Legacy modules (will migrate into ash-core or frontend over time)
pub mod auto_config;
pub mod cmd;
pub mod completions;
pub mod config;
pub mod data;
pub mod host;
pub mod job;
// Plan 030 M0: `menu` is only consumed by the TUI REPL (frontend::repl).
#[cfg(feature = "frontend-tui")]
pub mod menu;
pub mod plugin;
pub mod prompt;
pub mod repl_mode;
pub mod shell;
pub mod signal;
pub mod smart_command;
pub mod ash_command_tool;

/// Default `~/.ashrc` content, seeded on first start so users discover the
/// user-defined-functions feature. Editable by the user afterwards.
pub const DEFAULT_ASHRC: &str = include_str!("default_ashrc.txt");

// Re-export core modules at crate root for backward compatibility
pub use ash_core::bookmarks;
pub use ash_core::parser;
pub use ash_core::pipeline;

// Re-export frontend modules at crate root for backward compatibility.
// Plan 030 M0: gated with the frontend feature.
#[cfg(feature = "frontend-tui")]
pub use frontend::repl;
#[cfg(feature = "frontend-tui")]
pub use frontend::term;

#[cfg(feature = "frontend-tui")]
pub use repl::Repl;
pub use shell::Shell;
