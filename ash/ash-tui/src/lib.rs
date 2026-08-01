//! ash-tui — terminal frontend for ASH (Plan 037 M2.2)
//!
//! Contains the reedline-driven REPL, ratatui structured-output rendering,
//! completion/prompt/menu terminal adapters, and the terminal-only commands
//! (`less`/`more`/`color`). Built on top of `auto-shell` (pure Shell logic).
//!
//! This crate exists so that `auto-shell` has ZERO terminal dependencies —
//! the crate boundary provides the isolation that the `frontend-tui` feature
//! flag used to give.
