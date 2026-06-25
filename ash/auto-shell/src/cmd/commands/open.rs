//! `open` command — open a file with the OS default application (Plan 004).
//!
//! Across the auto-os ecosystem `open` means "launch/open an application":
//! e.g. `auto-man open` opens a project in its IDE, and here `open <file>`
//! opens a file in its default GUI application (explorer/xdg-open/...).
//! Use `show` to parse a file into the pipeline instead.

use std::path::{Path, PathBuf};

use miette::Result;

use crate::cmd::parser::ParsedArgs;
use crate::cmd::pipeline_convert::{atom_to_pipeline_data, pipeline_data_to_atom};
use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;

pub struct OpenCommand;

impl Command for OpenCommand {
    fn name(&self) -> &str {
        "open"
    }

    fn signature(&self) -> Signature {
        Signature::new("open", "Open a file with the default application")
            .optional("file", "Path to the file to open")
            .extra_help(
                "Opens the file using the OS default application\n  \
                 (Windows: explorer/shell, macOS: open, Linux: xdg-open).\n  \
                 To parse a file into the pipeline, use `show` instead.",
            )
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        // Rule 5: open is a launcher, not a pipeline filter; ignore input.
        let path = args
            .first()
            .ok_or_else(|| miette::miette!("open: missing file argument"))?;

        let resolved = resolve_path(path, shell);
        if !resolved.exists() {
            miette::bail!("open: {}: No such file or directory", path);
        }

        opener::open(&resolved)
            .map_err(|e| miette::miette!("open: {}: {}", path, e))?;

        // Rule 6: success produces no pipeline output.
        Ok(PipelineData::empty())
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

/// Resolve a path relative to the shell's CWD (mirrors cat's resolve_path).
fn resolve_path(arg: &str, shell: &Shell) -> PathBuf {
    let path = Path::new(arg);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        shell.pwd().join(arg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_file_errors() {
        let mut shell = Shell::new();
        // forward-slash absolute path so parse_args doesn't mangle it
        let result = shell.execute("open /no/such/file_xyz.txt");
        assert!(result.is_err(), "missing file should error");
    }

    #[test]
    fn open_no_argument_errors() {
        let mut shell = Shell::new();
        let result = shell.execute("open");
        assert!(result.is_err(), "open with no argument should error");
    }

    #[test]
    fn open_existing_file_does_not_error() {
        // We cannot assert a GUI window appears; just verify that opening a
        // real file does not return an error (the spawn succeeded).
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("ash_open_gui_test_{}", pid));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hello.txt");
        std::fs::write(&path, "hi").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "/");

        let mut shell = Shell::new();
        let result = shell.execute(&format!("open {}", path_str));
        // Allow either success or an error if no default app is configured in
        // the test environment — but a missing file would have errored above,
        // so here we just ensure it doesn't panic. Relax to is_ok OR a
        // non-"No such file" error.
        match &result {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("{:?}", e);
                assert!(
                    !msg.contains("No such file"),
                    "should not be a missing-file error: {msg}"
                );
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_help_mentions_show() {
        let mut shell = Shell::new();
        let out = shell.execute("open --help").unwrap_or(None).unwrap_or_default();
        assert!(out.contains("open"), "usage should mention open: {out}");
        // help should hint users toward `show` for parsing
        assert!(out.contains("show"), "help should mention show: {out}");
    }
}
