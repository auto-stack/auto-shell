use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use ash_core::pipeline::AtomPipeline;
use auto_val::{Array, Value};
use miette::{IntoDiagnostic, Result};
use std::path::Path;

pub struct SortCommand;

impl Command for SortCommand {
    fn name(&self) -> &str {
        "sort"
    }

    fn signature(&self) -> Signature {
        Signature::new("sort", "Sort lines or records, optionally by field/column")
            .optional("file", "File to sort (default: stdin)")
            .flag_with_short("reverse", 'r', "Reverse sort order")
            .flag_with_short("numeric", 'n', "Numeric sort")
            .flag_with_short("unique", 'u', "Remove duplicate lines")
            .flag_with_short("ignore-case", 'f', "Fold lower case to upper case for comparison")
            .option_with_short(
                "with",
                'w',
                "Sort records by FIELD name (structured input)",
            )
            .option_with_short(
                "key",
                'k',
                "Sort text by column NUMBER (1-based)",
            )
            .option_with_short(
                "field-separator",
                't',
                "Field separator char for -k (default: whitespace)",
            )
    }

    fn run(
        &self,
        args: &ParsedArgs,
        input: PipelineData,
        _shell: &mut Shell,
    ) -> Result<PipelineData> {
        let reverse = args.has_flag("reverse");
        let numeric = args.has_flag("numeric");
        let unique = args.has_flag("unique");
        let ignore_case = args.has_flag("ignore-case");
        let with = args.get_option("with"); // -w <field>
        let key = args.get_option("key"); // -k <column>
        let sep = args.get_option("field-separator"); // -t <char>

        // Rule 3: -w and -k are mutually exclusive.
        if with.is_some() && key.is_some() {
            miette::bail!("sort: -w and -k are mutually exclusive");
        }

        if let Some(field) = with {
            // Rule 1 & 4: structured field sort. Input must be a Value::Array.
            let arr = match &input {
                PipelineData::Value(Value::Array(a)) => a.clone(),
                _ => miette::bail!("sort -w requires a list of records"),
            };
            let sorted = sort_array_by_field(&arr, field, reverse, numeric)?;
            Ok(PipelineData::from_value(Value::Array(sorted)))
        } else {
            // Text mode: read file or stdin, then sort lines or columns.
            let text = if let Some(path) = args.first() {
                std::fs::read_to_string(Path::new(path)).into_diagnostic()?
            } else {
                get_text(input)?
            };

            if let Some(k) = key {
                let col: usize = k
                    .parse()
                    .map_err(|_| miette::miette!("sort: -k expects a column number, got '{}'", k))?;
                Ok(PipelineData::from_text(sort_by_column(
                    &text, col, sep.map(|s| s.as_str()), reverse, numeric,
                )))
            } else {
                // Existing whole-line behavior (zero change).
                Ok(PipelineData::from_text(sort_lines(
                    &text, reverse, numeric, unique, ignore_case,
                )))
            }
        }
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        // Use the generic bridge so that structured (-w) output keeps its
        // Table/Record type tag and renders as a table. Previously this
        // flattened everything to text, which broke sort -w.
        let legacy_in = crate::cmd::pipeline_convert::atom_to_pipeline_data(input);
        let legacy_out = self.run(args, legacy_in, shell)?;
        Ok(crate::cmd::pipeline_convert::pipeline_data_to_atom(
            legacy_out,
        ))
    }
}

/// Extract text from PipelineData
fn get_text(input: PipelineData) -> Result<String> {
    match input {
        PipelineData::Text(s) => Ok(s),
        PipelineData::Value(Value::Str(s)) => Ok(s.to_string()),
        PipelineData::Value(Value::Array(arr)) => {
            let lines: Vec<String> = arr.iter().map(|v| v.as_str().to_string()).collect();
            Ok(lines.join("\n"))
        }
        _ => miette::bail!("sort: input must be text"),
    }
}

/// Sort lines according to the given flags
pub fn sort_lines(
    text: &str,
    reverse: bool,
    numeric: bool,
    unique: bool,
    ignore_case: bool,
) -> String {
    let mut lines: Vec<&str> = text.lines().collect();

    lines.sort_by(|a, b| {
        let cmp = if numeric {
            compare_numeric(a, b, ignore_case)
        } else if ignore_case {
            a.to_lowercase().cmp(&b.to_lowercase())
        } else {
            a.cmp(b)
        };
        if reverse {
            cmp.reverse()
        } else {
            cmp
        }
    });

    if unique {
        lines.dedup_by(|a, b| {
            if ignore_case {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        });
    }

    lines.join("\n")
}

/// Sort an Array of records (Obj) by a named field (Plan 003, -w mode).
///
/// Records missing the field sort last (stable). When `numeric` is true,
/// field values are compared as numbers (leading numeric prefix); otherwise
/// as strings.
pub fn sort_array_by_field(
    arr: &Array,
    field: &str,
    reverse: bool,
    numeric: bool,
) -> Result<Array> {
    // Pair each value with a sort key; None (missing field) sorts last.
    let mut keyed: Vec<(Option<f64>, String, Value)> = arr
        .iter()
        .map(|v| {
            let s = match v {
                Value::Obj(obj) => obj.get_str(field).map(|s| s.to_string()),
                _ => None,
            };
            let num = s.as_deref().and_then(extract_leading_number);
            (num, s.unwrap_or_default(), v.clone())
        })
        .collect();

    keyed.sort_by(|a, b| {
        let ord = match (numeric, &a.0, &b.0) {
            (true, Some(na), Some(nb)) => na.partial_cmp(nb).unwrap_or(std::cmp::Ordering::Equal),
            _ => a.1.cmp(&b.1),
        };
        // Missing-field handling: a record with a present field always sorts
        // before one without. We encode presence via the numeric/str pair:
        // if both have a key string, normal compare; if a has key but b
        // doesn't, a first. We detect "has key" by checking the original.
        ord
    });

    // Separate correction for missing-field ordering (stable, sorts last).
    // Re-sort stably so that missing-field entries move to the end while
    // preserving the field-based order of the rest.
    keyed.sort_by(|a, b| {
        let a_has = has_field(&a.2, field);
        let b_has = has_field(&b.2, field);
        match (a_has, b_has) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });

    if reverse {
        keyed.reverse();
    }

    let values: Vec<Value> = keyed.into_iter().map(|(_, _, v)| v).collect();
    Ok(Array::from(values))
}

/// Whether a Value is an Obj containing the given field.
fn has_field(v: &Value, field: &str) -> bool {
    matches!(v, Value::Obj(obj) if obj.get_str(field).is_some())
}

/// Sort text lines by a 1-based column number (Plan 003, -k mode).
///
/// `sep` of None means whitespace-separated fields; a single char is used
/// verbatim as the delimiter.
pub fn sort_by_column(
    text: &str,
    col: usize,
    sep: Option<&str>,
    reverse: bool,
    numeric: bool,
) -> String {
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();

    let key_of = |line: &str| -> String {
        let field = match sep {
            Some(s) => line.split(s.as_bytes()[0] as char).nth(col.saturating_sub(1)),
            None => line.split_whitespace().nth(col.saturating_sub(1)),
        };
        field.unwrap_or("").to_string()
    };

    lines.sort_by(|a, b| {
        let ka = key_of(a);
        let kb = key_of(b);
        let cmp = if numeric {
            match (extract_leading_number(&ka), extract_leading_number(&kb)) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => ka.cmp(&kb),
            }
        } else {
            ka.cmp(&kb)
        };
        if reverse {
            cmp.reverse()
        } else {
            cmp
        }
    });

    lines.join("\n")
}

/// Compare two strings numerically (leading numeric prefix)
fn compare_numeric(a: &str, b: &str, ignore_case: bool) -> std::cmp::Ordering {
    let na = extract_leading_number(a);
    let nb = extract_leading_number(b);

    match (na, nb) {
        (Some(va), Some(vb)) => va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal),
        _ => {
            if ignore_case {
                a.to_lowercase().cmp(&b.to_lowercase())
            } else {
                a.cmp(b)
            }
        }
    }
}

/// Extract leading numeric value from a string
fn extract_leading_number(s: &str) -> Option<f64> {
    let trimmed = s.trim_start();
    let num_str: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num_str.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_basic() {
        let text = "cherry\napple\nbanana";
        assert_eq!(sort_lines(text, false, false, false, false), "apple\nbanana\ncherry");
    }

    #[test]
    fn test_sort_reverse() {
        let text = "apple\nbanana\ncherry";
        assert_eq!(sort_lines(text, true, false, false, false), "cherry\nbanana\napple");
    }

    #[test]
    fn test_sort_numeric() {
        let text = "10\n2\n1\n20\n3";
        assert_eq!(sort_lines(text, false, true, false, false), "1\n2\n3\n10\n20");
    }

    #[test]
    fn test_sort_unique() {
        let text = "apple\nbanana\napple\ncherry\nbanana";
        assert_eq!(sort_lines(text, false, false, true, false), "apple\nbanana\ncherry");
    }

    #[test]
    fn test_sort_ignore_case() {
        let text = "Banana\napple\nCherry";
        assert_eq!(sort_lines(text, false, false, false, true), "apple\nBanana\nCherry");
    }

    #[test]
    fn test_extract_leading_number() {
        assert_eq!(extract_leading_number("42abc"), Some(42.0));
        assert_eq!(extract_leading_number("abc"), None);
        assert_eq!(extract_leading_number("3.14"), Some(3.14));
    }

    // ---- Plan 003: sort by field ----

    use auto_val::{Array, Obj};

    fn rec(pairs: &[(&str, &str)]) -> Value {
        let mut o = Obj::new();
        for (k, v) in pairs {
            o.set(*k, Value::str(*v));
        }
        Value::Obj(o)
    }

    #[test]
    fn sort_by_field_numeric_ascending() {
        let arr = Array::from(vec![
            rec(&[("name", "alice"), ("age", "30")]),
            rec(&[("name", "bob"), ("age", "25")]),
            rec(&[("name", "carol"), ("age", "35")]),
        ]);
        let sorted = sort_array_by_field(&arr, "age", false, true).unwrap();
        let ages: Vec<String> = sorted
            .iter()
            .map(|v| v.as_obj().get_str("age").unwrap().to_string())
            .collect();
        assert_eq!(ages, vec!["25", "30", "35"]);
    }

    #[test]
    fn sort_by_field_numeric_descending() {
        let arr = Array::from(vec![
            rec(&[("name", "alice"), ("age", "30")]),
            rec(&[("name", "bob"), ("age", "25")]),
            rec(&[("name", "carol"), ("age", "35")]),
        ]);
        let sorted = sort_array_by_field(&arr, "age", true, true).unwrap();
        let ages: Vec<String> = sorted
            .iter()
            .map(|v| v.as_obj().get_str("age").unwrap().to_string())
            .collect();
        assert_eq!(ages, vec!["35", "30", "25"]);
    }

    #[test]
    fn sort_by_field_string_lexical() {
        let arr = Array::from(vec![
            rec(&[("name", "carol")]),
            rec(&[("name", "alice")]),
            rec(&[("name", "bob")]),
        ]);
        let sorted = sort_array_by_field(&arr, "name", false, false).unwrap();
        let names: Vec<String> = sorted
            .iter()
            .map(|v| v.as_obj().get_str("name").unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["alice", "bob", "carol"]);
    }

    #[test]
    fn sort_by_field_missing_field_sorts_last() {
        // Records lacking the field go to the end (rule 5).
        let arr = Array::from(vec![
            rec(&[("name", "alice"), ("age", "30")]),
            rec(&[("name", "noage")]), // no age field
            rec(&[("name", "bob"), ("age", "25")]),
        ]);
        let sorted = sort_array_by_field(&arr, "age", false, true).unwrap();
        let names: Vec<String> = sorted
            .iter()
            .map(|v| v.as_obj().get_str("name").unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["bob", "alice", "noage"]);
    }

    #[test]
    fn sort_by_column_text_mode() {
        let text = "alice,30\nbob,25\ncarol,35\n";
        let sorted = sort_by_column(text, 2, Some(","), false, true);
        // sorted by 2nd column numerically: bob(25), alice(30), carol(35)
        let lines: Vec<&str> = sorted.lines().collect();
        assert_eq!(lines, vec!["bob,25", "alice,30", "carol,35"]);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use crate::shell::Shell;

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

    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_temp_csv(name: &str, contents: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("ash_sort_test_{}_{}", pid, n));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn open_pipe_sort_w_age_ascending() {
        let path = write_temp_csv(
            "data.csv",
            "name,age\nAlice,30\nBob,25\nCarol,35\nDavid,28\n",
        );
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("open {} | sort -w age", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        // age ascending: Bob(25), David(28), Alice(30), Carol(35)
        let bob_pos = plain.find("Bob").unwrap_or(usize::MAX);
        let david_pos = plain.find("David").unwrap_or(usize::MAX);
        let alice_pos = plain.find("Alice").unwrap_or(usize::MAX);
        let carol_pos = plain.find("Carol").unwrap_or(usize::MAX);
        assert!(bob_pos < david_pos, "Bob(25) before David(28): {plain}");
        assert!(david_pos < alice_pos, "David(28) before Alice(30): {plain}");
        assert!(alice_pos < carol_pos, "Alice(30) before Carol(35): {plain}");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn open_pipe_sort_w_age_reverse() {
        let path = write_temp_csv(
            "data.csv",
            "name,age\nAlice,30\nBob,25\nCarol,35\n",
        );
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("open {} | sort -w age -r", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        // age descending: Carol(35), Alice(30), Bob(25)
        let carol_pos = plain.find("Carol").unwrap_or(usize::MAX);
        let alice_pos = plain.find("Alice").unwrap_or(usize::MAX);
        let bob_pos = plain.find("Bob").unwrap_or(usize::MAX);
        assert!(carol_pos < alice_pos, "Carol first when -r: {plain}");
        assert!(alice_pos < bob_pos, "Bob last when -r: {plain}");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn sort_k_column_text_mode_integration() {
        let path = write_temp_csv(
            "data.csv",
            "name,age\nAlice,30\nBob,25\nCarol,35\n",
        );
        let mut shell = Shell::new();
        let out = shell
            .execute(&format!("sort -k 2 -t , -n {}", path))
            .unwrap_or(None)
            .unwrap_or_default();
        let plain = strip_ansi(&out);
        // by 2nd column numeric: Bob,25 / Alice,30 / Carol,35
        let bob_pos = plain.find("Bob").unwrap_or(usize::MAX);
        let alice_pos = plain.find("Alice").unwrap_or(usize::MAX);
        let carol_pos = plain.find("Carol").unwrap_or(usize::MAX);
        assert!(bob_pos < alice_pos, "{plain}");
        assert!(alice_pos < carol_pos, "{plain}");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn sort_w_and_k_mutually_exclusive_errors() {
        let mut shell = Shell::new();
        let path = write_temp_csv("data.csv", "name,age\nAlice,30\n");
        let result = shell.execute(&format!("sort -w age -k 2 {}", path));
        assert!(result.is_err(), "-w and -k together should error");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn sort_w_on_text_input_errors() {
        let mut shell = Shell::new();
        // piping plain text into sort -w should error (not a list of records)
        let result = shell.execute("echo hello | sort -w age");
        assert!(result.is_err(), "sort -w on text input should error");
    }
}
