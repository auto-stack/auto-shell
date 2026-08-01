//! Render a frontend-agnostic [`RenderedOutput`] (from ash-core) into iced
//! widgets — the GUI's "last mile", mirroring the TUI's `rendered_to_ansi`.
//!
//! Plan 030 M2: only the `Table` variant is rendered (file lists / generic
//! tables). Other variants become a text paragraph. CellTag → iced styling is
//! the GUI's presentation choice (the TUI maps the same tags to ANSI colors).

use ash_core::renderer::{CellTag, FileNameKind, RenderedCell, RenderedOutput};
use iced::widget::{column, container, row, scrollable, text, Column, TextInput};
use iced::{Color, Element, Length};

/// The message type the rendered widgets produce. M2 has no interactivity yet
/// (clicking a filename is M4), so this is a placeholder.
#[derive(Debug, Clone)]
pub enum GuiMsg {
    /// The user typed in the command input.
    InputChanged(String),
    /// The user submitted the command (Enter).
    RunCommand,
}

/// Render a [`RenderedOutput`] into an iced [`Element`]. `input` is the current
/// command-line text (so the caller can keep the input box in view).
pub fn rendered_to_iced<'a>(rendered: &'a RenderedOutput, input: &'a str) -> Element<'a, GuiMsg> {
    let body: Element<GuiMsg> = match rendered {
        RenderedOutput::Table { columns, rows, .. } => table_view(columns, rows),
        RenderedOutput::Text(t) => text(t.as_str()).into(),
        RenderedOutput::Empty => text("(empty output)").into(),
        RenderedOutput::Record(pairs) => {
            let rec_rows: Vec<Element<GuiMsg>> = pairs
                .iter()
                .map(|(k, cell)| {
                    let v = match cell {
                        RenderedCell::Text(t) | RenderedCell::Tagged { text: t, .. } => t.clone(),
                    };
                    row![text(format!("{k}: ")), text(v)].spacing(4).into()
                })
                .collect();
            column(rec_rows).into()
        }
        RenderedOutput::Error { message, .. } => text(message.as_str())
            .style(|_theme| text::Style {
                color: Some(Color::from_rgb8(220, 80, 80)),
            })
            .into(),
    };

    // A persistent command input at the top + the rendered body below, in a
    // scrollable area.
    let input_box = TextInput::new("type a command (e.g. ls)", input)
        .on_input(GuiMsg::InputChanged)
        .on_submit(GuiMsg::RunCommand)
        .width(Length::Fill);

    let content = column![input_box, body].spacing(8);
    scrollable(container(content).padding(12)).into()
}

/// Render a table as a header row + data rows.
fn table_view<'a>(columns: &'a [String], rows: &'a [Vec<RenderedCell>]) -> Element<'a, GuiMsg> {
    // Header: gray column names.
    let header_cells: Vec<Element<GuiMsg>> = columns
        .iter()
        .map(|c| {
            text(c.as_str())
                .style(|_t| text::Style {
                    color: Some(Color::from_rgb8(120, 120, 120)),
                })
                .into()
        })
        .collect();
    let header: Element<GuiMsg> = row(header_cells).spacing(16).into();

    // Data rows.
    let data_rows: Vec<Element<GuiMsg>> = rows
        .iter()
        .map(|cells| {
            let cell_widgets: Vec<Element<GuiMsg>> = cells.iter().map(cell_widget).collect();
            row(cell_widgets).spacing(16).into()
        })
        .collect();

    let mut table = Column::with_children(vec![header]);
    for r in data_rows {
        table = table.push(r);
    }
    table.spacing(4).into()
}

/// Render one cell, applying tag-based styling (color by file kind, like the TUI).
fn cell_widget(cell: &RenderedCell) -> Element<'_, GuiMsg> {
    let (s, tag) = match cell {
        RenderedCell::Text(t) => (t.clone(), CellTag::Plain),
        RenderedCell::Tagged { text, tag } => (text.clone(), *tag),
    };
    let color = tag_color(tag);
    // Owned String: iced's `text` takes impl Into<String>, so this is 'static.
    text(s)
        .style(move |_theme| text::Style { color: Some(color) })
        .into()
}

/// Map a [`CellTag`] to an iced [`Color`] — the GUI's presentation choice for
/// the same semantic tags the TUI colors via ANSI.
fn tag_color(tag: CellTag) -> Color {
    match tag {
        CellTag::FileName(kind) => match kind {
            FileNameKind::Dir => Color::from_rgb8(96, 160, 255),   // blue
            FileNameKind::CodeAtRs => Color::from_rgb8(96, 200, 96), // green
            FileNameKind::Executable => Color::from_rgb8(96, 200, 220), // cyan
            FileNameKind::Config => Color::from_rgb8(220, 200, 96), // yellow
            FileNameKind::Plain => Color::WHITE,
        },
        CellTag::Dir => Color::from_rgb8(96, 160, 255),   // blue
        CellTag::Permission => Color::from_rgb8(120, 120, 120), // dim gray
        CellTag::Plain => Color::WHITE,
    }
}
