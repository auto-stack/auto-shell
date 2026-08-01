//! Renderer module — ratatui Buffer to terminal output bridges
//!
//! This module provides the conversion layer between ratatui's in-memory
//! `Buffer` (rendered by widgets) and ANSI strings that can be displayed
//! in the terminal via reedline or direct stdout.

pub mod buffer_to_ansi;
pub mod table;
// Plan 030 M1: TUI renderer that consumes ash-core's RenderedOutput.
pub mod tui;

pub use buffer_to_ansi::{buffer_to_ansi, buffer_to_plain};
pub use table::{render_table, render_table_with};
pub use tui::{rendered_to_ansi, TuiRenderer};
