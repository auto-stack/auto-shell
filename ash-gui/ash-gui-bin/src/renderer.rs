//! Render the GUI: a scrolling list of Blocks + a command input with completion.
//!
//! Plan 030 M3: the main view is `block_list_view` — each Block renders a
//! status-colored header (command + status icon) and its `RenderedOutput` body.
//! M2's `rendered_to_iced` is reused for each Block's body.

use ash_core::renderer::{CellTag, FileNameKind, RenderedCell, RenderedOutput};
use iced::widget::{
    column, container, mouse_area, row, scrollable, space, text, text_input, Column, TextInput,
};
use iced::{Color, Element, Length};

use crate::block::{Block, BlockStatus};

/// The messages the GUI widgets produce.
#[derive(Debug, Clone)]
pub enum GuiMsg {
    InputChanged(String),
    RunCommand,
    HistoryPrev,
    HistoryNext,
    /// User clicked a completion suggestion.
    PickCompletion(String),
    /// User clicked a file name cell (open it).
    OpenPath(String),
}

/// The full GUI view: a scrollable list of Blocks + an input box (with
/// completion suggestions) pinned at the bottom.
pub fn block_list_view<'a>(
    blocks: &'a [Block],
    input: &'a str,
    suggestions: &'a [String],
) -> Element<'a, GuiMsg> {
    // Build the block list (newest at the bottom).
    let block_widgets: Vec<Element<GuiMsg>> = if blocks.is_empty() {
        vec![text("(no commands yet — type one below, e.g. ls)").into()]
    } else {
        blocks.iter().map(block_view).collect()
    };
    let list = Column::with_children(block_widgets).spacing(6);

    // Completion suggestions (if any), just above the input.
    let mut footer_children: Vec<Element<GuiMsg>> = Vec::new();
    // Input box: Enter runs.
    let input_box = TextInput::new("type a command (e.g. ls)", input)
        .on_input(GuiMsg::InputChanged)
        .on_submit(GuiMsg::RunCommand)
        .width(Length::Fill);
    footer_children.push(input_box.into());
    if !suggestions.is_empty() {
        let sugg_row = row(suggestions.iter().map(|s| {
            text(s.as_str())
                .style(|_t| text::Style {
                    color: Some(Color::from_rgb8(140, 180, 255)),
                })
                .into()
        }))
        .spacing(12);
        footer_children.push(sugg_row.into());
    }
    let footer = Column::with_children(footer_children).spacing(4);

    let content = column![scrollable(list).height(Length::Fill), footer].spacing(8);
    container(content).padding(12).into()
}

/// Render one Block: a status-colored header line + the output body.
fn block_view<'a>(block: &'a Block) -> Element<'a, GuiMsg> {
    let (icon, icon_color) = status_icon(&block.status);
    let header = row![
        text(format!("{} {}", icon, block.command)),
        space::horizontal(),
        text(block.status_label())
            .style(move |_t| text::Style { color: Some(icon_color) }),
    ]
    .spacing(8);

    let body = rendered_to_iced(&block.output);
    column![header, container(body).padding(iced::Padding {
        top: 4.0,
        right: 0.0,
        bottom: 0.0,
        left: 12.0,
    })]
    .spacing(2)
    .into()
}

/// Status icon + color for a block header.
fn status_icon(status: &BlockStatus) -> (&'static str, Color) {
    match status {
        BlockStatus::Success => ("╭", Color::from_rgb8(120, 200, 120)),
        BlockStatus::Failed(_) => ("╭", Color::from_rgb8(220, 100, 100)),
        BlockStatus::Running => ("╭", Color::from_rgb8(200, 180, 80)),
    }
}

// ── RenderedOutput → iced widget (carried over from M2, with M4 hooks) ──────

/// Render a [`RenderedOutput`] into an iced [`Element`].
pub fn rendered_to_iced(rendered: &RenderedOutput) -> Element<'_, GuiMsg> {
    match rendered {
        RenderedOutput::Table { columns, rows, .. } => table_view(columns, rows),
        RenderedOutput::Text(t) => text(t.as_str()).into(),
        RenderedOutput::Empty => text("").into(),
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
    }
}

/// Render a table as a header row + data rows.
fn table_view<'a>(columns: &'a [String], rows: &'a [Vec<RenderedCell>]) -> Element<'a, GuiMsg> {
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

/// Render one cell, applying tag-based styling. FileName cells are clickable
/// (M4 CellTag interaction).
fn cell_widget(cell: &RenderedCell) -> Element<'_, GuiMsg> {
    let (s, tag) = match cell {
        RenderedCell::Text(t) => (t.clone(), CellTag::Plain),
        RenderedCell::Tagged { text, tag } => (text.clone(), *tag),
    };
    let color = tag_color(tag);
    let tw = text(s.clone()).style(move |_theme| text::Style { color: Some(color) });

    // Make FileName / Dir cells clickable to open the file (M4).
    if matches!(tag, CellTag::FileName(_) | CellTag::Dir) {
        mouse_area(tw)
            .on_press(GuiMsg::OpenPath(s))
            .into()
    } else {
        tw.into()
    }
}

/// Map a [`CellTag`] to an iced [`Color`].
fn tag_color(tag: CellTag) -> Color {
    match tag {
        CellTag::FileName(kind) => match kind {
            FileNameKind::Dir => Color::from_rgb8(96, 160, 255),
            FileNameKind::CodeAtRs => Color::from_rgb8(96, 200, 96),
            FileNameKind::Executable => Color::from_rgb8(96, 200, 220),
            FileNameKind::Config => Color::from_rgb8(220, 200, 96),
            FileNameKind::Plain => Color::WHITE,
        },
        CellTag::Dir => Color::from_rgb8(96, 160, 255),
        CellTag::Permission => Color::from_rgb8(120, 120, 120),
        CellTag::Plain => Color::WHITE,
    }
}
