//! Rendering for the editor modal (Plan 070): textarea + status line inside
//! the fixed-height inline viewport.

use ratatui_core::layout::{Constraint, Layout, Rect};
use ratatui_core::style::{Color, Style};
use ratatui_core::text::Line;
use ratatui_textarea::TextArea;
use std::io;

use super::term::EditorTerminal;

/// One frame: textarea on top (fills), dim status line at the bottom.
/// Returns the textarea's chunk so the caller can place the hardware cursor.
pub fn draw(
    terminal: &mut EditorTerminal,
    textarea: &mut TextArea,
    status: &str,
) -> io::Result<Rect> {
    let mut chunk = Rect::default();
    terminal.draw(|f| {
        let chunks =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(f.area());
        f.render_widget(&*textarea, chunks[0]);
        let hint = Line::styled(
            format!(" {status}"),
            Style::default().fg(Color::DarkGray),
        );
        f.render_widget(hint, chunks[1]);
        chunk = chunks[0];
    })?;
    Ok(chunk)
}
