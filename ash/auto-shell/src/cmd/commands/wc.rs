use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use auto_val::{Value, Obj, Array};
use miette::Result;

pub struct WcCommand;

impl Command for WcCommand {
    fn name(&self) -> &str {
        "wc"
    }

    fn signature(&self) -> Signature {
        Signature::new("wc", "Count lines, words, bytes, and characters")
            .optional("file", "File(s) to count (default: count from pipeline)")
            .flag_with_short("lines", 'l', "Count lines")
            .flag_with_short("words", 'w', "Count words")
            .flag_with_short("bytes", 'c', "Count bytes")
            .flag_with_short("chars", 'm', "Count characters (Unicode-aware)")
    }

    fn run(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        // Extract flags
        let count_lines = args.has_flag("lines");
        let count_words = args.has_flag("words");
        let count_bytes = args.has_flag("bytes");
        let count_chars = args.has_flag("chars");

        // If no flags specified, count all
        let count_all = !count_lines && !count_words && !count_bytes && !count_chars;

        // If file arguments are provided, read and count them (POSIX wc behavior).
        // This takes precedence over pipeline input.
        if !args.positionals.is_empty() {
            return wc_files(args, shell, count_lines, count_words, count_bytes, count_chars, count_all);
        }

        match input {
            PipelineData::Value(Value::Array(arr)) => {
                // For counting lines (-l), or counting all with non-text array, count array elements
                if count_lines && !count_words && !count_bytes && !count_chars {
                    // Special case: just counting elements (like "ls | wc -l")
                    let mut obj = Obj::new();
                    obj.set("lines", Value::Int(arr.len() as i32));
                    Ok(PipelineData::from_value(Value::Obj(obj)))
                } else if count_all {
                    // When counting all and array contains objects (like ls output),
                    // just count the array elements
                    let mut obj = Obj::new();
                    obj.set("lines", Value::Int(arr.len() as i32));
                    Ok(PipelineData::from_value(Value::Obj(obj)))
                } else {
                    // Count text content in each element
                    let mut results = Vec::new();

                    for (index, item) in arr.iter().enumerate() {
                        if let Some(text) = extract_text(item) {
                            let mut obj = Obj::new();

                            if count_lines || count_all {
                                let lines = count_lines_in_text(&text);
                                obj.set("lines", Value::Int(lines as i32));
                            }

                            if count_words || count_all {
                                let words = count_words_in_text(&text);
                                obj.set("words", Value::Int(words as i32));
                            }

                            if count_bytes || count_all {
                                let bytes = text.len();
                                obj.set("bytes", Value::Int(bytes as i32));
                            }

                            if count_chars || count_all {
                                let chars = text.chars().count();
                                obj.set("chars", Value::Int(chars as i32));
                            }

                            // Add index or filename if available
                            if let Value::Str(filename) = item {
                                obj.set("file", Value::str(filename.as_ref()));
                            } else {
                                obj.set("index", Value::Int(index as i32));
                            }

                            results.push(Value::Obj(obj));
                        }
                    }

                    // Add total if counting multiple items
                    if results.len() > 1 {
                        let mut total = Obj::new();
                        let mut total_lines = 0;
                        let mut total_words = 0;
                        let mut total_bytes = 0;
                        let mut total_chars = 0;

                        for result in &results {
                            if let Value::Obj(obj) = result {
                                if let Some(Value::Int(n)) = obj.get("lines") {
                                    total_lines += n;
                                }
                                if let Some(Value::Int(n)) = obj.get("words") {
                                    total_words += n;
                                }
                                if let Some(Value::Int(n)) = obj.get("bytes") {
                                    total_bytes += n;
                                }
                                if let Some(Value::Int(n)) = obj.get("chars") {
                                    total_chars += n;
                                }
                            }
                        }

                        if count_lines || count_all {
                            total.set("lines", Value::Int(total_lines));
                        }
                        if count_words || count_all {
                            total.set("words", Value::Int(total_words));
                        }
                        if count_bytes || count_all {
                            total.set("bytes", Value::Int(total_bytes));
                        }
                        if count_chars || count_all {
                            total.set("chars", Value::Int(total_chars));
                        }
                        total.set("file", Value::str("total"));

                        results.push(Value::Obj(total));
                    }

                    if results.is_empty() {
                        // If no text elements found, just return element count
                        let mut obj = Obj::new();
                        obj.set("lines", Value::Int(arr.len() as i32));
                        Ok(PipelineData::from_value(Value::Obj(obj)))
                    } else {
                        Ok(PipelineData::from_value(Value::Array(Array::from(results))))
                    }
                }
            }
            PipelineData::Value(Value::Str(text)) => {
                // Count single string
                let mut obj = Obj::new();

                if count_lines || count_all {
                    let lines = count_lines_in_text(text.as_ref());
                    obj.set("lines", Value::Int(lines as i32));
                }

                if count_words || count_all {
                    let words = count_words_in_text(text.as_ref());
                    obj.set("words", Value::Int(words as i32));
                }

                if count_bytes || count_all {
                    let bytes = text.len();
                    obj.set("bytes", Value::Int(bytes as i32));
                }

                if count_chars || count_all {
                    let chars = text.as_ref().chars().count();
                    obj.set("chars", Value::Int(chars as i32));
                }

                Ok(PipelineData::from_value(Value::Obj(obj)))
            }
            PipelineData::Value(Value::Obj(obj)) => {
                // If it's an object with a "content" field, count that
                if let Some(Value::Str(content)) = obj.get("content") {
                    let mut result = Obj::new();

                    if count_lines || count_all {
                        let lines = count_lines_in_text(content.as_ref());
                        result.set("lines", Value::Int(lines as i32));
                    }

                    if count_words || count_all {
                        let words = count_words_in_text(content.as_ref());
                        result.set("words", Value::Int(words as i32));
                    }

                    if count_bytes || count_all {
                        let bytes = content.len();
                        result.set("bytes", Value::Int(bytes as i32));
                    }

                    if count_chars || count_all {
                        let chars = content.as_ref().chars().count();
                        result.set("chars", Value::Int(chars as i32));
                    }

                    // Copy filename if present
                    if let Some(filename) = obj.get("name") {
                        result.set("file", filename.clone());
                    }

                    Ok(PipelineData::from_value(Value::Obj(result)))
                } else {
                    miette::bail!("wc: cannot count non-text object");
                }
            }
            PipelineData::Text(text) => {
                // Count plain text input
                let mut obj = Obj::new();

                if count_lines || count_all {
                    let lines = count_lines_in_text(&text);
                    obj.set("lines", Value::Int(lines as i32));
                }

                if count_words || count_all {
                    let words = count_words_in_text(&text);
                    obj.set("words", Value::Int(words as i32));
                }

                if count_bytes || count_all {
                    let bytes = text.len();
                    obj.set("bytes", Value::Int(bytes as i32));
                }

                if count_chars || count_all {
                    let chars = text.chars().count();
                    obj.set("chars", Value::Int(chars as i32));
                }

                Ok(PipelineData::from_value(Value::Obj(obj)))
            }
            PipelineData::Value(_) => {
                miette::bail!("wc: input must be text, string, or array of texts");
            }
        }
    }

    fn run_atom(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let legacy_in = crate::cmd::pipeline_convert::atom_to_pipeline_data(input);
        let legacy_out = self.run(args, legacy_in, shell)?;
        let value = match legacy_out {
            PipelineData::Value(v) => v,
            PipelineData::Text(s) => Value::str(&s),
        };
        Ok(AtomPipeline::from_atom(Atom::new(value, AtomType::CountResult)))
    }
}

/// Count lines, words, bytes, chars for one or more files (POSIX wc behavior).
/// Called when file arguments are given (e.g., `wc -l shell.rs` or `wc file1 file2`).
fn wc_files(
    args: &crate::cmd::parser::ParsedArgs,
    shell: &mut Shell,
    count_lines: bool,
    count_words: bool,
    count_bytes: bool,
    count_chars: bool,
    count_all: bool,
) -> Result<PipelineData> {
    let mut results: Vec<Value> = Vec::new();
    let mut total_lines = 0i32;
    let mut total_words = 0i32;
    let mut total_bytes = 0i32;
    let mut total_chars = 0i32;

    for file_arg in &args.positionals {
        let path = match shell.resolve_path(file_arg, false) {
            Ok(p) => p,
            Err(e) => {
                miette::bail!("wc: {}: {}", file_arg, e);
            }
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                miette::bail!("wc: {}: {}", file_arg, e);
            }
        };

        let lines = count_lines_in_text(&content) as i32;
        let words = count_words_in_text(&content) as i32;
        let bytes = content.len() as i32;
        let chars = content.chars().count() as i32;

        total_lines += lines;
        total_words += words;
        total_bytes += bytes;
        total_chars += chars;

        let mut obj = Obj::new();
        if count_lines || count_all {
            obj.set("lines", Value::Int(lines));
        }
        if count_words || count_all {
            obj.set("words", Value::Int(words));
        }
        if count_bytes || count_all {
            obj.set("bytes", Value::Int(bytes));
        }
        if count_chars || count_all {
            obj.set("chars", Value::Int(chars));
        }
        obj.set("file", Value::str(file_arg));
        results.push(Value::Obj(obj));
    }

    // If multiple files, add a "total" row (POSIX behavior).
    if args.positionals.len() > 1 {
        let mut total = Obj::new();
        if count_lines || count_all {
            total.set("lines", Value::Int(total_lines));
        }
        if count_words || count_all {
            total.set("words", Value::Int(total_words));
        }
        if count_bytes || count_all {
            total.set("bytes", Value::Int(total_bytes));
        }
        if count_chars || count_all {
            total.set("chars", Value::Int(total_chars));
        }
        total.set("file", Value::str("total"));
        results.push(Value::Obj(total));
    }

    // Single file → return a single object; multiple → array.
    if results.len() == 1 {
        Ok(PipelineData::from_value(results.into_iter().next().unwrap()))
    } else {
        Ok(PipelineData::from_value(Value::Array(Array::from(results))))
    }
}

/// Extract text content from a Value
fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

/// Count lines in text (number of newlines + 1 if non-empty)
fn count_lines_in_text(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

/// Count words in text (whitespace-separated sequences)
fn count_words_in_text(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_lines() {
        let text = "line1\nline2\nline3";
        assert_eq!(count_lines_in_text(text), 3);

        let text = "";
        assert_eq!(count_lines_in_text(text), 0);

        let text = "single line";
        assert_eq!(count_lines_in_text(text), 1);
    }

    #[test]
    fn test_count_words() {
        let text = "hello world test";
        assert_eq!(count_words_in_text(text), 3);

        let text = "  extra  spaces  ";
        assert_eq!(count_words_in_text(text), 2);

        let text = "";
        assert_eq!(count_words_in_text(text), 0);
    }

    #[test]
    fn test_extract_text() {
        let value = Value::str("hello world");
        assert_eq!(extract_text(&value), Some("hello world".to_string()));

        let value = Value::Int(42);
        assert_eq!(extract_text(&value), None);
    }

    #[test]
    fn test_wc_counts_array_elements() {
        // Test that wc -l counts array elements (like ls output)
        let wc = WcCommand;
        let mut flags = std::collections::HashMap::new();
        flags.insert("lines".to_string(), true);

        let args = crate::cmd::parser::ParsedArgs {
            positionals: vec![],
            flags,
            named: std::collections::HashMap::new(),
            ..Default::default()
        };

        // Create an array like ls would return
        let mut arr = Array::new();
        let mut obj1 = Obj::new();
        obj1.set("name", Value::str("file1.txt"));
        let mut obj2 = Obj::new();
        obj2.set("name", Value::str("file2.txt"));
        let mut obj3 = Obj::new();
        obj3.set("name", Value::str("file3.txt"));

        arr.push(Value::Obj(obj1));
        arr.push(Value::Obj(obj2));
        arr.push(Value::Obj(obj3));

        let input = PipelineData::from_value(Value::Array(arr));
        let result = wc.run(&args, input, &mut Shell::new()).unwrap();

        if let PipelineData::Value(Value::Obj(obj)) = result {
            assert_eq!(obj.get("lines"), Some(Value::Int(3)));
        } else {
            panic!("Expected Value::Obj with lines count");
        }
    }

    #[test]
    fn test_wc_counts_file() {
        // wc -l <file> should read the file and count its lines.
        let dir = std::env::temp_dir().join(format!("ash-wc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let test_file = dir.join("test.txt");
        std::fs::write(&test_file, "line1\nline2\nline3\n").unwrap();

        let wc = WcCommand;
        let mut flags = std::collections::HashMap::new();
        flags.insert("lines".to_string(), true);
        let args = crate::cmd::parser::ParsedArgs {
            positionals: vec![test_file.to_string_lossy().into_owned()],
            flags,
            ..Default::default()
        };

        let mut shell = Shell::new();
        // Use absolute path so resolve_path works regardless of cwd.
        let result = wc.run(&args, PipelineData::empty(), &mut shell).unwrap();

        if let PipelineData::Value(Value::Obj(obj)) = result {
            let lines = obj.get("lines");
            assert_eq!(lines, Some(Value::Int(3)), "should count 3 lines in the file");
        } else {
            panic!("Expected Value::Obj, got {:?}", result);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
