//! Brief one-line summaries of tool args / results.
//!
//! Shared by the F4 chat turn handler (now `ash_tui::repl`) and `ash ask`
//! (`ai::ask`). Pure string formatting — no terminal dependencies — so it
//! lives in the terminal-dep-free `auto-shell` crate (Plan 037 M2.0/M2.2).

/// Truncate a string to `max` chars, appending an ellipsis if cut.
pub fn brief_truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}\u{2026}")
    }
}

/// Render tool-call args (a JSON value) as a brief one-line summary.
pub fn brief_args(args: &serde_json::Value) -> String {
    let s = match args {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Object(map) => {
            // Show {"key": value, ...} compactly, focusing on string args.
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| match v {
                    serde_json::Value::String(s) => format!("{k}: {s}"),
                    _ => format!("{k}: {v}"),
                })
                .collect();
            parts.join(", ")
        }
        other => other.to_string(),
    };
    brief_truncate(&s, 80)
}

/// Render a tool result as a brief one-line summary (first non-empty line).
pub fn brief_result(result: &str) -> String {
    let first_line = result.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    brief_truncate(first_line.trim(), 80)
}

/// Plan 079: render a tool result for the chat transcript. Multi-line
/// output (tables, long listings) summarizes as a line count instead of
/// the first line — a table's header row reads as garbage ("Name Type
/// Size Modified"); a count tells the user the tool ran and produced
/// data. Single-line results keep their content (pwd/eval stay useful).
pub fn brief_tool_result(result: &str) -> String {
    let lines: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    match lines.len() {
        0 => String::new(),
        1 => brief_truncate(lines[0].trim(), 80),
        n => format!("{n} 行"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_single_line_shows_content() {
        assert_eq!(brief_tool_result("hello"), "hello");
        assert_eq!(brief_tool_result("/tmp/dir"), "/tmp/dir");
    }

    #[test]
    fn tool_result_single_line_truncates() {
        let long = "x".repeat(100);
        let out = brief_tool_result(&long);
        assert_eq!(out.chars().count(), 81); // 80 + ellipsis
        assert!(out.ends_with('\u{2026}'));
    }

    #[test]
    fn tool_result_multiline_shows_count() {
        assert_eq!(brief_tool_result("Name Type\nrow1\nrow2"), "3 行");
    }

    #[test]
    fn tool_result_blank_lines_ignored() {
        assert_eq!(brief_tool_result("\n\nonly\n\n"), "only");
        assert_eq!(brief_tool_result("  \n  \n"), "");
        assert_eq!(brief_tool_result(""), "");
    }
}
