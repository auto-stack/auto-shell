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

// Plan 037 M2.0: the terminal-dep-FREE modules (ai/ai_context/ask/brief/suggest)
// moved out of frontend/ to the crate-root `ai/` module, so they don't get
// pulled into ash-tui and so smart_command/main don't gain a dep on the TUI.
