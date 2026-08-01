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
