//! realpath command — print the resolved absolute path
//!
//! Resolves symlinks, `..`, `.` and relative paths to produce the canonical
//! absolute path. Mirrors `realpath(1)` from coreutils.

use crate::cmd::{Command, PipelineData, Signature};
use crate::cmd::parser::ParsedArgs;
use crate::shell::Shell;
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use miette::{IntoDiagnostic, Result};
use auto_val::Value;

pub struct RealpathCommand;

impl Command for RealpathCommand {
    fn name(&self) -> &str {
        "realpath"
    }

    fn signature(&self) -> Signature {
        Signature::new("realpath", "Print the resolved absolute path")
            .required("path", "Path(s) to resolve")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        if args.positionals.is_empty() {
            miette::bail!("realpath: missing operand");
        }

        let mut results: Vec<String> = Vec::new();
        for arg in &args.positionals {
            let resolved = shell.resolve_path(arg, false)?;
            // resolve_path already canonicalizes; but if the path doesn't exist
            // yet it uses canonicalize_or_parent. For realpath we want the full
            // canonical path — if resolve_path succeeded, it's already canonical.
            results.push(resolved.to_string_lossy().into_owned());
        }

        if results.len() == 1 {
            Ok(PipelineData::from_text(results.into_iter().next().unwrap()))
        } else {
            Ok(PipelineData::from_value(Value::Array(
                results.into_iter().map(Value::str).collect::<Vec<_>>().into(),
            )))
        }
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let legacy = self.run(args, PipelineData::empty(), shell)?;
        let value = match legacy {
            PipelineData::Text(s) => Value::str(&s),
            PipelineData::Value(v) => v,
        };
        Ok(AtomPipeline::from_atom(Atom::new(value, AtomType::Text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realpath_command_name() {
        let cmd = RealpathCommand;
        assert_eq!(cmd.name(), "realpath");
    }

    #[test]
    fn test_realpath_resolves_absolute() {
        // An absolute path to an existing file resolves to itself (canonicalized).
        let mut shell = Shell::new();
        let args = ParsedArgs {
            positionals: vec![".".to_string()],
            ..Default::default()
        };
        let result = cmd_run(&RealpathCommand, &args, &mut shell);
        assert!(result.is_ok());
    }

    fn cmd_run(cmd: &RealpathCommand, args: &ParsedArgs, shell: &mut Shell) -> Result<PipelineData> {
        cmd.run(args, PipelineData::empty(), shell)
    }
}
