//! Frontend-agnostic rendering intermediate representation (Plan 030 M1).
//!
//! This module turns a structured [`AtomPipeline`] into a [`RenderedOutput`] —
//! a pure-data description of *what* to show (columns, cells, semantic tags),
//! with **no** knowledge of *how* to draw it. Two frontends then render the
//! same `RenderedOutput`:
//!
//! - the TUI (auto-shell) converts it to an ANSI string via ratatui + nu-ansi-term
//! - the GUI (ash-gui-bin, M2) converts it to iced widgets
//!
//! Keeping this in `ash-core` (which has no terminal dependencies) means the
//! "which AtomType becomes which kind of view" logic is written once and shared.
//!
//! ## M1 scope
//! Only the `Table` variant is produced (file lists, process lists, generic
//! tables). `Record`/`Error` variants exist for forward-compat but are not yet
//! routed. Non-structured atoms stay text (the caller's fallback).

use crate::pipeline::{AtomPipeline, AtomType};

/// A command output's visual description — frontend-agnostic.
///
/// A TUI renderer turns this into an ANSI string; a GUI renderer turns it into
/// a widget tree. Both consume the *same* value.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RenderedOutput {
    /// A structured table (file list / process list / generic rows-and-columns).
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<RenderedCell>>,
        atom_type: AtomType,
    },
    /// A single record (key → cell). Plan 030 M4: now routed for single-Obj
    /// atoms (stat / date / version / sys mem), carrying atom_type so frontends
    /// can pick a specialized widget (e.g. a gauge for MemoryInfo).
    Record {
        fields: Vec<(String, RenderedCell)>,
        atom_type: AtomType,
    },
    /// Plain text.
    Text(String),
    /// Empty output.
    Empty,
    /// An error. (Forward-compat; M1 does not route here.)
    Error {
        message: String,
        kind: RenderErrorKind,
    },
}

/// One cell of a rendered table. `Tagged` carries a semantic [`CellTag`] so a
/// frontend can style/interact with it (e.g. make a filename clickable).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RenderedCell {
    /// A plain text cell with no special semantics.
    Text(String),
    /// A cell whose meaning is known (filename / path / …) — frontends may
    /// color or interact with it based on the tag.
    Tagged { text: String, tag: CellTag },
}

/// Semantic tag for a [`RenderedCell`]. Frontend-agnostic — it carries *what*
/// the cell is, not *what color* to draw it. The mapping tag → color/widget is
/// each frontend's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CellTag {
    /// A file/directory name. The kind carries enough to reproduce the existing
    /// per-extension coloring without embedding colors here.
    FileName(FileNameKind),
    /// The literal directory marker (the "dir" value in a `type` column).
    Dir,
    /// A permissions string (e.g. `rwxr-xr-x`) — typically dimmed.
    Permission,
    /// Anything without special semantics.
    Plain,
}

/// Sub-kind of a [`CellTag::FileName`], mirroring the existing per-extension
/// coloring in `cell_style` (auto-shell's table renderer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum FileNameKind {
    /// A directory name.
    Dir,
    /// `.at` / `.rs` source files.
    CodeAtRs,
    /// `.exe` / `.dll` executables.
    Executable,
    /// `.toml` / `.json` / `.yaml` config files.
    Config,
    /// Any other file.
    Plain,
}

/// What kind of error a [`RenderedOutput::Error`] represents. (Forward-compat.)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RenderErrorKind {
    NotFound,
    PermissionDenied,
    NonzeroExit,
    Other,
}

/// A renderer turns a pipeline into a [`RenderedOutput`]. TUI and GUI each
/// provide one implementation; both share [`render_pipeline_to_structured`].
pub trait Renderer: Send {
    /// Describe `pipeline` as a frontend-agnostic [`RenderedOutput`].
    fn render(&self, pipeline: &AtomPipeline) -> RenderedOutput;
    /// The width the renderer would prefer (e.g. terminal width for the TUI).
    fn width_hint(&self) -> u16;
}

/// Turn a structured [`AtomPipeline`] into a [`RenderedOutput::Table`], or
/// `None` when the pipeline isn't a table-shaped Atom (non-Atom variants,
/// non-structured atom types, non-array values, empty/mixed arrays).
///
/// This is the **pure-logic** half of rendering: it collects columns (with the
/// canonical sort priority), formats cell values, and assigns [`CellTag`]s.
/// It does **not** compute column widths, borders, zebra striping, or the icon
/// column — those are presentation choices left to each frontend (the TUI adds
/// them in `rendered_to_ansi`; the GUI may choose differently).
pub fn render_pipeline_to_structured(pipeline: &AtomPipeline) -> Option<RenderedOutput> {
    let atom = match pipeline {
        AtomPipeline::Atom(a) => a,
        _ => return None,
    };
    if !atom.is_structured() {
        return None;
    }

    // Plan 030 M4: a single object → RenderedOutput::Record (stat/date/version/
    // sys mem). Carries atom_type so frontends can specialize (e.g. MemoryInfo
    // gauge).
    if let auto_val::Value::Obj(obj) = &atom.value {
        let fields: Vec<(String, RenderedCell)> = obj
            .iter()
            .map(|(k, v)| {
                let text = format_cell_value(v);
                (k.to_string(), RenderedCell::Tagged { text, tag: CellTag::Plain })
            })
            .collect();
        if !fields.is_empty() {
            return Some(RenderedOutput::Record {
                fields,
                atom_type: atom.atom_type,
            });
        }
        return None;
    }

    let arr = match &atom.value {
        auto_val::Value::Array(a) => a,
        _ => return None,
    };
    if !arr.iter().all(|v| matches!(v, auto_val::Value::Obj(_))) {
        return None;
    }

    let columns = collect_columns(arr);
    if columns.is_empty() {
        return None;
    }

    // Build rows: each cell gets a value string + a semantic tag derived from
    // its column and the row's `type` context (dirs/code/exec/config coloring).
    let rows: Vec<Vec<RenderedCell>> = arr
        .iter()
        .map(|item| {
            let obj = match item {
                auto_val::Value::Obj(o) => o,
                _ => return Vec::new(),
            };
            let row_type = match obj.get("type") {
                Some(auto_val::Value::Str(s)) => Some(s.to_string()),
                _ => None,
            };
            columns
                .iter()
                .map(|col| {
                    let text = match obj.get(col.as_str()) {
                        Some(v) => format_cell_value(&v),
                        None => String::new(),
                    };
                    // The name column is classified by the row's name (which
                    // equals the cell text for that column); other columns tag
                    // by their own text/value.
                    let tag = cell_tag(&text, col, row_type.as_deref());
                    RenderedCell::Tagged { text, tag }
                })
                .collect()
        })
        .collect();

    Some(RenderedOutput::Table {
        columns,
        rows,
        atom_type: atom.atom_type,
    })
}

/// Decide a cell's [`CellTag`] from its column, text, and the row's type/name.
///
/// Mirrors the classification in auto-shell's `cell_style`:
/// - the `name` column → `FileName(kind)` (dir / code / exec / config / plain)
/// - the literal "dir" text → `Dir`
/// - the `permissions` column → `Permission`
/// - anything else → `Plain`
fn cell_tag(text: &str, col: &str, row_type: Option<&str>) -> CellTag {
    if col == "name" {
        return CellTag::FileName(file_name_kind(text, row_type));
    }
    if col == "permissions" {
        return CellTag::Permission;
    }
    if text == "dir" {
        return CellTag::Dir;
    }
    CellTag::Plain
}

/// Classify a file name into a [`FileNameKind`] (mirrors the name-column
/// coloring rules in `cell_style`).
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

/// Collect all unique column keys from an array of objects, sorted by the
/// canonical priority (ported from auto-shell's `collect_columns` so column
/// order — and thus the visual layout — stays identical).
pub fn collect_columns(arr: &auto_val::Array) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for item in arr.iter() {
        if let auto_val::Value::Obj(obj) = item {
            for (key, _) in obj.iter() {
                let key_str = key.to_string();
                if !columns.contains(&key_str) {
                    columns.push(key_str);
                }
            }
        }
    }

    columns.sort_by(|a, b| {
        let has_long_format =
            a == "permissions" || a == "owner" || b == "permissions" || b == "owner";

        if has_long_format {
            // Name first (the visual focus), then the metadata columns.
            let priority = ["name", "permissions", "owner", "size", "modified"];
            let a_pos = priority.iter().position(|&p| p == a).unwrap_or(usize::MAX);
            let b_pos = priority.iter().position(|&p| p == b).unwrap_or(usize::MAX);
            a_pos.cmp(&b_pos).then_with(|| a.cmp(b))
        } else {
            let priority = ["name", "type", "size", "modified"];
            let a_pos = priority.iter().position(|&p| p == a).unwrap_or(usize::MAX);
            let b_pos = priority.iter().position(|&p| p == b).unwrap_or(usize::MAX);
            a_pos.cmp(&b_pos).then_with(|| a.cmp(b))
        }
    });

    columns
}

/// Format a `Value` for cell display (no extra quotes for strings). Ported from
/// auto-shell's `format_cell_value` so cell text is byte-identical.
pub fn format_cell_value(val: &auto_val::Value) -> String {
    use auto_val::Value;
    match val {
        Value::Str(s) => s.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Nil => "nil".to_string(),
        Value::Null => "null".to_string(),
        Value::Void => "void".to_string(),
        _ => val.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auto_val::{Array, Obj, Value};

    fn file_listing() -> Value {
        let mut a = Obj::new();
        a.set("name", Value::str("main.rs"));
        a.set("type", Value::str("file"));
        a.set("size", Value::Int(1024));

        let mut b = Obj::new();
        b.set("name", Value::str("src"));
        b.set("type", Value::str("dir"));
        b.set("size", Value::Void);

        Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b)]))
    }

    fn atom_of(value: Value, atom_type: AtomType) -> AtomPipeline {
        AtomPipeline::Atom(crate::pipeline::atom::Atom { value, atom_type })
    }

    #[test]
    fn collect_columns_sorts_by_priority() {
        // Insert keys out of order; expect name, type, size.
        let mut o = Obj::new();
        o.set("size", Value::Int(1));
        o.set("type", Value::str("file"));
        o.set("name", Value::str("x"));
        let arr = Array::from_vec(vec![Value::Obj(o)]);
        let cols = collect_columns(&arr);
        assert_eq!(cols, vec!["name", "type", "size"]);
    }

    #[test]
    fn collect_columns_long_format_priority() {
        // With permissions/owner present, the long-format priority applies.
        let mut o = Obj::new();
        o.set("permissions", Value::str("-rw-rw-rw-"));
        o.set("owner", Value::str("me"));
        o.set("size", Value::Int(1));
        o.set("name", Value::str("x"));
        o.set("modified", Value::str("2026-01-01"));
        let arr = Array::from_vec(vec![Value::Obj(o)]);
        let cols = collect_columns(&arr);
        assert_eq!(
            cols,
            vec!["name", "permissions", "owner", "size", "modified"]
        );
    }

    #[test]
    fn format_cell_value_basic_types() {
        assert_eq!(format_cell_value(&Value::str("hi")), "hi");
        assert_eq!(format_cell_value(&Value::Int(42)), "42");
        assert_eq!(format_cell_value(&Value::Bool(true)), "true");
        assert_eq!(format_cell_value(&Value::Void), "void");
    }

    #[test]
    fn render_structured_file_listing() {
        let pipeline = atom_of(file_listing(), AtomType::FileList);
        let ro = render_pipeline_to_structured(&pipeline).expect("file list is structured");
        let (columns, rows) = match ro {
            RenderedOutput::Table { columns, rows, .. } => (columns, rows),
            other => panic!("expected Table, got {other:?}"),
        };
        assert_eq!(columns, vec!["name", "type", "size"]);
        assert_eq!(rows.len(), 2);
        // main.rs → CodeAtRs; src (dir) → Dir.
        let main_name = &rows[0][0];
        match main_name {
            RenderedCell::Tagged {
                tag: CellTag::FileName(FileNameKind::CodeAtRs),
                text,
            } => assert_eq!(text, "main.rs"),
            other => panic!("main.rs name cell wrong: {other:?}"),
        }
        let src_name = &rows[1][0];
        match src_name {
            RenderedCell::Tagged {
                tag: CellTag::FileName(FileNameKind::Dir),
                text,
            } => assert_eq!(text, "src"),
            other => panic!("src name cell wrong: {other:?}"),
        }
        // The "dir" value in the type column → Dir tag.
        let src_type = &rows[1][1];
        match src_type {
            RenderedCell::Tagged {
                tag: CellTag::Dir,
                text,
            } => assert_eq!(text, "dir"),
            other => panic!("src type cell wrong: {other:?}"),
        }
    }

    #[test]
    fn render_returns_none_for_non_atom_pipeline() {
        assert!(render_pipeline_to_structured(&AtomPipeline::Text("hi".into())).is_none());
        assert!(render_pipeline_to_structured(&AtomPipeline::Empty).is_none());
    }

    #[test]
    fn render_returns_none_for_unstructured_atom() {
        let pipeline = atom_of(Value::str("plain"), AtomType::Text);
        assert!(render_pipeline_to_structured(&pipeline).is_none());
    }

    #[test]
    fn render_single_obj_becomes_record_with_atom_type() {
        // Plan 030 M4: a single object (stat / date / sys mem) → Record, and the
        // atom_type is carried so frontends can specialize (e.g. MemoryInfo gauge).
        let mut o = Obj::new();
        o.set("total", Value::Int(8192));
        o.set("usage_percent", Value::Int(72));
        let pipeline = atom_of(Value::Obj(o), AtomType::MemoryInfo);
        let ro = render_pipeline_to_structured(&pipeline).expect("single obj is a record");
        match ro {
            RenderedOutput::Record { fields, atom_type } => {
                assert_eq!(atom_type, AtomType::MemoryInfo);
                assert_eq!(fields.len(), 2);
                assert!(fields.iter().any(|(k, _)| k == "usage_percent"));
            }
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn render_empty_obj_returns_none() {
        let pipeline = atom_of(Value::Obj(Obj::new()), AtomType::Record);
        assert!(render_pipeline_to_structured(&pipeline).is_none());
    }

    #[test]
    fn render_returns_none_for_non_array_value() {
        let pipeline = atom_of(Value::str("not an array"), AtomType::FileList);
        assert!(render_pipeline_to_structured(&pipeline).is_none());
    }

    #[test]
    fn render_returns_none_for_empty_or_mixed_array() {
        let empty = atom_of(Value::Array(Array::new()), AtomType::FileList);
        assert!(render_pipeline_to_structured(&empty).is_none());

        let mixed = atom_of(
            Value::Array(Array::from_vec(vec![Value::Int(1), Value::str("x")])),
            AtomType::FileList,
        );
        assert!(render_pipeline_to_structured(&mixed).is_none());
    }

    #[test]
    fn file_name_kind_classification() {
        assert_eq!(file_name_kind("x", Some("dir")), FileNameKind::Dir);
        assert_eq!(file_name_kind("a.rs", None), FileNameKind::CodeAtRs);
        assert_eq!(file_name_kind("a.at", None), FileNameKind::CodeAtRs);
        assert_eq!(file_name_kind("a.exe", None), FileNameKind::Executable);
        assert_eq!(file_name_kind("a.toml", None), FileNameKind::Config);
        assert_eq!(file_name_kind("a.yaml", None), FileNameKind::Config);
        assert_eq!(file_name_kind("readme", None), FileNameKind::Plain);
    }

    #[test]
    fn permissions_column_gets_permission_tag() {
        let mut o = Obj::new();
        o.set("name", Value::str("x"));
        o.set("type", Value::str("file"));
        o.set("permissions", Value::str("-rw-r--r--"));
        let pipeline = atom_of(
            Value::Array(Array::from_vec(vec![Value::Obj(o)])),
            AtomType::FileList,
        );
        let ro = render_pipeline_to_structured(&pipeline).unwrap();
        let (columns, rows) = match ro {
            RenderedOutput::Table { columns, rows, .. } => (columns, rows),
            _ => unreachable!(),
        };
        // Find the permissions cell by column index (order depends on sort).
        let perm_idx = columns
            .iter()
            .position(|c| c == "permissions")
            .expect("permissions column present");
        let perm = &rows[0][perm_idx];
        match perm {
            RenderedCell::Tagged {
                tag: CellTag::Permission,
                text,
            } => assert_eq!(text, "-rw-r--r--"),
            other => panic!("permissions cell wrong: {other:?}"),
        }
    }
}
