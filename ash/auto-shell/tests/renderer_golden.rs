//! Plan 030 M1 — golden comparison: the new Renderer-trait path
//! (`render_pipeline_to_structured` + `rendered_to_ansi`) must produce
//! **byte-identical** output to the old `render_table_with` path.
//!
//! This is the M1 "visual no-change" guarantee. If any of these break, the
//! refactor changed what users see for `ls`/`ps`/table output.
//!
//! We compare the raw ANSI strings directly (not stripped) so styling escapes
//! must match too.

use ash_core::pipeline::atom::{Atom, AtomType};
use ash_core::pipeline::AtomPipeline;
use ash_core::renderer::render_pipeline_to_structured;
use auto_shell::config::IconStyle;
use auto_shell::frontend::renderer::{render_table_with, rendered_to_ansi};
use auto_val::{Array, Obj, Value};

/// Build an AtomPipeline wrapping a Value with the given AtomType.
fn pipeline(value: Value, atom_type: AtomType) -> AtomPipeline {
    AtomPipeline::Atom(Atom { value, atom_type })
}

/// Assert the old and new paths agree for `value` at a given width + icon style.
fn assert_paths_match(value: &Value, atom_type: AtomType, width: u16, icons: IconStyle, label: &str) {
    let old = render_table_with(value, width, icons);
    let new = {
        let pl = pipeline(value.clone(), atom_type);
        match render_pipeline_to_structured(&pl) {
            Some(ro) => rendered_to_ansi(&ro, width, icons),
            None => None,
        }
    };
    if old != new {
        panic!(
            "old vs new render paths disagree [{label}] at width {width} icons {icons:?}:\n\
             --- OLD ---\n{}\n\
             --- NEW ---\n{}",
            old.unwrap_or_default(),
            new.unwrap_or_default()
        );
    }
}

#[test]
fn golden_file_listing_plain_icons() {
    let mut a = Obj::new();
    a.set("name", Value::str("main.rs"));
    a.set("type", Value::str("file"));
    a.set("size", Value::Int(1024));

    let mut b = Obj::new();
    b.set("name", Value::str("src"));
    b.set("type", Value::str("dir"));
    b.set("size", Value::Void);

    let mut c = Obj::new();
    c.set("name", Value::str("config.toml"));
    c.set("type", Value::str("file"));
    c.set("size", Value::Int(200));

    let value = Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b), Value::Obj(c)]));
    for width in [40u16, 60, 80, 120] {
        assert_paths_match(&value, AtomType::FileList, width, IconStyle::Plain, "file-listing");
    }
}

#[test]
fn golden_file_listing_emoji_and_nerd_icons() {
    let mut a = Obj::new();
    a.set("name", Value::str("app.exe"));
    a.set("type", Value::str("file"));

    let mut b = Obj::new();
    b.set("name", Value::str("lib.dll"));
    b.set("type", Value::str("file"));

    let mut c = Obj::new();
    c.set("name", Value::str("bin"));
    c.set("type", Value::str("dir"));

    let value = Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b), Value::Obj(c)]));
    for icons in [IconStyle::Emoji, IconStyle::NerdFont] {
        assert_paths_match(&value, AtomType::FileList, 80, icons, "icons");
    }
}

#[test]
fn golden_long_format_with_permissions() {
    let mut a = Obj::new();
    a.set("name", Value::str("main.rs"));
    a.set("type", Value::str("file"));
    a.set("permissions", Value::str("-rw-r--r--"));
    a.set("owner", Value::str("alice"));
    a.set("size", Value::Int(1024));
    a.set("modified", Value::str("2026-01-01"));

    let mut b = Obj::new();
    b.set("name", Value::str("src"));
    b.set("type", Value::str("dir"));
    b.set("permissions", Value::str("drwxr-xr-x"));
    b.set("owner", Value::str("bob"));
    b.set("size", Value::Void);
    b.set("modified", Value::str("2026-02-02"));

    let value = Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b)]));
    for width in [60u16, 100, 140] {
        assert_paths_match(&value, AtomType::FileList, width, IconStyle::Plain, "long-format");
    }
}

#[test]
fn golden_non_file_listing_table() {
    // A table without name+type columns — not a file listing, no icon column.
    let mut a = Obj::new();
    a.set("widget", Value::str("button"));
    a.set("count", Value::Int(7));

    let mut b = Obj::new();
    b.set("widget", Value::str("slider"));
    b.set("count", Value::Int(3));

    let value = Value::Array(Array::from_vec(vec![Value::Obj(a), Value::Obj(b)]));
    assert_paths_match(&value, AtomType::Table, 60, IconStyle::Plain, "non-file-table");
    // Icon style Off is a no-op here anyway, but verify both icon paths agree.
    assert_paths_match(&value, AtomType::Table, 60, IconStyle::Off, "non-file-table-off");
}

#[test]
fn golden_off_icons_skip_icon_column() {
    let mut a = Obj::new();
    a.set("name", Value::str("x.rs"));
    a.set("type", Value::str("file"));
    let value = Value::Array(Array::from_vec(vec![Value::Obj(a)]));
    assert_paths_match(&value, AtomType::FileList, 60, IconStyle::Off, "icons-off");
}

#[test]
fn golden_non_table_inputs_both_none() {
    // Non-array / empty / mixed — both paths return None.
    let cases = [
        (Value::str("hello"), AtomType::FileList),
        (Value::Array(Array::new()), AtomType::FileList),
        (
            Value::Array(Array::from_vec(vec![Value::Int(1), Value::str("x")])),
            AtomType::FileList,
        ),
    ];
    for (value, atom_type) in cases {
        let old = render_table_with(&value, 80, IconStyle::Plain);
        let new = render_pipeline_to_structured(&pipeline(value, atom_type))
            .and_then(|ro| rendered_to_ansi(&ro, 80, IconStyle::Plain));
        assert_eq!(old, None, "old path should be None");
        assert_eq!(new, None, "new path should be None");
    }
}
