use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use auto_val::Value;
use miette::{IntoDiagnostic, Result};
use std::path::Path;

pub struct CutCommand;

impl Command for CutCommand {
    fn name(&self) -> &str {
        "cut"
    }

    fn signature(&self) -> Signature {
        Signature::new("cut", "Remove sections from each line of text")
            .optional("file", "File to read (default: stdin)")
            .flag_with_short("delimiter", 'd', "Field delimiter (default: TAB)")
            .flag_with_short("fields", 'f', "Field list (e.g., 1,3 or 1-3)")
            .option_with_short("characters", 'c', "Character list (e.g., 1-3)")
            .option_with_short("bytes", 'b', "Byte list (e.g., 2-)")
            .flag_with_short("only-delimited", 's', "Suppress lines without the delimiter")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        input: PipelineData,
        _shell: &mut Shell,
    ) -> Result<PipelineData> {
        let text = if let Some(path) = args.first() {
            std::fs::read_to_string(Path::new(path)).into_diagnostic()?
        } else {
            get_text(input)?
        };

        // Plan 006 P0-3: -c / -b / -f are mutually exclusive (POSIX).
        let modes = [
            args.has_flag("fields"),
            args.get_option("characters").is_some(),
            args.get_option("bytes").is_some(),
        ];
        if modes.iter().filter(|&&m| m).count() > 1 {
            miette::bail!("cut: only one of -f, -c, or -b may be specified");
        }

        // -c : characters (UTF-8 aware)
        if let Some(spec) = args.get_option("characters") {
            let ranges = parse_ranges(spec)?;
            return Ok(PipelineData::from_text(cut_characters(&text, &ranges, spec)));
        }

        // -b : bytes (UTF-8 char-boundary aware)
        if let Some(spec) = args.get_option("bytes") {
            let ranges = parse_ranges(spec)?;
            return Ok(PipelineData::from_text(cut_bytes(&text, &ranges, spec)));
        }

        // -f : fields (existing path, unchanged). -s only meaningful with -f.
        let only_delimited = args.has_flag("only-delimited");
        let delimiter = args.positional_or(1, "\t").to_string();
        let delim = if args.has_flag("delimiter") {
            args.positionals.get(1).map(|s| s.as_str()).unwrap_or("\t").to_string()
        } else {
            delimiter
        };

        let fields_spec = args
            .positionals
            .iter()
            .skip_while(|s| {
                // skip the file path (first positional) and any -d value
                s.as_str() == delim.as_str()
            })
            .find(|s| {
                s.contains(',') || s.contains('-') || s.chars().all(|c| c.is_ascii_digit())
            })
            .cloned();
        let spec = fields_spec.unwrap_or_else(|| "1".to_string());
        let field_indices = parse_field_list(&spec)?;

        let result = cut_fields(&text, &delim, &field_indices, only_delimited);
        Ok(PipelineData::from_text(result))
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let legacy_in = crate::cmd::pipeline_convert::atom_to_pipeline_data(input);
        let legacy_out = self.run(args, legacy_in, shell)?;
        let text = legacy_out.into_text();
        Ok(AtomPipeline::from_atom(Atom::new(Value::str(&text), AtomType::Text)))
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
        _ => miette::bail!("cut: input must be text"),
    }
}

/// A 1-based index range, possibly open at the end ("2-" → open_end) or
/// open at the start ("-3" → start=1).
#[derive(Debug, Clone, Copy)]
struct Range {
    start: usize,
    end: usize,   // inclusive; for open "N-", set to usize::MAX
}

/// Parse a spec like "1,3" / "1-3" / "2-" / "-3" / "1,3-5" into ranges.
pub fn parse_ranges(spec: &str) -> Result<Vec<Range>> {
    let mut ranges = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            miette::bail!("cut: invalid field/char/byte spec: {}", spec);
        }
        if let Some((s, e)) = part.split_once('-') {
            let start: usize = if s.is_empty() { 1 } else { s.parse().into_diagnostic()? };
            if start == 0 {
                miette::bail!("cut: indices must be >= 1");
            }
            let end = if e.is_empty() {
                usize::MAX // open range "N-"
            } else {
                let end: usize = e.parse().into_diagnostic()?;
                if end == 0 || end < start {
                    miette::bail!("cut: invalid range: {}", part);
                }
                end
            };
            ranges.push(Range { start, end });
        } else {
            let idx: usize = part.parse().into_diagnostic()?;
            if idx == 0 {
                miette::bail!("cut: index must be >= 1");
            }
            ranges.push(Range { start: idx, end: idx });
        }
    }
    Ok(ranges)
}

/// Parse a field spec into a flat sorted list of 1-based indices
/// (for the existing -f path). Open ranges expand to a sentinel start.
pub fn parse_field_list(spec: &str) -> Result<Vec<usize>> {
    let mut indices = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let (s, e) = part.split_once('-').unwrap();
            let start: usize = if s.is_empty() { 1 } else { s.parse().into_diagnostic()? };
            if start == 0 {
                miette::bail!("cut: field indices must be >= 1");
            }
            if e.is_empty() {
                // "N-" : keep start as a marker; cut_fields won't expand, but
                // for backward compat we just include start.
                indices.push(start);
            } else {
                let end: usize = e.parse().into_diagnostic()?;
                if end == 0 {
                    miette::bail!("cut: field indices must be >= 1");
                }
                for i in start..=end {
                    indices.push(i);
                }
            }
        } else {
            let idx: usize = part.parse().into_diagnostic()?;
            if idx == 0 {
                miette::bail!("cut: field index must be >= 1");
            }
            indices.push(idx);
        }
    }
    indices.sort();
    indices.dedup();
    Ok(indices)
}

/// Extract specified fields from each line. When `only_delimited` is true,
/// lines without the delimiter are suppressed (POSIX -s).
pub fn cut_fields(text: &str, delimiter: &str, field_indices: &[usize], only_delimited: bool) -> String {
    text.lines()
        .filter(|line| !(only_delimited && !line.contains(delimiter)))
        .map(|line| {
            let fields: Vec<&str> = line.split(delimiter).collect();
            field_indices
                .iter()
                .filter_map(|&idx| fields.get(idx - 1).copied())
                .collect::<Vec<&str>>()
                .join(delimiter)
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Whether a 1-based position `pos` is covered by any of the ranges.
fn in_ranges(pos: usize, ranges: &[Range]) -> bool {
    ranges.iter().any(|r| pos >= r.start && pos <= r.end)
}

/// Extract specified character positions from each line (UTF-8 aware).
pub fn cut_characters(text: &str, ranges: &[Range], _spec: &str) -> String {
    text.lines()
        .map(|line| {
            line.chars()
                .enumerate()
                .filter_map(|(i, c)| {
                    if in_ranges(i + 1, ranges) {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Extract specified byte positions, respecting UTF-8 char boundaries:
/// a byte position that would split a multi-byte char is skipped.
pub fn cut_bytes(text: &str, ranges: &[Range], _spec: &str) -> String {
    text.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let mut out: Vec<u8> = Vec::new();
            let mut pos = 0usize;
            while pos < bytes.len() {
                let next = next_boundary(line, pos);
                // pos is a 0-based byte offset; +1 makes it 1-based
                if in_ranges(pos + 1, ranges) {
                    out.extend_from_slice(&bytes[pos..next]);
                }
                pos = next;
            }
            String::from_utf8(out).unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Find the next UTF-8 char boundary after `pos`.
fn next_boundary(s: &str, pos: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = pos + 1;
    while i < bytes.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_field_list_single() {
        assert_eq!(parse_field_list("1").unwrap(), vec![1]);
    }

    #[test]
    fn test_parse_field_list_comma() {
        assert_eq!(parse_field_list("1,3").unwrap(), vec![1, 3]);
    }

    #[test]
    fn test_parse_field_list_range() {
        assert_eq!(parse_field_list("1-3").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_field_list_open_n_to_end() {
        // "2-" keeps 2 as a marker (cut_fields won't fully expand, but parse works)
        assert_eq!(parse_field_list("2-").unwrap(), vec![2]);
    }

    #[test]
    fn test_parse_field_list_open_start_to_m() {
        assert_eq!(parse_field_list("-3").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_cut_fields_basic() {
        let text = "one:two:three\nfour:five:six";
        assert_eq!(cut_fields(text, ":", &[1], false), "one\nfour");
    }

    #[test]
    fn test_cut_fields_only_delimited() {
        // -s suppresses lines without the delimiter
        let text = "a,b\ncd";
        assert_eq!(cut_fields(text, ",", &[1], true), "a");
    }

    // ---- Plan 006 P0-3: -c / -b ----

    #[test]
    fn test_cut_characters_range() {
        let r = parse_ranges("1-3").unwrap();
        assert_eq!(cut_characters("hello", &r, "1-3"), "hel");
    }

    #[test]
    fn test_cut_characters_open_end() {
        // cut -c2- → from char 2 to end
        let r = parse_ranges("2-").unwrap();
        assert_eq!(cut_characters("hello", &r, "2-"), "ello");
    }

    #[test]
    fn test_cut_characters_utf8() {
        // "héllo" chars: h,é,l,l,o → -c1-3 → "hél"
        let r = parse_ranges("1-3").unwrap();
        assert_eq!(cut_characters("héllo", &r, "1-3"), "hél");
    }

    #[test]
    fn test_cut_bytes_basic() {
        let r = parse_ranges("1-2").unwrap();
        assert_eq!(cut_bytes("hello", &r, "1-2"), "he");
    }

    #[test]
    fn test_cut_bytes_open_end() {
        // cut -b2- → from byte 2 to end
        let r = parse_ranges("2-").unwrap();
        assert_eq!(cut_bytes("hello", &r, "2-"), "ello");
    }

    #[test]
    fn test_cut_bytes_utf8_skips_mid_char() {
        // "héllo": h(1) é(bytes 2,3,4) l(5) l(6) o(7) — char boundaries at 1,2,5,6,7
        // Requesting byte position 3 lands inside 'é' (not a boundary) → skipped.
        let r = parse_ranges("1,3,5").unwrap();
        assert_eq!(cut_bytes("héllo", &r, "1,3,5"), "hl");
    }
}
