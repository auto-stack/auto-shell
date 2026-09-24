//! du command - Display disk usage
//!
//! Estimates file and directory space usage with optional human-readable
//! output and depth limiting.

use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use crate::cmd::ShellContext;
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use auto_val::{Value, Obj, Array};
use miette::Result;
use std::path::{Path, PathBuf};

pub struct DuCommand;

impl Command for DuCommand {
    fn name(&self) -> &str {
        "du"
    }

    fn signature(&self) -> Signature {
        Signature::new("du", "Display disk usage")
            .optional("path", "Path to measure (default: current directory)")
            .flag_with_short("summarize", 's', "Display only total")
            .flag_with_short("human-readable", 'h', "Print sizes in KB/MB/GB")
            .flag_with_short("depth", 'd', "Max display depth")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut dyn ShellContext,
    ) -> Result<PipelineData> {
        let path_arg = args.first().unwrap_or(".");
        let summarize = args.has_flag("summarize");
        let human = args.has_flag("human-readable");

        let max_depth: Option<usize> = args.named.get("depth")
            .and_then(|s| s.parse().ok());

        let root = if Path::new(path_arg).is_absolute() {
            PathBuf::from(path_arg)
        } else {
            shell.pwd().join(path_arg)
        };

        if !root.exists() {
            miette::bail!("du: cannot access '{}': No such file or directory", path_arg);
        }

        let (mut entries, root_bytes) = build_entries(&root, summarize, max_depth, human)?;

        // Sort by size descending
        entries.sort_by(|a, b| {
            let sa = extract_size(a);
            let sb = extract_size(b);
            sb.cmp(&sa)
        });

        // PLAN-082 T-05 (AC-09): total == the root row ("."), which already
        // contains every subtree. The old sum() over ALL entries re-added the
        // subdirectory rows on top of the root row (≈2× actual; 2026-09-24
        // 实测 `.`=16.08GB / total=32.17GB — the contradiction that sent an
        // AI model into a retry loop, see PLAN-083 §4).
        let mut total_obj = Obj::new();
        total_obj.set("path", Value::str("total"));
        total_obj.set("size", Value::str(&format_size(root_bytes, human)));
        total_obj.set("bytes", Value::Str(root_bytes.to_string().into()));
        entries.push(Value::Obj(Box::new(total_obj)));

        Ok(PipelineData::from_value(Value::Array(Array::from(entries))))
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: AtomPipeline,
        shell: &mut dyn ShellContext,
    ) -> Result<AtomPipeline> {
        let legacy_in = crate::cmd::pipeline_convert::atom_to_pipeline_data(input);
        let legacy_out = self.run(args, legacy_in, shell)?;
        let value = match legacy_out {
            PipelineData::Value(v) => v,
            PipelineData::Text(s) => Value::str(&s),
        };
        Ok(AtomPipeline::from_atom(Atom::new(value, AtomType::Table)))
    }
}

/// Extract the byte size from an entry Value.
fn extract_size(v: &Value) -> u64 {
    if let Value::Obj(obj) = v {
        if let Some(Value::Str(s)) = obj.get("bytes") {
            s.parse::<u64>().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    }
}

/// Build the per-subdirectory/root entry rows and return them together with
/// the root's total bytes (PLAN-082 T-05: extracted so the total ==
/// root-row invariant is unit-testable without a ShellContext).
fn build_entries(
    root: &Path,
    summarize: bool,
    max_depth: Option<usize>,
    human: bool,
) -> Result<(Vec<Value>, u64)> {
    let mut entries = Vec::new();
    let root_bytes = compute_du(root, root, 0, summarize, max_depth, human, &mut entries)?;
    Ok((entries, root_bytes))
}

/// Recursively compute disk usage. Returns the bytes of `current`'s subtree
/// (the root total at depth 0 — du's `total` row mirrors this value).
fn compute_du(
    root: &Path,
    current: &Path,
    depth: usize,
    summarize: bool,
    max_depth: Option<usize>,
    human: bool,
    entries: &mut Vec<Value>,
) -> Result<u64> {
    if let Some(max) = max_depth {
        if depth > max {
            return Ok(0);
        }
    }

    let mut total_bytes: u64 = 0;

    if current.is_dir() {
        let dir_entries = match std::fs::read_dir(current) {
            Ok(e) => e,
            Err(_) => return Ok(0),
        };

        for entry in dir_entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();

            let size = if path.is_dir() {
                let mut subdir_bytes: u64 = 0;
                compute_du_inner(&path, &mut subdir_bytes)?;
                subdir_bytes
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            };

            total_bytes += size;

            // Add subdirectory entry unless summarize mode
            if !summarize && path.is_dir() {
                let rel = pathdiff(root, &path);
                let mut obj = Obj::new();
                obj.set("path", Value::str(&rel));
                obj.set("size", Value::str(&format_size(size, human)));
                obj.set("bytes", Value::Str(size.to_string().into()));
                entries.push(Value::Obj(Box::new(obj)));
            }
        }
    } else {
        total_bytes = std::fs::metadata(current).map(|m| m.len()).unwrap_or(0);
    }

    // Add the current entry (root level)
    if depth == 0 {
        let rel = pathdiff(root, current);
        let mut obj = Obj::new();
        obj.set("path", Value::str(&rel));
        obj.set("size", Value::str(&format_size(total_bytes, human)));
        obj.set("bytes", Value::Str(total_bytes.to_string().into()));
        entries.push(Value::Obj(Box::new(obj)));
    }

    Ok(total_bytes)
}

/// Inner helper: accumulate total bytes for a subtree.
fn compute_du_inner(path: &Path, total: &mut u64) -> Result<()> {
    if path.is_file() {
        *total += std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        return Ok(());
    }

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let child = entry.path();
        if child.is_dir() {
            compute_du_inner(&child, total)?;
        } else {
            *total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    Ok(())
}

/// Format bytes as human-readable size.
fn format_size(bytes: u64, human: bool) -> String {
    if !human {
        return bytes.to_string();
    }

    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// Compute relative path string.
fn pathdiff(root: &Path, target: &Path) -> String {
    match target.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => target.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_du_command_name() {
        let cmd = DuCommand;
        assert_eq!(cmd.name(), "du");
    }

    #[test]
    fn test_format_size_plain() {
        assert_eq!(format_size(1024, false), "1024");
    }

    #[test]
    fn test_format_size_human() {
        assert_eq!(format_size(0, true), "0B");
        assert_eq!(format_size(512, true), "512B");
        assert!(format_size(1024, true).contains("KB"));
        assert!(format_size(1048576, true).contains("MB"));
        assert!(format_size(1073741824, true).contains("GB"));
    }

    // ── PLAN-082 T-05 (AC-09): total == root row, never 2× ────────────

    fn temp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ash-du-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
        std::fs::write(dir.join("file_a"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("sub/file_b"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.join("sub/deep/file_c"), vec![0u8; 5000]).unwrap();
        dir
    }

    fn row_bytes(entries: &[Value], path: &str) -> u64 {
        entries
            .iter()
            .find_map(|v| {
                let obj = match v {
                    Value::Obj(o) => o,
                    _ => return None,
                };
                match (obj.get("path"), obj.get("bytes")) {
                    (Some(Value::Str(p)), Some(Value::Str(b))) if p.as_str() == path => {
                        b.parse::<u64>().ok()
                    }
                    _ => None,
                }
            })
            .unwrap_or(u64::MAX)
    }

    #[test]
    fn du_total_equals_root_row_multilevel() {
        let dir = temp_tree("multi");
        let (entries, root_bytes) =
            build_entries(&dir, false, None, false).expect("build_entries");
        // Root row "." already contains the whole subtree; the sub row is the
        // nested files only. Old bug: total = sum(all rows) = 9000 + 8000.
        assert_eq!(root_bytes, 9000);
        assert_eq!(row_bytes(&entries, "."), 9000);
        assert_eq!(row_bytes(&entries, "sub"), 8000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn du_total_empty_dir() {
        let dir = std::env::temp_dir().join(format!("ash-du-test-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (entries, root_bytes) =
            build_entries(&dir, false, None, false).expect("build_entries");
        assert_eq!(root_bytes, 0);
        assert_eq!(row_bytes(&entries, "."), 0);
        assert_eq!(entries.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn du_summarize_yields_only_root_row() {
        let dir = temp_tree("sum");
        let (entries, root_bytes) =
            build_entries(&dir, true, None, false).expect("build_entries");
        assert_eq!(root_bytes, 9000);
        assert_eq!(entries.len(), 1, "summarize keeps only the root row");
        assert_eq!(row_bytes(&entries, "."), 9000);
        std::fs::remove_dir_all(&dir).ok();
    }
}
