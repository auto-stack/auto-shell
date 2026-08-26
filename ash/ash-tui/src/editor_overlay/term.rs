//! Terminal lifecycle for the editor modal (Plan 070).
//!
//! RAII pattern mirrors block_tui.rs's `TerminalGuard`: raw mode is restored
//! on every exit path including panics. No alternate screen, no mouse capture
//! — the whole point is that native scrollback/copy stays usable.

use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_crossterm::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self, Stdout};

pub type EditorTerminal = Terminal<ratatui_crossterm::CrosstermBackend<Stdout>>;

/// RAII: enable raw mode on create, restore on drop (incl. panic unwinding).
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter(height: u16) -> io::Result<(Self, EditorTerminal)> {
        enable_raw_mode()?;
        let backend = ratatui_crossterm::CrosstermBackend::new(io::stdout());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Inline(height) },
        )?;
        Ok((Self, terminal))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

/// Close the modal cleanly: clear the box rows IN PLACE (cursor to the box's
/// top-left, clear to end of screen — the rows below were already overwritten
/// by the box draws). The caller then prints the committed echo starting at
/// that row, so no blank gap remains between the transcript and the echo.
pub fn exit_modal(terminal: &mut EditorTerminal) {
    let area = terminal.get_frame().area();
    let _ = ratatui_crossterm::crossterm::execute!(
        io::stdout(),
        ratatui_crossterm::crossterm::cursor::MoveTo(0, area.y),
        ratatui_crossterm::crossterm::terminal::Clear(
            ratatui_crossterm::crossterm::terminal::ClearType::FromCursorDown
        ),
    );
}
