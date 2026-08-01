//! AshPrompt — modular prompt engine for AutoShell
//!
//! Inspired by Starship's architecture, but with minimal dependencies:
//! - `rayon` for parallel module rendering
//! - `toml` for configuration
//! - `nu-ansi-term` for ANSI styling (already a dependency)
//!
//! # Quick start
//!
//! ```ignore
//! use auto_shell::prompt::{AshPrompt, config::AshConfig};
//!
//! let prompt = AshPrompt::new(AshConfig::load());
//! // Use with reedline: Reedline::create().with_prompt(prompt)
//! ```

pub mod config;
pub mod context;
// Plan 030 M0: the prompt engine/module styling use nu-ansi-term + reedline —
// gate them with the TUI frontend. `config` and `context` are terminal-dep-free
// and consumed by shell.rs, so they stay ungated.
#[cfg(feature = "frontend-tui")]
pub mod engine;
#[cfg(feature = "frontend-tui")]
pub mod module;
#[cfg(feature = "frontend-tui")]
pub mod modules;

pub use config::AshConfig;
pub use context::AshContext;
#[cfg(feature = "frontend-tui")]
pub use engine::AshPrompt;
#[cfg(feature = "frontend-tui")]
pub use module::{PromptModule, PromptSegment, SegmentStyle};
