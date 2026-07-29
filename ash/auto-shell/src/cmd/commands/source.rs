//! source command — Execute a .ash script in the current shell session.
//!
//! Like bash `source` / `.`, reads an ash script file and executes its
//! contents within the current shell context (variables, functions, and
//! shell commands all affect the current session).
//!
//! ## Usage
//! ```text
//! > source path/to/script.ash     # execute script in current session
//! > . path/to/script.ash          # shorter alias (bash-compatible)
//! ```

use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use miette::Result;

fn do_source(args: &ParsedArgs, shell: &mut Shell) -> Result<()> {
    let path_str = args
        .positionals
        .first()
        .map(|s| s.as_str())
        .ok_or_else(|| miette::miette!("source: missing file operand"))?;

    let path = shell.resolve_path(path_str, false)?;

    if !path.exists() {
        miette::bail!("source: {}: No such file", path.display());
    }

    shell.execute_script_file(&path)?;
    Ok(())
}

// ============================================================================
// source command
// ============================================================================

pub struct SourceCommand;

impl Command for SourceCommand {
    fn name(&self) -> &str {
        "source"
    }

    fn signature(&self) -> Signature {
        Signature::new("source", "Execute a .ash script in the current shell session")
            .required("file", "Path to the .ash script file")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        do_source(args, shell)?;
        Ok(PipelineData::from_text(String::new()))
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        do_source(args, shell)?;
        Ok(AtomPipeline::from_atom(Atom::new(
            auto_val::Value::str(""),
            AtomType::RunResult,
        )))
    }
}

// ============================================================================
// . (dot) command — bash-compatible alias for source
// ============================================================================

/// `MoreCommand`-style alias: registers `.` as a command name so users
/// can type `> . script.ash` (bash `source` / POSIX `.` convention).
pub struct DotCommand;

impl Command for DotCommand {
    fn name(&self) -> &str {
        "."
    }

    fn signature(&self) -> Signature {
        Signature::new(".", "Execute a .ash script in the current shell session (alias for source)")
            .required("file", "Path to the .ash script file")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        do_source(args, shell)?;
        Ok(PipelineData::from_text(String::new()))
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        do_source(args, shell)?;
        Ok(AtomPipeline::from_atom(Atom::new(
            auto_val::Value::str(""),
            AtomType::RunResult,
        )))
    }
}
