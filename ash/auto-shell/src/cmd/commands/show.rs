//! `show` command — read a file (or pipeline text) and parse it into the
//! pipeline based on its extension or an explicit `--as` flag.
//!
//! Formerly `open` (Plan 001); renamed in Plan 004 because `open` now means
//! "launch with the OS default application" across the auto-os ecosystem.

use std::path::Path;

use auto_val::Value;
use miette::{IntoDiagnostic, Result};

use crate::cmd::commands::from_csv::parse_csv;
use crate::cmd::commands::from_json::parse_json;
use crate::cmd::parser::ParsedArgs;
use crate::cmd::pipeline_convert::{atom_to_pipeline_data, pipeline_data_to_atom};
use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;

/// The target output format for `show`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Csv,
    Text,
}

/// Decide the output format from an explicit `--as` option, a file extension
/// hint, defaulting to text. Priority: `--as` > extension > text (rules 4 & 11).
fn resolve_format(args: &ParsedArgs, ext_hint: Option<&str>) -> Format {
    match args.get_option("as").map(|s| s.as_str()) {
        Some("json") => Format::Json,
        Some("csv") => Format::Csv,
        Some("text") => Format::Text,
        Some(_other) => {
            // Unknown --as value: fall back to text rather than guessing.
            Format::Text
        }
        None => match ext_hint.map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("json") => Format::Json,
            Some("csv") => Format::Csv,
            _ => Format::Text,
        },
    }
}

/// Parse already-loaded text according to a target format. Pure function,
/// extracted for unit testing without touching the filesystem (rules 5/6/7).
fn parse_text(text: &str, fmt: Format) -> Result<PipelineData> {
    match fmt {
        Format::Json => Ok(PipelineData::from_value(parse_json(text)?)),
        Format::Csv => Ok(PipelineData::from_value(Value::Array(parse_csv(
            text, ",", true,
        )?))),
        Format::Text => Ok(PipelineData::from_text(text.to_string())),
    }
}

/// Lower-cased file extension, or None if absent/empty.
fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
}

pub struct ShowCommand;

impl Command for ShowCommand {
    fn name(&self) -> &str {
        "show"
    }

    fn signature(&self) -> Signature {
        Signature::new("show", "Read a file and parse it into the pipeline by extension")
            .optional("file", "Path to the file to show (default: parse pipeline text)")
            .option_with_short(
                "as",
                'a',
                "Force a format: json | csv | text (default: infer from extension)",
            )
            .extra_help(
                "Formatting rules:\n  \
                 .json  → parsed to a structured Value (table/record)\n  \
                 .csv   → parsed to a table (Array of Obj)\n  \
                 other  → raw file text (same as `cat`)",
            )
    }

    fn run(
        &self,
        args: &ParsedArgs,
        input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        // Rule 10: only a single file is supported this iteration.
        if args.positionals.len() > 1 {
            miette::bail!("show: only one file argument is supported");
        }

        let (text, ext_hint) = if let Some(path) = args.positionals.first() {
            // Rule 1 & 8: a file argument takes precedence over pipeline input.
            // Plan 009: resolve via shell (honors --sandbox).
            let resolved = shell.resolve_path(path, false)?;
            if !resolved.exists() {
                miette::bail!("show: {}: No such file or directory", path);
            }
            let content = std::fs::read_to_string(&resolved)
                .into_diagnostic()
                .map_err(|e| miette::miette!("show: {}: {}", path, e))?;
            (content, extension_of(&resolved))
        } else {
            // Rule 2 & 3: no file → consume pipeline text, else error.
            match input {
                PipelineData::Text(s) => (s, None),
                PipelineData::Value(Value::Str(s)) => (s.to_string(), None),
                _ => miette::bail!("show: no input (provide a file or pipe text)"),
            }
        };

        let fmt = resolve_format(args, ext_hint.as_deref());
        parse_text(&text, fmt)
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: ash_core::pipeline::AtomPipeline,
        shell: &mut Shell,
    ) -> Result<ash_core::pipeline::AtomPipeline> {
        let legacy_in = atom_to_pipeline_data(input);
        let legacy_out = self.run(args, legacy_in, shell)?;
        Ok(pipeline_data_to_atom(legacy_out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with_as(value: Option<&str>) -> ParsedArgs {
        let mut args = ParsedArgs::default();
        if let Some(v) = value {
            args.named.insert("as".to_string(), v.to_string());
        }
        args
    }

    #[test]
    fn resolve_format_uses_extension_when_no_as() {
        assert_eq!(resolve_format(&args_with_as(None), Some("json")), Format::Json);
        assert_eq!(resolve_format(&args_with_as(None), Some("csv")), Format::Csv);
    }

    #[test]
    fn resolve_format_defaults_to_text_for_unknown_extension() {
        assert_eq!(resolve_format(&args_with_as(None), Some("txt")), Format::Text);
        assert_eq!(resolve_format(&args_with_as(None), Some("log")), Format::Text);
    }

    #[test]
    fn resolve_format_defaults_to_text_when_no_extension() {
        assert_eq!(resolve_format(&args_with_as(None), None), Format::Text);
    }

    #[test]
    fn resolve_format_as_overrides_extension() {
        // rule 11: --as json on a .csv file → Json
        assert_eq!(
            resolve_format(&args_with_as(Some("json")), Some("csv")),
            Format::Json
        );
        assert_eq!(
            resolve_format(&args_with_as(Some("csv")), Some("json")),
            Format::Csv
        );
    }

    #[test]
    fn resolve_format_as_works_without_extension_hint() {
        // pipe mode: no extension, explicit --as
        assert_eq!(resolve_format(&args_with_as(Some("csv")), None), Format::Csv);
        assert_eq!(resolve_format(&args_with_as(Some("json")), None), Format::Json);
    }

    #[test]
    fn resolve_format_extension_is_case_insensitive() {
        assert_eq!(resolve_format(&args_with_as(None), Some("JSON")), Format::Json);
        assert_eq!(resolve_format(&args_with_as(None), Some("CSV")), Format::Csv);
    }

    // ---- parse_text: the actual parsing, filesystem-free ----

    #[test]
    fn parse_text_csv_produces_array_of_obj() {
        let csv = "name,age\nalice,30\nbob,25\n";
        let out = parse_text(csv, Format::Csv).unwrap();
        match out {
            PipelineData::Value(Value::Array(arr)) => {
                assert_eq!(arr.len(), 2, "two data rows");
                let row0 = arr.get(0).unwrap();
                match row0 {
                    Value::Obj(obj) => {
                        let name = obj.get_str("name").map(|s| s.to_string());
                        let age = obj.get_str("age").map(|s| s.to_string());
                        assert_eq!(name, Some("alice".to_string()));
                        assert_eq!(age, Some("30".to_string()));
                    }
                    other => panic!("expected Obj row, got {:?}", other),
                }
            }
            other => panic!("expected Array value, got {:?}", other),
        }
    }

    #[test]
    fn parse_text_json_array_produces_value() {
        let json = r#"[{"x":1},{"x":2}]"#;
        let out = parse_text(json, Format::Json).unwrap();
        assert!(matches!(out, PipelineData::Value(Value::Array(_))));
    }

    #[test]
    fn parse_text_json_object_produces_value() {
        let json = r#"{"a":1,"b":"two"}"#;
        let out = parse_text(json, Format::Json).unwrap();
        assert!(matches!(out, PipelineData::Value(Value::Obj(_))));
    }

    #[test]
    fn parse_text_text_passes_through() {
        let out = parse_text("plain contents\n", Format::Text).unwrap();
        match out {
            PipelineData::Text(s) => assert_eq!(s, "plain contents\n"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_text_invalid_json_errors() {
        let result = parse_text("{ not valid json", Format::Json);
        assert!(result.is_err(), "malformed JSON should error");
    }

    #[test]
    fn parse_text_empty_csv_yields_empty_table() {
        let out = parse_text("", Format::Csv).unwrap();
        match out {
            PipelineData::Value(Value::Array(arr)) => assert_eq!(arr.len(), 0),
            other => panic!("expected empty Array, got {:?}", other),
        }
    }

    // ---- extension_of ----

    #[test]
    fn extension_of_lowercase_known_extensions() {
        assert_eq!(extension_of(Path::new("a.csv")), Some("csv".to_string()));
        assert_eq!(extension_of(Path::new("a.JSON")), Some("json".to_string()));
    }

    #[test]
    fn extension_of_none_when_absent() {
        assert_eq!(extension_of(Path::new("README")), None);
        assert_eq!(extension_of(Path::new("a.")), None);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::shell::Shell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Strip ANSI escape sequences so assertions can match plain text in
    /// rendered (colorized) table output.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for cc in chars.by_ref() {
                        if cc.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    continue;
                }
            }
            out.push(c);
        }
        out
    }

    /// Create a temp file with the given name suffix and contents, returning
    /// its absolute path as a forward-slash string (parse_args treats a bare
    /// `\` as an escape char, so backslash Windows paths get mangled).
    fn write_temp(name: &str, contents: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("ash_show_test_{}_{}", pid, n));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn show_csv_file_renders_table() {
        let path = write_temp("data.csv", "name,age\nalice,30\nbob,25\n");
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("show {}", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        assert!(plain.contains("alice"), "row 'alice' should appear: {plain}");
        assert!(plain.contains("bob"), "row 'bob' should appear: {plain}");
        assert!(plain.contains("30"), "value '30' should appear: {plain}");
        let dir = std::path::Path::new(&path).parent().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn show_json_array_file_renders_table() {
        let path = write_temp("data.json", r#"[{"x":1},{"x":2}]"#);
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("show {}", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        assert!(plain.contains('1'), "value should appear: {plain}");
        let dir = std::path::Path::new(&path).parent().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn show_unknown_extension_is_text() {
        let path = write_temp("notes.txt", "hello world\nsecond line\n");
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("show {}", path))
            .unwrap_or(None)
            .unwrap_or_default();
        assert_eq!(out.trim(), "hello world\nsecond line");
        let dir = std::path::Path::new(&path).parent().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn show_nonexistent_file_errors() {
        let mut shell = Shell::new();
        let result = shell.execute("show /no/such/file_xyz.csv");
        assert!(result.is_err(), "missing file should error");
    }

    #[test]
    fn show_as_overrides_extension() {
        let path = write_temp("data.txt", "a,b\n1,2\n");
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("show {} --as csv", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        assert!(plain.contains('1'), "csv value should appear: {plain}");
        let dir = std::path::Path::new(&path).parent().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn show_help_lists_options() {
        let mut shell = Shell::new();
        let out = shell.execute("show --help").unwrap_or(None).unwrap_or_default();
        assert!(out.contains("--as"), "--as should be in help: {out}");
        assert!(out.contains("show"), "usage should mention show: {out}");
    }
}
