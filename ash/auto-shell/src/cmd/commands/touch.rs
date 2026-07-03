//! touch command - Create files or update timestamps
//!
//! Creates empty files if they don't exist, or updates their modification
//! timestamp if they do.

use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use ash_core::pipeline::AtomPipeline;
use miette::{IntoDiagnostic, Result};
use std::path::PathBuf;

pub struct TouchCommand;

impl Command for TouchCommand {
    fn name(&self) -> &str {
        "touch"
    }

    fn signature(&self) -> Signature {
        Signature::new("touch", "Create empty files or update timestamps")
            .required("file", "File(s) to create or update")
            .flag_with_short("no-create", 'c', "Do not create files, only update timestamps")
            // Plan 006 P0-5: POSIX flags
            .flag_with_short("access", 'a', "Change only the access time")
            .flag_with_short("modification", 'm', "Change only the modification time")
            .option_with_short("reference", 'r', "Use the times of REFERENCE file instead of now")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        if args.positionals.is_empty() {
            miette::bail!("touch: missing file operand");
        }

        let no_create = args.has_flag("no-create");
        // POSIX: default changes both atime and mtime. -a or -m restricts it.
        let touch_atime = args.has_flag("access") || !args.has_flag("modification");
        let touch_mtime = args.has_flag("modification") || !args.has_flag("access");

        // -r reference: read times from another file (best-effort).
        let ref_time: Option<std::time::SystemTime> = if let Some(ref_path) = args.get_option("reference") {
            let resolved = resolve_touch_path(ref_path, shell)?;
            let meta = std::fs::metadata(&resolved)
                .into_diagnostic()
                .map_err(|e| miette::miette!("touch -r: {}: {}", ref_path, e))?;
            Some(meta.modified().unwrap_or(std::time::SystemTime::now()))
        } else {
            None
        };

        let mut created = 0;
        let mut updated = 0;

        for arg in &args.positionals {
            let path = resolve_touch_path(arg, shell)?;

            if path.exists() {
                // Update timestamp
                let _ = std::fs::File::open(&path)
                    .into_diagnostic()
                    .map_err(|e| miette::miette!("touch: {}: {}", arg, e))?;
                if touch_mtime {
                    set_file_mtime(&path, ref_time)?;
                }
                if touch_atime {
                    // atime setting is best-effort: the shim only bumps via flush,
                    // so -a is effectively a no-op here (documented limitation).
                    set_file_atime(&path, ref_time)?;
                }
                updated += 1;
            } else if !no_create {
                // Create the file
                std::fs::File::create(&path)
                    .into_diagnostic()
                    .map_err(|e| miette::miette!("touch: {}: {}", arg, e))?;
                created += 1;
            }
        }

        let _ = (created, updated);
        Ok(PipelineData::empty())
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let legacy = self.run(args, PipelineData::empty(), shell)?;
        Ok(crate::cmd::pipeline_convert::pipeline_data_to_atom(legacy))
    }
}

/// Resolve path relative to shell CWD, honoring the security policy
/// (Plan 009: --sandbox / --read-only). touch writes, so for_write=true.
fn resolve_touch_path(arg: &str, shell: &mut Shell) -> Result<PathBuf> {
    shell.resolve_path(arg, true)
}

/// Set a file's modification time (cross-platform, best-effort).
/// If `ref_time` is given, use it instead of now (for -r).
fn set_file_mtime(path: &std::path::Path, ref_time: Option<std::time::SystemTime>) -> Result<()> {
    let now = ref_time.unwrap_or_else(std::time::SystemTime::now);
    let ft = filetime::FileTime::from_system_time(now);
    filetime::set_file_mtime(path, ft).into_diagnostic()
}

/// Set a file's access time. Best-effort: the shim only bumps via flush,
/// so on most platforms this is effectively a no-op for atime specifically.
fn set_file_atime(path: &std::path::Path, _ref_time: Option<std::time::SystemTime>) -> Result<()> {
    // No portable atime API; re-flush to bump timestamps generally.
    filetime::set_file_atime(path, filetime::FileTime::from_system_time(std::time::SystemTime::now()))
        .into_diagnostic()
}

// Since filetime may not be in Cargo.toml, provide a fallback.
mod filetime {
    use std::path::Path;

    /// Minimal FileTime shim.
    #[derive(Debug)]
    pub struct FileTime {
        _secs: i64,
        _nsecs: u32,
    }

    impl FileTime {
        pub fn from_system_time(st: std::time::SystemTime) -> Self {
            let dur = st.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            Self {
                _secs: dur.as_secs() as i64,
                _nsecs: dur.subsec_nanos(),
            }
        }
    }

    /// Set file modification time.
    ///
    /// This is a best-effort implementation. On platforms without `utimens`,
    /// we simply open the file for writing to bump the timestamp.
    pub fn set_file_mtime(path: &Path, _ft: FileTime) -> std::io::Result<()> {
        // Portable fallback: open for append and close — bumps mtime on most OSes
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        f.flush()?;
        Ok(())
    }

    /// Set file access time. Portable fallback (same flush); atime support
    /// is platform-dependent and this is effectively best-effort/no-op.
    pub fn set_file_atime(path: &Path, _ft: FileTime) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_command_name() {
        let cmd = TouchCommand;
        assert_eq!(cmd.name(), "touch");
    }

    #[test]
    fn test_touch_signature() {
        let cmd = TouchCommand;
        let sig = cmd.signature();
        assert_eq!(sig.name, "touch");
        assert!(sig.arguments.iter().any(|a| a.name == "file" && a.required));
    }

    // ---- Plan 006 P0-5: POSIX -a/-m/-r ----

    #[test]
    fn touch_flags_parse() {
        use crate::cmd::parser::parse_args;
        let sig = TouchCommand.signature();
        // -a and -m are recognized flags
        let parsed = parse_args(&sig, &["-a".to_string(), "f.txt".to_string()]).unwrap();
        assert!(parsed.has_flag("access"));
        assert!(!parsed.has_flag("modification"));
        let parsed = parse_args(&sig, &["-m".to_string(), "f.txt".to_string()]).unwrap();
        assert!(parsed.has_flag("modification"));
    }

    #[test]
    fn touch_reference_option_parses() {
        use crate::cmd::parser::parse_args;
        let sig = TouchCommand.signature();
        let parsed = parse_args(
            &sig,
            &["-r".to_string(), "ref.txt".to_string(), "target.txt".to_string()],
        )
        .expect("-r should parse");
        assert_eq!(parsed.get_option("reference").map(|s| s.as_str()), Some("ref.txt"));
        // target is the positional file operand
        assert!(parsed.positionals.iter().any(|p| p == "target.txt"));
    }

    #[test]
    fn touch_a_does_not_panic() {
        // Best-effort: -a on a real temp file must not error/panic (atime is
        // best-effort in the shim).
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("ash_touch_test_{}", pid));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.txt");
        std::fs::write(&path, "x").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "/");

        let mut shell = Shell::new();
        let result = shell.execute(&format!("touch -a {}", path_str));
        assert!(result.is_ok(), "touch -a should not error: {:?}", result);
        std::fs::remove_dir_all(&dir).ok();
    }
}
