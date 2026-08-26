//! ash-tui — terminal frontend for ASH (Plan 037 M2.2)
//!
//! Contains the reedline-driven REPL, ratatui structured-output rendering,
//! completion/prompt/menu terminal adapters, and the terminal-only commands
//! (`less`/`more`/`color`). Built on top of `auto-shell` (pure Shell logic).
//!
//! This crate exists so that `auto-shell` has ZERO terminal dependencies —
//! the crate boundary provides the isolation that the `frontend-tui` feature
//! flag used to give.

// Terminal-dependent modules moved here from auto-shell in Plan 037 M2.2.
pub mod block_header;
/// Plan 038 M0: experimental ratatui inline-viewport block TUI (counterpart
/// to the reedline-driven [`Repl`]). Owned by the `--block-tui` CLI flag.
pub mod block_tui;
pub mod commands;
/// Plan 038 M1: the bottom-line input editor for the block TUI.
pub mod editor;
/// Plan 038 M3: fullscreen subprocess handoff (teardown/rebuild ratatui).
pub mod subprocess;
// `commands_less.rs` is the original `less`/`more` implementation (crossterm),
// moved verbatim; `commands` re-exports it and adds `color`.
mod commands_less;
pub mod completions_reedline;
/// Plan 070: the bottom-dynamic script editor modal (ratatui Inline viewport).
pub mod editor_overlay;
pub mod menu;
pub mod prompt;
pub mod renderer;
pub mod repl;
pub mod term;

// Re-export the entry-point type for the `ash` binary (composition root).
pub use repl::Repl;
