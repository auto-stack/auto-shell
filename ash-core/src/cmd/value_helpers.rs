//! Helper methods for working with Auto values in shell commands
//!
//! This module provides convenience functions for converting between
//! shell data and Auto's Value types.

use auto_val::{Value, Obj, Array, AutoStr};
use crate::pipeline::atom::AtomType;

/// Build a file entry object (for ls command output)
pub fn build_file_entry(
    name: impl Into<AutoStr>,
    file_type: impl Into<AutoStr>,
    size: Option<i64>,
    modified: Option<String>,
    permissions: Option<String>,
) -> Value {
    let mut obj = Obj::new();
    obj.set("name", Value::str(name));
    obj.set("type", Value::str(file_type));

    if let Some(s) = size {
        obj.set("size", Value::Int(s as i32));
    }

    if let Some(m) = modified {
        obj.set("modified", Value::str(m));
    }

    if let Some(p) = permissions {
        obj.set("permissions", Value::str(p));
    }

    Value::Obj(obj)
}

/// Format a Value for display
///
/// This converts structured Auto values to human-readable text output.
/// - Arrays become tables (when possible)
/// - Objects become key-value lists
/// - Primitives use their Display implementation
pub fn format_value_for_display(val: &Value) -> String {
    match val {
        Value::Array(arr) => {
            // Try to format as table if all elements are objects
            format_array_as_table(arr)
        }
        Value::Obj(obj) => {
            // Format as key-value list
            format_obj_as_record(obj)
        }
        _ => val.to_string(),
    }
}

/// Format an Array as a table (if all elements are objects)
fn format_array_as_table(arr: &Array) -> String {
    // Check if all elements are objects
    if arr.values.is_empty() {
        return String::new();
    }

    let all_objects = arr.iter().all(|v| matches!(v, Value::Obj(_)));
    if !all_objects {
        // Not all objects, use default string representation
        return arr.to_string();
    }

    // Collect all object keys to determine columns
    let mut columns: Vec<String> = Vec::new();
    for item in arr.iter() {
        if let Value::Obj(obj) = item {
            for (key, _) in obj.iter() {
                let key_str = key.to_string();
                if !columns.contains(&key_str) {
                    columns.push(key_str);
                }
            }
        }
    }

    // If no columns, return empty
    if columns.is_empty() {
        return String::new();
    }

    // Sort columns with common preferences (ls -l format for long, name first for short)
    columns.sort_by(|a, b| {
        // For long format (has permissions), use: permissions, owner, size, modified, name
        // For short format, use: name, type, size, modified
        let has_long_format = a == "permissions" || a == "owner" || b == "permissions" || b == "owner";

        if has_long_format {
            // Long format: permissions, owner, size, modified, name
            let long_priority = ["permissions", "owner", "size", "modified", "name"];
            let a_pos = long_priority.iter().position(|&p| p == a).unwrap_or(usize::MAX);
            let b_pos = long_priority.iter().position(|&p| p == b).unwrap_or(usize::MAX);
            a_pos.cmp(&b_pos).then_with(|| a.cmp(b))
        } else {
            // Short format: name, type, size, modified
            let short_priority = ["name", "type", "size", "modified"];
            let a_pos = short_priority.iter().position(|&p| p == a).unwrap_or(usize::MAX);
            let b_pos = short_priority.iter().position(|&p| p == b).unwrap_or(usize::MAX);
            a_pos.cmp(&b_pos).then_with(|| a.cmp(b))
        }
    });

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();

    for item in arr.iter() {
        if let Value::Obj(obj) = item {
            for (i, col) in columns.iter().enumerate() {
                if let Some(value) = obj.get(col.as_str()) {
                    let value_str = format_value_for_table(&value);
                    widths[i] = widths[i].max(value_str.len());
                }
            }
        }
    }

    // Build table rows
    let mut result = String::new();

    // Header row with capitalized column names
    let header: Vec<String> = columns.iter().enumerate()
        .map(|(i, col)| {
            let title = match col.as_str() {
                "permissions" => "Permissions",
                "owner" => "Owner",
                "size" => "Size",
                "modified" => "Modified",
                "name" => "Name",
                "type" => "Type",
                _ => &col,
            };
            format!("{:<width$}", title, width = widths[i])
        })
        .collect();
    result.push_str(&header.join("  "));
    result.push('\n');

    // Data rows
    for item in arr.iter() {
        if let Value::Obj(obj) = item {
            let row: Vec<String> = columns.iter().enumerate()
                .map(|(i, col)| {
                    if let Some(value) = obj.get(col.as_str()) {
                        let value_str = format_value_for_table(&value);
                        format!("{:<width$}", value_str, width = widths[i])
                    } else {
                        format!("{:<width$}", "", width = widths[i])
                    }
                })
                .collect();
            result.push_str(&row.join("  "));
            result.push('\n');
        }
    }

    result.trim_end().to_string()
}

/// Format a Value for table cell display (without extra quotes for strings)
fn format_value_for_table(val: &Value) -> String {
    match val {
        Value::Str(s) => s.to_string(),  // No quotes for table cells
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Nil => "nil".to_string(),
        Value::Null => "null".to_string(),
        Value::Void => "void".to_string(),
        _ => val.to_string(),
    }
}

fn format_obj_as_record(obj: &Obj) -> String {
    let mut parts = Vec::new();
    for (key, val) in obj.iter() {
        parts.push(format!("{}: {}", key, val));
    }
    parts.join(", ")
}

// ──────────────────────────────────────────────────────────────────────────
// Plan 036 P1: bash-compatible plain-text formatting for structured commands
//
// When `--bash-compat` is active, format_output dispatches here to render
// structured Atoms (ls/grep/wc/ps output) as bash-style text instead of a
// ratatui table, so their stdout matches bash for parity testing.
// Returns None for atom types with no bash-classic equivalent (caller falls
// back to into_text()).
// ──────────────────────────────────────────────────────────────────────────

/// Format a structured Atom's value as bash-compatible plain text.
/// Returns None if no bash-classic rendering is defined for this atom type,
/// in which case the caller should fall back to `into_text()`.
pub fn format_atom_as_bash(atom_type: AtomType, value: &Value) -> Option<String> {
    match atom_type {
        AtomType::FileList | AtomType::FileEntry => format_file_list_as_bash(value),
        AtomType::MatchList => format_match_list_as_bash(value),
        AtomType::CountResult => format_count_result_as_bash(value),
        AtomType::ProcessList | AtomType::ProcessEntry => format_process_list_as_bash(value),
        AtomType::Path => Some(value_to_plain_string(value)),
        // Side-effect summary records (mkdir/cp/mv/rm result objects) are
        // silent in bash on success; emit empty so their stdout matches bash.
        AtomType::Record | AtomType::BuildResult | AtomType::RunResult => Some(String::new()),
        // Table/SystemInfo/DiskEntry/... have no single bash-classic form;
        // fall back to the default text rendering.
        _ => None,
    }
}

/// ls → one name per line (ls -1 style).
fn format_file_list_as_bash(value: &Value) -> Option<String> {
    // Detect long format: ls_command_value sets `permissions` only when -l is
    // used (mirrors how ps omits `command` in non-long mode). If the first
    // entry has permissions, render bash ls -l style columns.
    let is_long = match value {
        Value::Array(arr) => arr.iter().next().map_or(false, |item| {
            matches!(item, Value::Obj(obj) if obj.get("permissions").is_some())
        }),
        Value::Obj(_) => matches!(value, Value::Obj(obj) if obj.get("permissions").is_some()),
        _ => false,
    };
    if is_long {
        return format_file_list_long_as_bash(value);
    }

    // Short format: one entry per line. ls entries carry `name` (bare
    // filename); find entries carry `path` (relative path like `./a.log`).
    // Fall back to `path` when `name` is absent so find prints paths (what
    // bash `find` does) while ls prints filenames (Plan 036 defect-B).
    let names: Vec<String> = match value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                obj_field_str(item, "name").or_else(|| obj_field_str(item, "path"))
            })
            .collect(),
        Value::Obj(_) => obj_field_str(value, "name")
            .or_else(|| obj_field_str(value, "path"))
            .into_iter()
            .collect(),
        _ => return None,
    };
    if names.is_empty() {
        return Some(String::new());
    }
    Some(names.join("\n"))
}

/// ls -l → bash long format: `<perms> <links> <owner> <group> <size> <modified> <name>`
///
/// Note: ash's AshFileEntry lacks links count and group, so those are filled
/// with placeholders (`1` for links, owner reused for group). This yields a
/// visually-correct but not byte-identical bash ls -l; strict parity would
/// require extending AshFileEntry (tracked as a residual gap in plans/036).
fn format_file_list_long_as_bash(value: &Value) -> Option<String> {
    let arr = match value {
        Value::Array(arr) => arr,
        Value::Obj(_) => {
            // single entry
            return format_one_file_long(value);
        }
        _ => return None,
    };
    let mut lines = Vec::new();
    for item in arr.iter() {
        if let Some(line) = format_one_file_long(item) {
            lines.push(line);
        }
    }
    Some(lines.join("\n"))
}

fn format_one_file_long(item: &Value) -> Option<String> {
    let perms = obj_field_plain(item, "permissions").unwrap_or_else(|| "?".to_string());
    let owner = obj_field_plain(item, "owner").unwrap_or_else(|| "-".to_string());
    let size = obj_field_plain(item, "size").unwrap_or_else(|| "0".to_string());
    let modified = obj_field_plain(item, "modified").unwrap_or_else(|| "-".to_string());
    let name = obj_field_str(item, "name")?;
    // links=1 placeholder (ash has no link count); group reuses owner.
    Some(format!("{:<11} 1 {:<8} {:<8} {:>8} {} {}", perms, owner, owner, size, modified, name))
}

/// grep → matching line text, one per line. If `line_number` is present,
/// prefix it as `lineno:text` (grep -n style).
fn format_match_list_as_bash(value: &Value) -> Option<String> {
    let arr = match value {
        Value::Array(arr) => arr,
        _ => return None,
    };
    let mut lines = Vec::new();
    for item in arr.iter() {
        if let Value::Obj(obj) = item {
            // -l (files-with-matches) mode: obj only has `file`.
            if let Some(file) = obj_field_str(item, "file") {
                if obj_field_str(item, "text").is_none() && obj.get("count").is_none() {
                    // files-with-matches mode: just emit the file name
                    lines.push(file);
                    continue;
                }
            }
            // -c (count) mode: obj has `file` + `count`.
            if let Some(count_val) = obj.get("count") {
                lines.push(format_value_for_table(&count_val));
                continue;
            }
            // normal match: `text` (always present), optional `line_number`.
            let text = obj_field_str(item, "text").unwrap_or_default();
            if let Some(ln) = obj.get("line_number") {
                lines.push(format!("{}:{}", format_value_for_table(&ln), text));
            } else {
                lines.push(text);
            }
        }
    }
    Some(lines.join("\n"))
}

/// wc → a bare count number (pipe form: `cat f | wc -l` → `3`, no filename).
/// CountResult may be an Obj (single count) or Array (multiple files + total).
fn format_count_result_as_bash(value: &Value) -> Option<String> {
    match value {
        Value::Obj(obj) => count_obj_to_number(obj),
        Value::Array(arr) => {
            // Multi-file wc: the last entry is the total (file == "total").
            // Emit the total's count, or sum if no total row.
            let mut total: Option<i64> = None;
            let mut sum: i64 = 0;
            for item in arr.iter() {
                if let Value::Obj(o) = item {
                    if matches!(obj_field_str(item, "file").as_deref(), Some("total")) {
                        total = count_obj_to_i64(o);
                    } else if let Some(n) = count_obj_to_i64(o) {
                        sum += n;
                    }
                }
            }
            Some(total.unwrap_or(sum).to_string())
        }
        Value::Int(i) => Some(i.to_string()),
        Value::Uint(u) => Some(u.to_string()),
        _ => None,
    }
}

/// ps → classic columns: `  PID NAME` (or `  PID NAME COMMAND` if long).
fn format_process_list_as_bash(value: &Value) -> Option<String> {
    let arr = match value {
        Value::Array(arr) => arr,
        Value::Obj(_) => {
            // single process entry
            return format_one_process(value);
        }
        _ => return None,
    };
    // Detect long mode (has `command` field on first entry).
    let has_command = arr
        .iter()
        .next()
        .map(|item| matches!(item, Value::Obj(obj) if obj.get("command").is_some()))
        .unwrap_or(false);
    let mut lines = Vec::new();
    for item in arr.iter() {
        if let Some(line) = format_process_entry(item, has_command) {
            lines.push(line);
        }
    }
    Some(lines.join("\n"))
}

fn format_process_entry(item: &Value, has_command: bool) -> Option<String> {
    let pid = obj_field_plain(item, "pid")?;
    let name = obj_field_plain(item, "name")?;
    if has_command {
        let cmd = obj_field_plain(item, "command").unwrap_or_default();
        Some(format!("{:>6} {} {}", pid, name, cmd))
    } else {
        Some(format!("{:>6} {}", pid, name))
    }
}

fn format_one_process(value: &Value) -> Option<String> {
    let has_command = matches!(value, Value::Obj(obj) if obj.get("command").is_some());
    format_process_entry(value, has_command)
}

// ── helpers ──────────────────────────────────────────────────────────────

/// Extract a string field from an Obj value (Value::Str → unquoted string).
fn obj_field_str(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Obj(obj) => match obj.get(key)? {
            Value::Str(s) => Some(s.to_string()),
            other => Some(format_value_for_table(&other)),
        },
        _ => None,
    }
}

/// Extract a field formatted for plain display (int/uint/str → string).
fn obj_field_plain(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Obj(obj) => Some(format_value_for_table(&obj.get(key)?)),
        _ => None,
    }
}

/// Render a scalar Value as a plain string (Str without quotes, Int/Uint as-is).
fn value_to_plain_string(value: &Value) -> String {
    format_value_for_table(value)
}

/// A CountResult Obj → its numeric count (first of lines/words/bytes/chars).
fn count_obj_to_i64(obj: &Obj) -> Option<i64> {
    for key in &["lines", "words", "bytes", "chars"] {
        if let Some(Value::Int(n)) = obj.get(*key) {
            return Some(n as i64);
        }
    }
    None
}

fn count_obj_to_number(obj: &Obj) -> Option<String> {
    Some(count_obj_to_i64(obj)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_file_entry() {
        let entry = build_file_entry(
            "test.txt",
            "file",
            Some(1024),
            Some("2025-01-22 10:00".to_string()),
            Some("-rw-r--r--".to_string()),
        );

        if let Value::Obj(obj) = entry {
            assert_eq!(obj.get("name").unwrap(), Value::str("test.txt"));
            assert_eq!(obj.get("type").unwrap(), Value::str("file"));
            assert_eq!(obj.get("size").unwrap(), Value::Int(1024));
        } else {
            panic!("Expected Obj");
        }
    }

    #[test]
    fn test_build_file_entry_minimal() {
        let entry = build_file_entry("test", "file", None, None, None);

        if let Value::Obj(obj) = entry {
            assert_eq!(obj.get("name").unwrap(), Value::str("test"));
            assert_eq!(obj.get("type").unwrap(), Value::str("file"));
            assert!(obj.get("size").is_none());
            assert!(obj.get("modified").is_none());
            assert!(obj.get("permissions").is_none());
        } else {
            panic!("Expected Obj");
        }
    }

    #[test]
    fn test_format_primitive() {
        let val = Value::Int(42);
        let formatted = format_value_for_display(&val);
        assert_eq!(formatted, "42");
    }

    #[test]
    fn test_format_string() {
        let val = Value::str("hello");
        let formatted = format_value_for_display(&val);
        // Value::Str adds quotes in Display implementation
        assert_eq!(formatted, "\"hello\"");
    }

    #[test]
    fn test_format_obj() {
        let mut obj = Obj::new();
        obj.set("key", Value::str("value"));
        obj.set("count", Value::Int(42));

        let val = Value::Obj(obj);
        let formatted = format_value_for_display(&val);
        // format_obj_as_record outputs "key: \"value\", count: 42"
        assert!(formatted.contains("key:"));
        assert!(formatted.contains("\"value\""));
        assert!(formatted.contains("count:"));
        assert!(formatted.contains("42"));
    }

    // ── Plan 036 P1: bash-compat formatter tests ────────────────────────

    fn file_entry(name: &str) -> Value {
        build_file_entry(name, "file", Some(100), None, None)
    }

    #[test]
    fn bash_compat_file_list_one_name_per_line() {
        let arr = Array::from(vec![file_entry("a.txt"), file_entry("b.txt")]);
        let out = format_atom_as_bash(AtomType::FileList, &Value::Array(arr));
        assert_eq!(out.as_deref(), Some("a.txt\nb.txt"));
    }

    #[test]
    fn bash_compat_file_entry_single() {
        let out = format_atom_as_bash(AtomType::FileEntry, &file_entry("only.txt"));
        assert_eq!(out.as_deref(), Some("only.txt"));
    }

    #[test]
    fn bash_compat_match_list_text_lines() {
        let mut o1 = Obj::new();
        o1.set("file", Value::str("f.txt"));
        o1.set("text", Value::str("apple"));
        let mut o2 = Obj::new();
        o2.set("file", Value::str("f.txt"));
        o2.set("text", Value::str("apricot"));
        let arr = Array::from(vec![Value::Obj(o1), Value::Obj(o2)]);
        let out = format_atom_as_bash(AtomType::MatchList, &Value::Array(arr));
        assert_eq!(out.as_deref(), Some("apple\napricot"));
    }

    #[test]
    fn bash_compat_match_list_with_line_numbers() {
        let mut o = Obj::new();
        o.set("file", Value::str("f.txt"));
        o.set("line_number", Value::Int(3));
        o.set("text", Value::str("apple"));
        let arr = Array::from(vec![Value::Obj(o)]);
        let out = format_atom_as_bash(AtomType::MatchList, &Value::Array(arr));
        assert_eq!(out.as_deref(), Some("3:apple"));
    }

    #[test]
    fn bash_compat_count_result_obj() {
        let mut o = Obj::new();
        o.set("lines", Value::Int(5));
        let out = format_atom_as_bash(AtomType::CountResult, &Value::Obj(o));
        assert_eq!(out.as_deref(), Some("5"));
    }

    #[test]
    fn bash_compat_count_result_bare_int() {
        let out = format_atom_as_bash(AtomType::CountResult, &Value::Int(42));
        assert_eq!(out.as_deref(), Some("42"));
    }

    #[test]
    fn bash_compat_unsupported_returns_none() {
        // Table / Record / SystemInfo have no bash-classic form.
        let arr = Array::from(vec![file_entry("x.txt")]);
        assert_eq!(format_atom_as_bash(AtomType::Table, &Value::Array(arr)), None);
    }
}
