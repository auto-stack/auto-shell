//! Rendering for the editor modal (Plan 070): a rounded-border input box —
//! mode hint as the top title, key hints as the bottom title, textarea fills
//! the inner area.

use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::Line;
use ratatui_textarea::TextArea;
use ratatui_widgets::block::Block;
use ratatui_widgets::borders::BorderType;
use std::io;

use super::term::EditorTerminal;

/// One frame of the input box. Returns the textarea's inner area so the
/// caller can place the hardware cursor.
pub fn draw(
    terminal: &mut EditorTerminal,
    textarea: &mut TextArea,
    title: &str,
    hints: &str,
) -> io::Result<Rect> {
    let mut inner = Rect::default();
    terminal.draw(|f| {
        let area = f.area();
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(
                Line::from(format!(" {title} "))
                    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            )
            .title_bottom(
                Line::from(format!(" {hints} "))
                    .style(Style::default().fg(Color::DarkGray)),
            );
        f.render_widget(&block, area);
        inner = block.inner(area);
        f.render_widget(&*textarea, inner);
    })?;
    Ok(inner)
}
