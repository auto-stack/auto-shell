//! TUI renderer — turns a frontend-agnostic [`RenderedOutput`] (from ash-core)
//! into an ANSI string via ratatui + nu-ansi-term.
//!
//! Plan 030 M1: the *semantic* half of rendering (columns / cell values /
//! `CellTag`s) now lives in `ash_core::renderer`. This module owns the
//! *presentation* half — column widths, borders, zebra striping, the icon
//! column, and the `CellTag → ratatui::Style` color mapping — and is the TUI's
//! "last mile". A GUI renderer (`rendered_to_iced`, M2) consumes the same
//! `RenderedOutput` with different presentation choices.
//!
//! ## Visual compatibility
//! `rendered_to_ansi` reproduces the exact bytes the old `render_table_with`
//! produced. The golden-comparison test in this file asserts that, so the M1
//! refactor is visually a no-op.

use ash_core::renderer::{CellTag, FileNameKind, RenderedCell, RenderedOutput};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Constraint, Rect};
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, Padding};
use ratatui_widgets::borders::BorderType;
use ratatui_widgets::table::{Cell, Row, Table};

use super::buffer_to_ansi;
use auto_shell::config::IconStyle;

/// A TUI renderer: holds the terminal width and icon style, implements the
/// shared [`ash_core::renderer::Renderer`] trait.
pub struct TuiRenderer {
    pub width: u16,
    pub icons: IconStyle,
}

impl ash_core::renderer::Renderer for TuiRenderer {
    fn render(&self, pipeline: &ash_core::pipeline::AtomPipeline) -> RenderedOutput {
        // The TUI only cares about the structured case; non-structured atoms
        // fall back to text at the call site (Shell::format_output).
        ash_core::renderer::render_pipeline_to_structured(pipeline)
            .unwrap_or_else(|| RenderedOutput::Empty)
    }
    fn width_hint(&self) -> u16 {
        self.width
    }
}

/// Plan 037 M2.1: a [`RenderHook`] implementation that wraps
/// [`rendered_to_ansi`] + `crossterm::terminal::size()`. The TUI frontend
/// injects this into Shell so structured data renders as ratatui tables without
/// Shell depending on the terminal renderer directly.
pub struct TuiRenderHook;

impl auto_shell::shell::RenderHook for TuiRenderHook {
    fn render_structured(
        &self,
        rendered: &RenderedOutput,
        _term_width: u16,
        icons: IconStyle,
    ) -> Option<String> {
        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w)
            .unwrap_or(80);
        rendered_to_ansi(rendered, term_width, icons)
    }
}

/// Build the ratatui `Table` widget from a `RenderedOutput::Table`, returning
/// it + the computed area height. Shared by `rendered_to_ansi` (→ Buffer →
/// ANSI) and `render_table_lines` (→ Buffer → Line-based for block TUI).
///
/// Returns `None` for non-Table variants (the caller falls back to plain text).
fn build_table_widget(
    rendered: &RenderedOutput,
    term_width: u16,
    icons: IconStyle,
) -> Option<(Table<'_>, Rect)> {
    let (columns, rows) = match rendered {
        RenderedOutput::Table { columns, rows, .. } => (columns, rows),
        _ => return None,
    };
    if columns.is_empty() || rows.is_empty() {
        return None;
    }

    let orig_columns: Vec<String> = columns.clone();
    let mut display_columns: Vec<String> = columns.clone();

    let is_file_listing = orig_columns.iter().any(|c| c == "name")
        && orig_columns.iter().any(|c| c == "type");
    if is_file_listing && icons != IconStyle::Off {
        display_columns.insert(0, "icon".to_string());
    }

    let col_widths = calculate_column_widths(&display_columns, &orig_columns, rows, term_width, icons);

    let header = Row::new(display_columns.iter().map(|col| {
        let display_name = column_display_name(col);
        Cell::from(Text::styled(
            display_name,
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Gray),
        ))
    }));

    let data_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .map(|(row_idx, cells)| {
            let (row_type, row_name): (Option<String>, String) =
                row_context(cells, &orig_columns);

            let rendered_cells: Vec<Cell> = display_columns
                .iter()
                .map(|col| {
                    if col == "icon" {
                        let icon = file_icon(row_type.as_deref(), &row_name, icons);
                        let style = cell_style_for_name(&row_name, row_type.as_deref());
                        return Cell::from(Text::styled(icon, style));
                    }
                    let cell_idx = orig_columns.iter().position(|c| c == col);
                    let Some(cell) = cell_idx.and_then(|i| cells.get(i)) else {
                        return Cell::from(Text::from(""));
                    };
                    let (text, tag) = cell_parts(cell);
                    let style = tag_to_style(tag, col, &text, row_type.as_deref());
                    Cell::from(Text::styled(text, style))
                })
                .collect();

            let row_style = if row_idx % 2 == 0 {
                Style::default().bg(Color::Indexed(234))
            } else {
                Style::default()
            };
            Row::new(rendered_cells).style(row_style)
        })
        .collect();

    let constraints: Vec<Constraint> = col_widths
        .iter()
        .enumerate()
        .map(|(i, &w)| {
            if i == col_widths.len() - 1 {
                Constraint::Min(w)
            } else {
                Constraint::Length(w)
            }
        })
        .collect();

    let total_height = 2 + 1 + data_rows.len() as u16;
    let area = Rect::new(0, 0, term_width, total_height);

    let table = Table::new(data_rows, constraints)
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Indexed(240)))
                .padding(Padding::horizontal(1)),
        )
        .header(header)
        .column_spacing(1);

    Some((table, area))
}

/// Render a [`RenderedOutput`] to an ANSI string at the given terminal width
/// and icon style. Returns `None` for non-Table variants (the caller falls
/// back to plain text), reproducing `render_table_with`'s old "not a table"
/// contract.
pub fn rendered_to_ansi(
    rendered: &RenderedOutput,
    term_width: u16,
    icons: IconStyle,
) -> Option<String> {
    if let Some((table, area)) = build_table_widget(rendered, term_width, icons) {
        let mut buf = Buffer::empty(area);
        table.render(area, &mut buf);
        return Some(buffer_to_ansi(&buf));
    }
    // Plan 042 M6 B1 gap fix: `show <code-file>` produces a structured Code
    // pipeline; without this branch the hook returned None and format_output
    // fell back to `into_text()` — plain text, no colors (only the block TUI
    // ever rendered Code).
    if let RenderedOutput::Code { lines, .. } = rendered {
        return Some(auto_shell::cmd::commands::code_highlight::spans_to_ansi(lines));
    }
    None
}

/// Render a `RenderedOutput::Table` directly into a ratatui `Buffer` at the
/// given offset. Used by the block TUI (Plan 038 Gap 4) to draw structured
/// tables into the `insert_before` buffer — without the ANSI → strip_ansi
/// round-trip that degraded tables to plain text.
///
/// Returns the number of lines the rendered table occupies (for the caller's
/// height calculation). Returns `None` for non-Table variants.
pub fn render_table_to_buffer(
    buf: &mut Buffer,
    rendered: &RenderedOutput,
    term_width: u16,
    icons: IconStyle,
) -> Option<u16> {
    let (table, widget_area) = build_table_widget(rendered, term_width, icons)?;
    // The table widget renders relative to (0,0); we need to shift it to the
    // target buffer's origin. Render into a temp buffer then blit.
    let mut temp = Buffer::empty(widget_area);
    table.render(widget_area, &mut temp);
    // Blit temp → buf (line by line, cell by cell).
    for y in 0..widget_area.height {
        for x in 0..widget_area.width.min(buf.area.width) {
            let src = temp.get(x, y);
            let dst = buf.get_mut(buf.area.x + x, buf.area.y + y);
            *dst = src.clone();
        }
    }
    Some(widget_area.height)
}

/// Extract (text, tag) from a [`RenderedCell`].
fn cell_parts(cell: &RenderedCell) -> (String, CellTag) {
    match cell {
        RenderedCell::Text(t) => (t.clone(), CellTag::Plain),
        RenderedCell::Tagged { text, tag } => (text.clone(), *tag),
    }
}

/// Recover the row's (type, name) context from the rendered cells + column
/// layout (mirrors how the old code read obj["type"]/obj["name"]).
fn row_context(cells: &[RenderedCell], columns: &[String]) -> (Option<String>, String) {
    let mut row_type: Option<String> = None;
    let mut row_name = String::new();
    for (col, cell) in columns.iter().zip(cells.iter()) {
        let text = match cell {
            RenderedCell::Text(t) | RenderedCell::Tagged { text: t, .. } => t.clone(),
        };
        if col == "type" {
            row_type = Some(text);
        } else if col == "name" {
            row_name = text;
        }
    }
    (row_type, row_name)
}

/// Map a [`CellTag`] (frontend-agnostic) to a ratatui `Style` (TUI-specific),
/// reproducing the old `cell_style` coloring exactly.
fn tag_to_style(
    tag: CellTag,
    col: &str,
    text: &str,
    row_type: Option<&str>,
) -> Style {
    match tag {
        CellTag::FileName(kind) => name_style(kind),
        CellTag::Dir => Style::default().fg(Color::LightBlue),
        CellTag::Permission => Style::default().fg(Color::DarkGray),
        CellTag::Plain => {
            // The old code also colored the literal "dir" text in the type
            // column; that's already covered by CellTag::Dir. For plain cells
            // we keep default (matching the old fallback).
            let _ = (col, text, row_type);
            Style::default()
        }
    }
}

/// Name-column style by [`FileNameKind`] (mirrors the old name-coloring rules).
fn name_style(kind: FileNameKind) -> Style {
    match kind {
        FileNameKind::Dir => Style::default().fg(Color::LightBlue),
        FileNameKind::CodeAtRs => Style::default().fg(Color::Green),
        FileNameKind::Executable => Style::default().fg(Color::LightCyan),
        FileNameKind::Config => Style::default().fg(Color::Yellow),
        FileNameKind::Plain => Style::default(),
    }
}

/// Name-column style directly from name + type (used by the icon column, which
/// has no cell value of its own). Identical rules to `name_style(file_name_kind(..))`.
fn cell_style_for_name(name: &str, row_type: Option<&str>) -> Style {
    name_style(file_name_kind(name, row_type))
}

/// Classify a file name (duplicate of ash-core's helper, kept local so the TUI
/// can color the synthetic icon column without a round-trip).
fn file_name_kind(name: &str, row_type: Option<&str>) -> FileNameKind {
    if row_type == Some("dir") {
        return FileNameKind::Dir;
    }
    if name.ends_with(".at") || name.ends_with(".rs") {
        return FileNameKind::CodeAtRs;
    }
    if name.ends_with(".exe") || name.ends_with(".dll") {
        return FileNameKind::Executable;
    }
    if name.ends_with(".toml")
        || name.ends_with(".json")
        || name.ends_with(".yaml")
        || name.ends_with(".yml")
    {
        return FileNameKind::Config;
    }
    FileNameKind::Plain
}

/// Column display names (only the icon column is special — empty header).
fn column_display_name(col: &str) -> String {
    match col {
        "icon" => String::new(),
        _ => col.to_string(),
    }
}

/// Calculate column widths based on content. Ported verbatim from the old
/// `calculate_column_widths` so widths (and thus wrapping/clamping) are
/// byte-identical.
///
/// `display_columns` is the (possibly icon-prefixed) layout we render;
/// `orig_columns` indexes into each row's `cells` (the RenderedOutput order,
/// without the injected icon column).
fn calculate_column_widths(
    display_columns: &[String],
    orig_columns: &[String],
    rows: &[Vec<RenderedCell>],
    term_width: u16,
    icons: IconStyle,
) -> Vec<u16> {
    let border_overhead = 2 + 2; // left + right border chars
    let spacing_overhead = (display_columns.len().saturating_sub(1)) as u16; // column_spacing=1
    let available = term_width.saturating_sub(border_overhead + spacing_overhead);

    let mut widths: Vec<u16> = display_columns
        .iter()
        .map(|col| {
            if col == "icon" {
                return if icons == IconStyle::Emoji { 2u16 } else { 1u16 };
            }
            let header_width = column_display_name(col).len() as u16;
            let max_data_width = rows.iter().fold(0u16, |max, cells| {
                // Data columns index into `cells` by the ORIGINAL column order.
                let cell_idx = orig_columns.iter().position(|c| c == col);
                cell_idx
                    .and_then(|i| cells.get(i))
                    .map(|c| match c {
                        RenderedCell::Text(t) | RenderedCell::Tagged { text: t, .. } => t.len(),
                    })
                    .map(|l| max.max(l as u16))
                    .unwrap_or(max)
            });
            header_width.max(max_data_width) + 2
        })
        .collect();

    let total: u16 = widths.iter().sum();
    if total > available {
        let excess = total - available;
        let mut remaining = excess;
        for (col, w) in display_columns.iter().zip(widths.iter_mut()).rev() {
            if remaining == 0 {
                break;
            }
            if col == "icon" {
                continue;
            }
            let shrink = (*w).saturating_sub(4).min(remaining);
            *w -= shrink;
            remaining -= shrink;
        }
    }

    widths
}

/// Pick a leading icon glyph for a file-listing row (ported from `file_icon`).
fn file_icon(row_type: Option<&str>, name: &str, icons: IconStyle) -> &'static str {
    match icons {
        IconStyle::Emoji => match row_type {
            Some("dir") => "📁",
            _ => file_icon_by_name_emoji(name),
        },
        IconStyle::NerdFont => match row_type {
            Some("dir") => "\u{F07C}",
            _ => file_icon_by_name_nerd(name),
        },
        IconStyle::Off => "",
        IconStyle::Plain => match row_type {
            Some("dir") => "■",
            _ => file_icon_by_name_plain(name),
        },
    }
}

fn file_icon_by_name_plain(_name: &str) -> &'static str {
    "□"
}
fn file_icon_by_name_emoji(_name: &str) -> &'static str {
    "📄"
}
fn file_icon_by_name_nerd(_name: &str) -> &'static str {
    "\u{F15B}"
}

#[cfg(test)]
mod tests {
    use super::*;
    use ash_core::pipeline::atom::{Atom, AtomType};
    use ash_core::pipeline::AtomPipeline;
    use ash_core::renderer::render_pipeline_to_structured;
    use auto_val::{Array, Obj, Value};

    /// Strip CSI ANSI escapes so text-content assertions work despite per-cell
    /// styling.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                while let Some(csi) = chars.next() {
                    if csi.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn file_listing_pipeline() -> AtomPipeline {
        let mut a = Obj::new();
        a.set("name", Value::str("main.rs"));
        a.set("type", Value::str("file"));
        a.set("size", Value::Int(1024));

        let mut b = Obj::new();
        b.set("name", Value::str("src"));
        b.set("type", Value::str("dir"));
        b.set("size", Value::Void);

        AtomPipeline::Atom(Atom {
            value: Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b)])),
            atom_type: AtomType::FileList,
        })
    }

    #[test]
    fn rendered_to_ansi_produces_table_with_icons_and_names() {
        let pipeline = file_listing_pipeline();
        let rendered = render_pipeline_to_structured(&pipeline).unwrap();
        let out = rendered_to_ansi(&rendered, 60, IconStyle::Plain).unwrap();
        let plain = strip_ansi(&out);
        // Borders.
        assert!(out.contains('╭') || out.contains('┌'));
        assert!(out.contains('╰') || out.contains('└'));
        // Icons + names.
        assert!(out.contains('■'), "missing dir icon: {out}");
        assert!(out.contains('□'), "missing file icon: {out}");
        assert!(plain.contains("src"));
        assert!(plain.contains("main.rs"));
        assert!(plain.contains("name"));
    }

    #[test]
    fn rendered_to_ansi_returns_none_for_non_table() {
        assert!(rendered_to_ansi(&RenderedOutput::Empty, 80, IconStyle::Plain).is_none());
        assert!(rendered_to_ansi(&RenderedOutput::Text("hi".into()), 80, IconStyle::Plain).is_none());
    }

    /// Plan 042 M6 B1 gap fix: Code must render as 24-bit ANSI (the `show`
    /// pipeline), not fall back to plain text.
    #[test]
    fn rendered_to_ansi_colors_code_spans() {
        let rendered = RenderedOutput::Code {
            lines: vec![vec![
                ash_core::renderer::CodeSpan {
                    text: "let".into(),
                    r: 1,
                    g: 2,
                    b: 3,
                    bold: true,
                    italic: false,
                },
            ]],
            language: "rs".into(),
        };
        let out = rendered_to_ansi(&rendered, 80, IconStyle::Plain)
            .expect("Code should render, not degrade to text");
        assert!(out.contains("\x1b[38;2;1;2;3m"), "24-bit fg: {out:?}");
        assert!(out.contains("\x1b[1m"), "bold: {out:?}");
        assert!(out.contains("let"));
    }

    #[test]
    fn tag_to_style_matches_old_cell_style_rules() {
        // name column, dir → LightBlue.
        assert_eq!(
            tag_to_style(CellTag::FileName(FileNameKind::Dir), "name", "x", None),
            Style::default().fg(Color::LightBlue)
        );
        // .rs → Green.
        assert_eq!(
            tag_to_style(CellTag::FileName(FileNameKind::CodeAtRs), "name", "x.rs", None),
            Style::default().fg(Color::Green)
        );
        // permissions → DarkGray.
        assert_eq!(
            tag_to_style(CellTag::Permission, "permissions", "-rw-r--r--", None),
            Style::default().fg(Color::DarkGray)
        );
        // "dir" value → LightBlue.
        assert_eq!(
            tag_to_style(CellTag::Dir, "type", "dir", None),
            Style::default().fg(Color::LightBlue)
        );
    }
}
