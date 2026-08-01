//! Frontend module — terminal-dependent rendering layer
//!
//! This module contains code that depends on terminal libraries (reedline,
//! ratatui, nu-ansi-term, crossterm). It will eventually become the `ash-tui`
//! crate.
//!
//! ## Architecture
//!
//! - `renderer/` — ratatui Buffer → ANSI string conversion bridge
//! - `repl` — Read-Eval-Print Loop (reedline-driven)
//! - `term/` — Terminal utilities (highlight, prompt)
//! - `completions_reedline` — reedline Completer adapter

// Terminal-dependent submodules — only compiled with the frontend-tui feature.
#[cfg(feature = "frontend-tui")]
pub mod renderer;
#[cfg(feature = "frontend-tui")]
pub mod repl;
#[cfg(feature = "frontend-tui")]
pub mod term;
#[cfg(feature = "frontend-tui")]
pub mod completions_reedline;

// Terminal-dep-FREE submodules. These live under `frontend/` historically but
// have no reedline/crossterm/ratatui/nu-ansi-term usage, so they stay available
// without the frontend feature (smart_command::nlu uses `ai::block_on_async`,
// main.rs uses `ask::run`). (Plan 030 M0: candidates to move out of frontend/.)
pub mod ai;
pub mod ai_context;
pub mod ask;
pub mod brief;
pub mod suggest;
