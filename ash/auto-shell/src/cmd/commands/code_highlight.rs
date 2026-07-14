//! Code syntax highlighting for the `show` command.
//!
//! Uses `syntect` (the same engine bat uses) to colorize source code by
//! language. For TOML/INI (not in syntect's default set), falls back to a
//! lightweight regex-based highlighter. Maps file extensions to syntax
//! definitions and renders with ANSI escape codes.

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// File extensions that `show` should render with syntax highlighting.
pub fn is_code_file(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "toml" | "json" | "yaml" | "yml" | "xml" | "ini" | "conf" | "cfg"
        | "rs" | "py" | "js" | "ts" | "jsx" | "tsx"
        | "go" | "java" | "kt" | "scala" | "c" | "h" | "cpp" | "hpp" | "cc"
        | "cs" | "rb" | "php" | "swift" | "dart"
        | "sh" | "bash" | "zsh" | "fish" | "ps1"
        | "sql" | "graphql" | "proto"
        | "html" | "css" | "scss" | "less"
        | "md" | "markdown"
        | "dockerfile"
        | "gitignore" | "gitattributes"
        | "lua" | "r" | "jl" | "ex" | "exs" | "erl" | "hs" | "clj" | "cljs"
        | "vim" | "nim" | "zig" | "v" | "ml" | "fs"
    )
}

/// Highlight code text with ANSI color escapes.
pub fn highlight_code(text: &str, ext: &str) -> String {
    let extension = ext.to_ascii_lowercase();

    // TOML/INI are not in syntect's default syntax set — use a lightweight
    // regex-based highlighter.
    if matches!(extension.as_str(), "toml" | "ini" | "conf" | "cfg") {
        return highlight_toml_like(text);
    }

    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let syntax = match find_syntax_by_extension(&ps, &extension) {
        Some(s) => s,
        None => return text.to_string(),
    };

    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.get("base16-eighties.dark"))
        .unwrap();

    let mut h = HighlightLines::new(syntax, theme);
    let mut output = String::with_capacity(text.len());

    for line in LinesWithEndings::from(text) {
        let regions: Vec<(Style, &str)> = match h.highlight_line(line, &ps) {
            Ok(r) => r,
            Err(_) => {
                output.push_str(line);
                continue;
            }
        };
        let escaped = as_24_bit_terminal_escaped(&regions[..], false);
        output.push_str(&escaped);
        if !escaped.ends_with("\x1b[0m") {
            output.push_str("\x1b[0m");
        }
    }
    output
}

/// Lightweight highlighter for TOML/INI-style config files.
fn highlight_toml_like(text: &str) -> String {
    use nu_ansi_term::{Color, Style};

    let key_style = Style::new().fg(Color::Cyan);
    let string_style = Style::new().fg(Color::LightYellow);
    let num_style = Style::new().fg(Color::Purple);
    let bool_style = Style::new().fg(Color::Red);
    let comment_style = Style::new().fg(Color::DarkGray).italic();
    let table_style = Style::new().fg(Color::Blue).bold();

    let mut output = String::with_capacity(text.len() * 2);

    for line in text.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with('#') {
            output.push_str(&comment_style.paint(line).to_string());
        } else if trimmed.starts_with('[') {
            output.push_str(&table_style.paint(line).to_string());
        } else if let Some(eq_pos) = line.find('=') {
            let key_part = &line[..eq_pos];
            let value_part = &line[eq_pos + 1..];

            output.push_str(&key_style.paint(key_part).to_string());
            output.push('=');

            let v = value_part.trim_start();
            let leading_ws_len = value_part.len() - v.len();
            let leading_ws = &value_part[..leading_ws_len];

            if v.starts_with('"') || v.starts_with('\'') {
                output.push_str(leading_ws);
                output.push_str(&string_style.paint(v).to_string());
            } else if v == "true" || v == "false" {
                output.push_str(leading_ws);
                output.push_str(&bool_style.paint(v).to_string());
            } else if v.parse::<f64>().is_ok() && !v.is_empty() {
                output.push_str(leading_ws);
                output.push_str(&num_style.paint(v).to_string());
            } else if let Some(hash_pos) = find_comment_pos(v) {
                let (val, comment) = v.split_at(hash_pos);
                let val_t = val.trim_end();
                output.push_str(leading_ws);
                if val_t.starts_with('"') || val_t.starts_with('\'') {
                    output.push_str(&string_style.paint(val_t).to_string());
                } else if val_t.parse::<f64>().is_ok() {
                    output.push_str(&num_style.paint(val_t).to_string());
                } else {
                    output.push_str(val_t);
                }
                // Preserve whitespace between value and comment
                let ws_between = &val[val_t.len()..];
                output.push_str(ws_between);
                output.push_str(&comment_style.paint(comment).to_string());
            } else {
                output.push_str(value_part);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    if !text.ends_with('\n') && output.ends_with('\n') {
        output.pop();
    }
    output
}

/// Find `#` comment not inside a string.
fn find_comment_pos(s: &str) -> Option<usize> {
    let mut in_string = false;
    let mut quote_char = '"';
    for (i, c) in s.char_indices() {
        if in_string {
            if c == quote_char {
                in_string = false;
            }
        } else if c == '"' || c == '\'' {
            in_string = true;
            quote_char = c;
        } else if c == '#' {
            return Some(i);
        }
    }
    None
}

fn find_syntax_by_extension<'a>(
    ps: &'a SyntaxSet,
    ext: &str,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    if let Some(s) = ps.find_syntax_by_extension(ext) {
        return Some(s);
    }
    let alias = match ext {
        "dockerfile" => "dockerfile",
        "gitignore" | "gitattributes" => "gitignore",
        "sh" | "bash" => "shell",
        "zsh" => "bash",
        "ps1" => "powershell",
        "md" | "markdown" => "markdown",
        "cc" => "cpp",
        "h" => "c",
        "hpp" => "cpp",
        _ => return None,
    };
    ps.find_syntax_by_token(alias)
        .or_else(|| ps.find_syntax_by_name(alias))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_code_file() {
        assert!(is_code_file("toml"));
        assert!(is_code_file("rs"));
        assert!(is_code_file("py"));
        assert!(is_code_file("JSON"));
        assert!(!is_code_file("txt"));
        assert!(!is_code_file("csv"));
        assert!(!is_code_file(""));
    }

    #[test]
    fn test_highlight_toml() {
        let input = "name = \"ash\"\nversion = \"0.1.0\"\n";
        let result = highlight_code(input, "toml");
        assert!(
            result.contains("\x1b["),
            "highlighted output should contain ANSI codes"
        );
        assert!(result.contains("name"));
        assert!(result.contains("ash"));
    }

    #[test]
    fn test_highlight_toml_table_and_comment() {
        let input = "[dependencies]\n# a comment\nfoo = 42\nbar = true\n";
        let result = highlight_code(input, "toml");
        assert!(result.contains("\x1b["), "should have ANSI codes");
        assert!(result.contains("dependencies"));
        assert!(result.contains("comment"));
    }

    #[test]
    fn test_highlight_unknown_ext_returns_plain() {
        let input = "hello world";
        let result = highlight_code(input, "xyz");
        assert_eq!(result, input);
    }

    #[test]
    fn test_highlight_rs() {
        let input = "fn main() { println!(\"hi\"); }";
        let result = highlight_code(input, "rs");
        assert!(result.contains("\x1b["), "Rust code should be highlighted");
        assert!(result.contains("fn"));
    }
}
