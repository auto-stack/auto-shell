//! AutoShell - A modern shell environment using AutoLang
//!
//! This library provides the core functionality for the AutoShell REPL,
//! command execution, and pipeline system.
//!
//! ## Architecture (Plan 037 M2.2 → Plan 071 融合)
//!
//! - `auto-shell` (this crate) — pure Shell logic + commands, ZERO terminal deps
//! - `ash` crate — CLI binary + terminal frontend modules (`frontend/`,
//!   reedline/crossterm/ratatui; was the separate ash-tui crate until Plan 071)
//! - `ash-core` crate — pure logic, zero terminal dependencies
//! - `cmd/`, `completions/`, `data/`, `shell/` — Shell logic layer

// Core layer: re-export ash-core crate as `core` module for backward compatibility
pub use ash_core as core;

// Plan 037 M2.0: terminal-dep-free AI modules (moved out of frontend/ in M2.0;
// frontend/ itself was removed in M2.2 when its terminal-dependent contents
// moved to the ash-tui crate).
pub mod ai;

// Legacy modules (will migrate into ash-core or frontend over time)
pub mod auto_config;
pub mod cmd;
pub mod completions;
pub mod config;
pub mod data;
pub mod host;
pub mod job;
// Plan 037 M2.2: `menu` moved to the ash-tui crate (only the TUI REPL consumes it).
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

// Plan 037 M2.2: `repl`/`term`/`Repl`/`menu` moved to the ash-tui crate. The
// Shell logic layer (below) stays here.
pub use shell::Shell;
