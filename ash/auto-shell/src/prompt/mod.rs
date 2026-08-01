//! AshPrompt — modular prompt engine for AutoShell
//!
//! Inspired by Starship's architecture, but with minimal dependencies:
//! - `rayon` for parallel module rendering
//! - `toml` for configuration
//! - `nu-ansi-term` for ANSI styling (in ash-tui now)
//!
//! # Plan 037 M2.2 split
//!
//! This module retains only the terminal-dep-free parts (`config`, `context`),
//! which are consumed by `shell.rs`. The terminal-dependent parts — the
//! `AshPrompt` engine (impls `reedline::Prompt`), `PromptModule`/`PromptSegment`,
//! and the `modules/` (which use nu-ansi-term) — moved to the **ash-tui** crate
//! at `ash_tui::prompt`. They import `AshConfig`/`AshContext`/`GitInfo`/`GitStatus`
//! back across the crate boundary from here.

pub mod config;
pub mod context;

pub use config::AshConfig;
pub use context::AshContext;
