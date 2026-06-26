use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;
use ash_core::pipeline::AtomPipeline;
use miette::Result;

pub struct EchoCommand;

impl Command for EchoCommand {
    fn name(&self) -> &str {
        "echo"
    }

    fn signature(&self) -> Signature {
        Signature::new("echo", "Print arguments followed by a newline")
            .flag_with_short("no-newline", 'n', "Do not output the trailing newline")
    }

    fn run(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: PipelineData,
        _shell: &mut Shell,
    ) -> Result<PipelineData> {
        Ok(PipelineData::from_text(echo_text(
            &args.positionals,
            args.has_flag("no-newline"),
        )))
    }

    fn run_atom(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: AtomPipeline,
        _shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        Ok(AtomPipeline::text(echo_text(
            &args.positionals,
            args.has_flag("no-newline"),
        )))
    }
}

/// Build echo's output text: arguments joined by spaces, with a trailing
/// newline unless `no_newline` is set (POSIX default = trailing newline).
pub fn echo_text(positionals: &[String], no_newline: bool) -> String {
    let joined = positionals.join(" ");
    if no_newline {
        joined
    } else {
        format!("{joined}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_default_adds_trailing_newline() {
        // POSIX default: echo outputs its args followed by a newline.
        let out = echo_text(&["hello".to_string(), "world".to_string()], false);
        assert_eq!(out, "hello world\n");
    }

    #[test]
    fn echo_n_suppresses_newline() {
        let out = echo_text(&["hi".to_string()], true);
        assert_eq!(out, "hi");
    }

    #[test]
    fn echo_no_args_just_newline() {
        let out = echo_text(&[], false);
        assert_eq!(out, "\n");
    }

    #[test]
    fn echo_no_args_n_empty() {
        let out = echo_text(&[], true);
        assert_eq!(out, "");
    }
}
