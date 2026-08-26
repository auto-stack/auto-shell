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

/// Close the modal cleanly: blank the viewport rows (a final empty frame
/// erases the editor visuals), park the cursor on the line below, and show
/// it. After this the caller prints linearly from a fresh line.
pub fn exit_modal(terminal: &mut EditorTerminal) {
    // Empty frame → ratatui diffs against the last (editor) frame and clears
    // the viewport rows.
    let _ = terminal.draw(|_f| {});
    let bottom = terminal.get_frame().area().bottom();
    let last_row = bottom.saturating_sub(1);
    let _ = ratatui_crossterm::crossterm::execute!(
        io::stdout(),
        ratatui_crossterm::crossterm::cursor::MoveTo(0, last_row),
        ratatui_crossterm::crossterm::style::Print("\r\n"),
        ratatui_crossterm::crossterm::cursor::Show
    );
}
