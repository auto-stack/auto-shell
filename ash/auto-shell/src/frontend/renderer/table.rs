//! Ratatui Table rendering for structured shell output — public entry points.
//!
//! Plan 030 M1: the table-rendering logic now lives in two shared places:
//! - `ash_core::renderer` — the frontend-agnostic `RenderedOutput` + the
//!   pure-logic `render_pipeline_to_structured` (columns / cell values / tags)
//! - `frontend::renderer::tui` — the TUI presentation (`rendered_to_ansi`:
//!   widths / borders / zebra / icon column / tag→style mapping)
//!
//! This file keeps the original `render_table` / `render_table_with` entry
//! points as thin delegators for backward compatibility (callers + tests use
//! these signatures). The golden test (`tests/renderer_golden.rs`) proves the
//! delegated path produces byte-identical output to the old in-place code.

use auto_val::Value;

use crate::config::IconStyle;

/// Render a `Value::Array` (of objects) as a bordered ANSI table string.
///
/// Uses the default icon style (`Plain`). Returns `None` if the value is not a
/// table-compatible array.
pub fn render_table(value: &Value, term_width: u16) -> Option<String> {
    render_table_with(value, term_width, IconStyle::default())
}

/// Render a table with a specific [`IconStyle`] for file-listing rows.
///
/// Delegates to the shared Renderer-trait path (`render_pipeline_to_structured`
/// + `rendered_to_ansi`).
pub fn render_table_with(
    value: &Value,
    term_width: u16,
    icons: IconStyle,
) -> Option<String> {
    use ash_core::pipeline::atom::{Atom, AtomType};
    use ash_core::pipeline::AtomPipeline;
    use ash_core::renderer::render_pipeline_to_structured;
    let pipeline = AtomPipeline::Atom(Atom {
        value: value.clone(),
        atom_type: AtomType::Table,
    });
    render_pipeline_to_structured(&pipeline)
        .and_then(|ro| super::tui::rendered_to_ansi(&ro, term_width, icons))
}
