//! SmartCommand spec — the parsed form of a `command.at` file (Plan 029 §A.2-A.3).
//!
//! A `command.at` file declares one SmartCommand:
//!
//! ```text
//! command "git.finish-worktree" {
//!     description : "Finish a worktree: commit, push, then remove the branch"
//!     args        : ["target"]
//!     body        : "git.finish-worktree.ash"
//! }
//! ```
//!
//! `description` and `body` are required; `args` is optional (defaults to
//! empty). The parser is intentionally minimal (not a general `.at` parser) —
//! it handles one `command` block with quoted-string and array values.

use std::path::PathBuf;

/// A parsed SmartCommand declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartCommandSpec {
    /// Dotted command name, e.g. `git.finish-worktree`.
    pub name: String,
    /// One-line human description (shown by `ash smart list`).
    pub description: String,
    /// Positional argument names (documentation/validation; accessed in the
    /// body via `$1`, `$2`, …). May be empty.
    pub args: Vec<String>,
    /// Body script filename (relative to the `.at` file's directory). The body
    /// is an ash `.ash` script run with `system()` + AutoLang + `$1/$2`.
    pub body: String,
    /// Where this spec was loaded from (the `.at` file path). Used to resolve
    /// `body` relative to it. `None` for specs built in-memory (tests).
    pub source_path: Option<PathBuf>,
}

impl SmartCommandSpec {
    /// Build a spec in-memory (tests / programmatic construction).
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            args: Vec::new(),
            body: String::new(),
            source_path: None,
        }
    }

    /// Resolve the body script path relative to the spec's source file.
    /// Returns `None` if `source_path` is unset or `body` is empty.
    pub fn body_path(&self) -> Option<PathBuf> {
        let body = self.body.trim();
        if body.is_empty() {
            return None;
        }
        self.source_path
            .as_ref()
            .map(|dir| dir.parent().unwrap_or_else(|| std::path::Path::new(".")).join(body))
    }
}

/// Parse a `command.at` file's contents into a [`SmartCommandSpec`].
///
/// Returns `Err` if the file doesn't contain exactly one well-formed
/// `command "name" { ... }` block, or if `description`/`body` are missing.
pub fn parse_at(content: &str) -> Result<SmartCommandSpec, String> {
    let (name, body) = find_command_block(content)?;
    let mut spec = SmartCommandSpec::new(name, "");
    spec.source_path = None;

    for (key, value) in parse_fields(&body) {
        match key.as_str() {
            "description" => spec.description = parse_string_value(&value)?,
            "body" => spec.body = parse_string_value(&value)?,
            "args" => spec.args = parse_array_value(&value)?,
            _ => { /* ignore unknown fields for forward-compat */ }
        }
    }

    if spec.description.is_empty() {
        return Err(format!("SmartCommand '{}': missing 'description'", spec.name));
    }
    if spec.body.is_empty() {
        return Err(format!("SmartCommand '{}': missing 'body'", spec.name));
    }
    Ok(spec)
}

/// Locate the `command "NAME" { ... }` block. Returns `(name, block_body)`.
fn find_command_block(content: &str) -> Result<(String, String), String> {
    let content = content.trim();
    // Expect: command "name" {
    let cmd_kw = content
        .strip_prefix("command")
        .ok_or_else(|| "expected 'command \"name\" {'".to_string())?
        .trim_start();
    let ParsedString {
        value: name,
        len_with_quotes,
    } = parse_leading_quoted_string(cmd_kw)
        .ok_or_else(|| "expected quoted command name after 'command'".to_string())?;
    let rest = cmd_kw[len_with_quotes..].trim_start();
    let rest = rest
        .strip_prefix('{')
        .ok_or_else(|| "expected '{' after command name".to_string())?;
    // Find the matching closing brace. v1 specs have no nested braces, so a
    // simple search for the last '}' is sufficient and robust.
    let close = rest
        .rfind('}')
        .ok_or_else(|| "expected '}' to close command block".to_string())?;
    Ok((name, rest[..close].to_string()))
}

/// A parsed leading quoted string: holds the content and how many chars the
/// quoting consumed (for slicing the remainder).
struct ParsedString {
    value: String,
    len_with_quotes: usize,
}

/// If `s` starts with `"..."`, return the content + total length including
/// quotes. Handles `\"` and `\\` escapes.
fn parse_leading_quoted_string(s: &str) -> Option<ParsedString> {
    let bytes: Vec<char> = s.chars().collect();
    if bytes.first() != Some(&'"') {
        return None;
    }
    let mut value = String::new();
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                'n' => value.push('\n'),
                't' => value.push('\t'),
                other => value.push(other),
            }
            i += 2;
            continue;
        }
        if c == '"' {
            // Total chars consumed: i (opening quote) + 1 (closing quote),
            // measured in chars.
            let len_with_quotes = i + 1;
            return Some(ParsedString {
                value,
                len_with_quotes,
            });
        }
        value.push(c);
        i += 1;
    }
    None // unterminated
}

/// Parse the `key : value` lines inside a block body. Each value is the raw
/// text up to the newline (caller interprets string vs array).
fn parse_fields(body: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    fields
}

/// A value that is a quoted string → its content.
fn parse_string_value(raw: &str) -> Result<String, String> {
    parse_leading_quoted_string(raw)
        .map(|p| p.value)
        .ok_or_else(|| format!("expected a quoted string, got: {raw}"))
}

/// A value that is `["a", "b", ...]` → the elements.
fn parse_array_value(raw: &str) -> Result<Vec<String>, String> {
    let raw = raw.trim();
    let inner = raw
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .ok_or_else(|| format!("expected an array [...], got: {raw}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|item| {
            let item = item.trim();
            parse_leading_quoted_string(item)
                .map(|p| p.value)
                .ok_or_else(|| format!("array items must be quoted strings, got: {item}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"command "git.finish-worktree" {
    description : "Finish a worktree: commit, push, then remove the branch"
    args        : ["target"]
    body        : "git.finish-worktree.ash"
}
"#;

    #[test]
    fn parse_full_spec() {
        let spec = parse_at(SAMPLE).unwrap();
        assert_eq!(spec.name, "git.finish-worktree");
        assert_eq!(
            spec.description,
            "Finish a worktree: commit, push, then remove the branch"
        );
        assert_eq!(spec.args, vec!["target"]);
        assert_eq!(spec.body, "git.finish-worktree.ash");
    }

    #[test]
    fn parse_without_args() {
        let content = r#"command "hello" {
    description : "say hello"
    body        : "hello.ash"
}
"#;
        let spec = parse_at(content).unwrap();
        assert_eq!(spec.name, "hello");
        assert!(spec.args.is_empty());
    }

    #[test]
    fn parse_multiple_args() {
        let content = r#"command "deploy" {
    description : "deploy app"
    args        : ["env", "version"]
    body        : "deploy.ash"
}
"#;
        let spec = parse_at(content).unwrap();
        assert_eq!(spec.args, vec!["env", "version"]);
    }

    #[test]
    fn parse_ignores_unknown_fields() {
        let content = r#"command "x" {
    description : "d"
    body        : "x.ash"
    future      : "ignored"
}
"#;
        let spec = parse_at(content).unwrap();
        assert_eq!(spec.name, "x");
    }

    #[test]
    fn parse_ignores_comments() {
        let content = r#"command "x" {
    // a comment
    description : "d"
    body        : "x.ash"
}
"#;
        let spec = parse_at(content).unwrap();
        assert_eq!(spec.description, "d");
    }

    #[test]
    fn parse_escapes_in_strings() {
        let content = r#"command "x" {
    description : "say \"hi\" and \\path"
    body        : "x.ash"
}
"#;
        let spec = parse_at(content).unwrap();
        assert_eq!(spec.description, r#"say "hi" and \path"#);
    }

    #[test]
    fn error_missing_description() {
        let content = r#"command "x" {
    body : "x.ash"
}
"#;
        assert!(parse_at(content).is_err());
    }

    #[test]
    fn error_missing_body() {
        let content = r#"command "x" {
    description : "d"
}
"#;
        assert!(parse_at(content).is_err());
    }

    #[test]
    fn error_no_command_keyword() {
        assert!(parse_at("not a command").is_err());
    }

    #[test]
    fn error_unterminated_string() {
        let content = "command \"x {\nbody : \"x.ash\"\n}\n";
        assert!(parse_at(content).is_err());
    }

    #[test]
    fn body_path_resolves_relative_to_source() {
        let mut spec = SmartCommandSpec::new("x", "d");
        spec.body = "body.ash".to_string();
        spec.source_path = Some(PathBuf::from("/home/me/smart/x.at"));
        assert_eq!(
            spec.body_path(),
            Some(PathBuf::from("/home/me/smart/body.ash"))
        );
    }

    #[test]
    fn body_path_none_when_body_empty() {
        let spec = SmartCommandSpec::new("x", "d");
        assert_eq!(spec.body_path(), None);
    }

    #[test]
    fn empty_args_array() {
        let content = r#"command "x" {
    description : "d"
    args        : []
    body        : "x.ash"
}
"#;
        let spec = parse_at(content).unwrap();
        assert!(spec.args.is_empty());
    }
}
