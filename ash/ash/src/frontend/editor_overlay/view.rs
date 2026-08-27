//! Rendering for the editor modal (Plan 070): a rounded-border input box —
//! mode hint as the top title, key hints as the bottom title, textarea fills
//! the inner area. Also renders the committed echo (the same box style, dim,
//! line-numbered) as an ANSI string for linear printing.

use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::Line;
use ratatui_textarea::TextArea;
use ratatui_widgets::block::Block;
use ratatui_widgets::borders::BorderType;
use std::io;

use crate::frontend::tail::TailTerminal;

/// Cap for echo content columns — wider lines truncate with `…` so the box
/// never wraps in the terminal.
const ECHO_MAX_CONTENT_COLS: usize = 68;

/// One frame of the input box. Returns the textarea's inner area so the
/// caller can place the hardware cursor.
pub fn draw(
    terminal: &mut TailTerminal,
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

/// Render the submitted/cancelled script as a dim, rounded-border,
/// line-numbered box — the same visual language as the live editor box,
/// returned as an ANSI string for linear printing into the scrollback.
///
/// `cancelled` adds a `已取消` bottom title. Truncates over-wide lines with
/// `…` so the box never wraps.
pub fn render_script_block(title: &str, text: &str, cancelled: bool) -> String {
    use ratatui_core::buffer::Buffer;
    use ratatui_core::text::Span;
    use ratatui_core::widgets::Widget;
    use unicode_width::UnicodeWidthStr;

    let dim = Style::default().fg(Color::DarkGray);
    let lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect()
    };
    let lnum_width = lines.len().to_string().len();

    // Width-based truncation with `…` (chars may be double-wide).
    let truncate = |s: &str| -> String {
        if UnicodeWidthStr::width(s) <= ECHO_MAX_CONTENT_COLS {
            return s.to_string();
        }
        let mut acc = String::new();
        for ch in s.chars() {
            let w = UnicodeWidthStr::width(ch.to_string().as_str());
            if UnicodeWidthStr::width(acc.as_str()) + w > ECHO_MAX_CONTENT_COLS - 1 {
                break;
            }
            acc.push(ch);
        }
        acc.push('…');
        acc
    };

    let mut content: Vec<String> = Vec::with_capacity(lines.len());
    let mut content_width = 0usize;
    for line in &lines {
        let t = truncate(line);
        content_width = content_width.max(UnicodeWidthStr::width(t.as_str()));
        content.push(t);
    }
    let title_text = format!(" {title} ");
    let bottom_text = if cancelled { " 已取消 " } else { "" };
    let inner_width = (lnum_width + 1 + content_width)
        .max(UnicodeWidthStr::width(title_text.as_str()))
        .max(UnicodeWidthStr::width(bottom_text));
    let width = (inner_width + 2) as u16; // borders
    let height = (lines.len() + 2) as u16;

    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(dim)
        .title(Line::from(title_text).style(Style::default().fg(Color::DarkGray)));
    let block = if cancelled {
        block.title_bottom(Line::from(bottom_text).style(Style::default().fg(Color::Yellow)))
    } else {
        block
    };
    let inner = block.inner(buf.area);
    block.render(buf.area, &mut buf);
    for (i, line) in content.iter().enumerate() {
        let spans = vec![
            Span::styled(format!("{:>lw$} ", i + 1, lw = lnum_width), dim),
            Span::styled(line.clone(), dim),
        ];
        Line::from(spans).render(
            Rect::new(inner.x, inner.y + i as u16, inner.width, 1),
            &mut buf,
        );
    }
    crate::renderer::buffer_to_ansi(&buf)
}

#[cfg(test)]
mod tests {
    use super::render_script_block;

    #[test]
    fn boxed_echo_renders_borders_numbers_and_all_lines() {
        // 3 content lines → 3 numbered rows + 2 border rows = 5 rows.
        let ansi = render_script_block("▌# AutoScript", "fn add(a) {\n    a + 1\n}", false);
        assert!(ansi.contains('╭'), "missing top-left corner: {ansi:?}");
        assert!(ansi.contains('╰'), "missing bottom-left corner: {ansi:?}");
        assert_eq!(ansi.matches('\n').count(), 4);
        assert!(ansi.contains('1') && ansi.contains('2') && ansi.contains('3'));
    }

    #[test]
    fn cancelled_echo_mentions_cancelled() {
        // buffer_to_ansi paints per-cell (wide chars carry a trailing
        // continuation-space cell), so compare against the ANSI-and-space
        // -stripped text.
        let ansi = render_script_block("> 命令", "echo hi", true);
        let stripped: String = strip_ansi(&ansi)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(stripped.contains("已取消"));
    }

    #[test]
    fn overwide_line_truncates_instead_of_wrapping() {
        let long = "x".repeat(200);
        let ansi = render_script_block("> 命令", &long, false);
        // Every rendered row must not exceed the cap + borders + gutter.
        for line in ansi.split('\n') {
            let plain: String = strip_ansi(line);
            assert!(
                plain.chars().count() <= 2 + 1 + super::ECHO_MAX_CONTENT_COLS + 1,
                "echo row too wide ({} cols): {:?}",
                plain.chars().count(),
                plain
            );
        }
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            match (in_esc, c) {
                (false, '\x1b') => in_esc = true,
                (true, 'm') => in_esc = false,
                (false, ch) => out.push(ch),
                _ => {}
            }
        }
        out
    }
}
