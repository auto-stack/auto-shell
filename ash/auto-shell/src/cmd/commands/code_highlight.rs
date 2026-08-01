//! Code syntax highlighting for the `show` command.
//!
//! Uses `syntect` (the same engine bat uses) to colorize source code by
//! language. For TOML/INI (not in syntect's default set), falls back to a
//! lightweight regex-based highlighter. Maps file extensions to syntax
//! definitions and renders with ANSI escape codes.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Lazily-loaded singleton syntax set (loading 75 grammars takes ~1s;
/// cached so subsequent `show` calls are instant).
fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(|| SyntaxSet::load_defaults_newlines())
}

/// Lazily-loaded singleton theme set.
fn theme_set() -> &'static ThemeSet {
    static TS: OnceLock<ThemeSet> = OnceLock::new();
    TS.get_or_init(|| ThemeSet::load_defaults())
}

/// Pre-warm the syntax and theme caches in a background thread.
/// Call this early (e.g. from `Shell::new()`) so that the first `show`
/// invocation doesn't block on syntect loading.
pub fn warmup() {
    std::thread::Builder::new()
        .name("syntect-warmup".into())
        .spawn(|| {
            let _ = syntax_set();
            let _ = theme_set();
        })
        .ok();
}

/// File extensions that `show` should render with syntax highlighting.
pub fn is_code_file(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "toml" | "json" | "yaml" | "yml" | "xml" | "ini" | "conf" | "cfg"
        | "rs" | "py" | "js" | "ts" | "jsx" | "tsx"
        | "go" | "java" | "kt" | "scala" | "c" | "h" | "cpp" | "hpp" | "cc"
        | "cs" | "rb" | "php" | "swift" | "dart"
        | "sh" | "bash" | "zsh" | "fish" | "ps1"
        | "ash" | "at" | "as" | "au"
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

    // TOML/INI are not in syntect's default syntax set. Plan 037 M2.2: the
    // nu-ansi-term-based TOML highlighter moved to ash-tui; here TOML/INI now
    // fall through to plain syntect (the documented fallback).
    let ps = syntax_set();
    let ts = theme_set();

    let syntax = match find_syntax_by_extension(ps, &extension) {
        Some(s) => s,
        None => return text.to_string(),
    };

    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.get("base16-eighties.dark"))
        .unwrap();

    let mut h = HighlightLines::new(syntax, theme);
    // ANSI escape codes add roughly 30-60 bytes per color span; a 2×
    // estimate avoids reallocations for typical source files.
    let mut output = String::with_capacity(text.len().saturating_mul(2));

    for line in LinesWithEndings::from(text) {
        let regions: Vec<(Style, &str)> = match h.highlight_line(line, &ps) {
            Ok(r) => r,
            Err(_) => {
                output.push_str(line);
                continue;
            }
        };
        // Write ANSI escapes directly into `output` instead of allocating a
        // per-line temporary string (saves ~N allocations for an N-line file).
        append_as_24_bit_escaped(&mut output, &regions);
    }
    output
}

/// Append 24-bit ANSI terminal escape codes for the given style regions
/// directly into `out`, avoiding a per-line temporary-string allocation.
fn append_as_24_bit_escaped(out: &mut String, regions: &[(Style, &str)]) {
    use std::fmt::Write;
    for &(ref style, text) in regions {
        let _ = write!(
            out,
            "\x1b[38;2;{};{};{}m",
            style.foreground.r, style.foreground.g, style.foreground.b
        );
        out.push_str(text);
    }
    out.push_str("\x1b[0m");
}

// ── Writer-based (streaming) variants ──────────────────────────────

/// Highlight `text` line-by-line and write each line immediately to `writer`.
///
/// Unlike [`highlight_code`], this does not build a full String in memory
/// before returning — the first highlighted line hits the writer in ~10 µs,
/// giving the user immediate feedback even for very large files.
pub fn highlight_code_to_writer(
    text: &str,
    ext: &str,
    writer: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let extension = ext.to_ascii_lowercase();

    // TOML/INI: Plan 037 M2.2, the nu-ansi-term highlighter moved to ash-tui;
    // here they fall through to plain text below.
    let ps = syntax_set();
    let ts = theme_set();

    let syntax = match find_syntax_by_extension(ps, &extension) {
        Some(s) => s,
        None => return writer.write_all(text.as_bytes()),
    };

    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.get("base16-eighties.dark"))
        .unwrap();

    let mut h = HighlightLines::new(syntax, theme);
    // Reusable per-line buffer — cleared and refilled each iteration so we
    // never allocate more than a single line's worth of ANSI escapes.
    let mut line_buf = String::with_capacity(512);

    for line in LinesWithEndings::from(text) {
        line_buf.clear();
        match h.highlight_line(line, &ps) {
            Ok(regions) => {
                append_as_24_bit_escaped(&mut line_buf, &regions);
                writer.write_all(line_buf.as_bytes())?;
            }
            Err(_) => {
                writer.write_all(line.as_bytes())?;
            }
        }
    }
    Ok(())
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
            // Plan 036: .ash scripts mix shell commands (>) with AutoLang blocks.
            // Map to "sh" so syntect's built-in Shell Script syntax applies.
            "ash" => "sh",
            // Auto-lang (*.at, *.as, *.au): Rust-inspired syntax (fn, var, if,
            // for, struct, impl, match, etc.). "Rust" syntax gives reasonable
            // highlighting until a dedicated AutoLang syntax is authored.
            // Unlike "shell", syntect DOES know about the "rs" extension,
            // so we map to "rs" directly.
            "at" | "as" | "au" => "rs",
            "md" | "markdown" => "markdown",
            "cc" => "cpp",
            "h" => "c",
            "hpp" => "cpp",
            _ => return None,
        };
        // Retry with the aliased extension first
        if let Some(s) = ps.find_syntax_by_extension(alias) {
            return Some(s);
        }
        // Fall back to token/name lookup for non-extension aliases
        // like "dockerfile", "gitignore", "powershell"
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
        // Plan 037 M2.2: the nu-ansi-term TOML highlighter moved to ash-tui;
        // here TOML falls through to plain syntect (no syntax match) → plain
        // text. Content is preserved verbatim, no ANSI escapes.
        let input = "name = \"ash\"\nversion = \"0.1.0\"\n";
        let result = highlight_code(input, "toml");
        assert!(
            !result.contains("\x1b["),
            "TOML should be plain text now (no ANSI codes)"
        );
        assert!(result.contains("name"));
        assert!(result.contains("ash"));
    }

    #[test]
    fn test_highlight_toml_table_and_comment() {
        // Plan 037 M2.2: plain-text fallback (see test_highlight_toml).
        let input = "[dependencies]\n# a comment\nfoo = 42\nbar = true\n";
        let result = highlight_code(input, "toml");
        assert!(!result.contains("\x1b["), "should be plain text");
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
    fn test_is_code_file_new_extensions() {
        // Plan 036: ash / auto-lang file types
        assert!(is_code_file("ash"));
        assert!(is_code_file("at"));
        assert!(is_code_file("as"));
        assert!(is_code_file("au"));
    }

    #[test]
    fn test_highlight_ash() {
        let input = "> echo hello\n> cat file | grep foo\nfn main() {\n    print(\"hi\")\n}";
        let result = highlight_code(input, "ash");
        assert!(result.contains("\x1b["), "ash scripts should be highlighted as shell");
        assert!(result.contains("echo"));
    }

    #[test]
    fn test_highlight_at() {
        let input = "fn main() {\n    var x = \"hello\"\n    if x.len() > 0 {\n        print(x)\n    }\n}";
        let result = highlight_code(input, "at");
        assert!(result.contains("\x1b["), "auto-lang code should be highlighted as Rust");
        assert!(result.contains("fn"));
    }

    #[test]
    fn test_highlight_rs() {
        let input = "fn main() { println!(\"hi\"); }";
        let result = highlight_code(input, "rs");
        assert!(result.contains("\x1b["), "Rust code should be highlighted");
        assert!(result.contains("fn"));
    }
}
