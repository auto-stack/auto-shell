//! rm command - Remove files and directories
//!
//! Provides cross-platform file and directory removal.

use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;
use ash_core::pipeline::AtomPipeline;
use auto_val::Value;
use miette::{IntoDiagnostic, Result};
use std::fs;
use std::path::Path;

pub struct RmCommand;

impl Command for RmCommand {
    fn name(&self) -> &str {
        "rm"
    }

    fn signature(&self) -> Signature {
        Signature::new("rm", "Remove files and directories")
            .required("path", "Path to remove")
            .flag_with_short("recursive", 'r', "Remove directories and their contents recursively")
            .flag_with_short("recursive-upper", 'R', "Remove recursively (POSIX standard form of -r)")
            .flag_with_short("force", 'f', "Ignore nonexistent files and arguments")
            .flag_with_short("interactive", 'i', "Prompt (refuse, in non-interactive) before each removal")
            .flag_with_short("verbose", 'v', "Show what files are being removed")
    }

    fn run(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        if args.positionals.is_empty() {
            miette::bail!("rm: missing operand");
        }

        let recursive = args.has_flag("recursive") || args.has_flag("recursive-upper");
        let force = args.has_flag("force");
        let verbose = args.has_flag("verbose");

        let mut removed_count = 0;
        let mut errors = Vec::new();

        for arg in &args.positionals {
            // Plan 009: resolve via shell (honors --sandbox / --read-only).
            let target_path = match shell.resolve_path(arg, true) {
                Ok(p) => p,
                Err(e) => {
                    if !force {
                        errors.push(format!("{}: {}", arg, e));
                    }
                    continue;
                }
            };

            let result = if target_path.is_dir() {
                if recursive {
                    remove_dir(&target_path, verbose)
                } else {
                    miette::bail!("rm: cannot remove '{}': Is a directory", arg);
                }
            } else {
                remove_file(&target_path, verbose)
            };

            match result {
                Ok(count) => removed_count += count,
                Err(e) => {
                    if !force {
                        errors.push(format!("{}: {}", arg, e));
                    }
                }
            }
        }

        // Return summary as Value
        let mut result_obj = auto_val::Obj::new();
        result_obj.set("files_removed", Value::I64(removed_count as i64));

        if !errors.is_empty() {
            let error_values: Vec<Value> = errors
                .iter()
                .map(|e| Value::str(e))
                .collect();
            result_obj.set("errors", Value::from(error_values));
            result_obj.set("success", Value::Bool(false));
        } else {
            result_obj.set("success", Value::Bool(true));
        }

        Ok(PipelineData::from_value(Value::Obj(result_obj)))
    }

    fn run_atom(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let legacy = self.run(args, PipelineData::empty(), shell)?;
        Ok(crate::cmd::pipeline_convert::pipeline_data_to_atom(legacy))
    }
}

/// Remove a single file
fn remove_file(path: &Path, verbose: bool) -> Result<usize> {
    if verbose {
        eprintln!("removing '{}'", path.display());
    }

    fs::remove_file(path).into_diagnostic()?;
    Ok(1)
}

/// Remove a directory recursively
fn remove_dir(path: &Path, verbose: bool) -> Result<usize> {
    let mut count = 0;

    // First, remove all contents
    for entry in fs::read_dir(path).into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let entry_path = entry.path();

        // Critical: check for symlinks BEFORE is_dir(). A symlink to a
        // directory returns true from is_dir(), but we must remove the link
        // itself (remove_file), NOT recurse into its target — otherwise we'd
        // delete the linked content and fail to remove the link, leaving the
        // directory non-empty. This is especially common with pnpm-style
        // node_modules where package dirs are symlinks into .pnpm/.
        let is_symlink = entry_path.is_symlink() || fs::symlink_metadata(&entry_path).map(|m| m.file_type().is_symlink()).unwrap_or(false);

        count += if is_symlink {
            // Remove the symlink itself (works for both file and dir symlinks)
            if verbose {
                eprintln!("removing symlink '{}'", entry_path.display());
            }
            // remove_file works for symlinks to files; for symlinks to dirs
            // on Windows we need remove_dir (which removes the link, not target)
            if fs::remove_file(&entry_path).is_err() {
                fs::remove_dir(&entry_path).into_diagnostic()?;
            }
            1
        } else if entry_path.is_dir() {
            remove_dir(&entry_path, verbose)?
        } else {
            if verbose {
                eprintln!("removing '{}'", entry_path.display());
            }
            fs::remove_file(&entry_path).into_diagnostic()?;
            1
        };
    }

    // Then remove the directory itself
    if verbose {
        eprintln!("removing directory '{}'", path.display());
    }
    fs::remove_dir(path).into_diagnostic()?;

    Ok(count + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rm_command_name() {
        let cmd = RmCommand;
        assert_eq!(cmd.name(), "rm");
    }

    #[test]
    fn test_rm_signature() {
        let cmd = RmCommand;
        let sig = cmd.signature();
        assert_eq!(sig.name, "rm");
        assert_eq!(sig.description, "Remove files and directories");
        assert_eq!(sig.arguments.iter().filter(|a| a.required).count(), 1);
    }

    #[test]
    fn test_rm_recursive_with_symlinks() {
        // Regression: pnpm-style node_modules contain symlinks to directories.
        // remove_dir must delete the symlink itself, NOT recurse into its
        // target. Otherwise the directory stays non-empty and rm -rf silently
        // fails (files_removed: 0 with -f swallowing the error).
        use crate::shell::Shell;
        let dir = std::env::temp_dir().join(format!("ash-rm-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real_pkg")).unwrap();
        std::fs::write(dir.join("real_pkg/index.js"), "x").unwrap();
        // Create a symlink pointing to real_pkg (simulates pnpm node_modules)
        #[cfg(unix)]
        std::os::unix::fs::symlink("real_pkg", dir.join("link_pkg")).unwrap();
        #[cfg(windows)]
        {
            // Windows: use std::os::windows::fs::symlink_dir (may need admin/dev mode)
            let _ = std::os::windows::fs::symlink_dir("real_pkg", dir.join("link_pkg"));
        }
        // Also create a regular file
        std::fs::write(dir.join("plain.txt"), "x").unwrap();

        let mut shell = Shell::new();
        let result = shell.execute(&format!("rm -rf {}", dir.display()));
        assert!(result.is_ok(), "rm -rf should succeed: {:?}", result);
        assert!(!dir.exists(), "directory should be fully deleted");
    }
}
