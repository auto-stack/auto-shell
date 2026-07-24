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
            .flag_with_short("interpret", 'e', "Interpret backslash escape sequences")
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
            args.has_flag("interpret"),
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
            args.has_flag("interpret"),
        )))
    }
}

/// Build echo's output text: arguments joined by spaces, with a trailing
/// newline unless `no_newline` is set (POSIX default = trailing newline).
/// When `interpret` is set (-e), backslash escapes are interpreted.
pub fn echo_text(positionals: &[String], no_newline: bool, interpret: bool) -> String {
    let joined = positionals.join(" ");
    let text = if interpret { interpret_escapes(&joined) } else { joined };
    if no_newline {
        text
    } else {
        format!("{text}\n")
    }
}

/// Interpret common backslash escape sequences (bash echo -e subset).
fn interpret_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('a') => out.push('\x07'), // bell
                Some('b') => out.push('\x08'), // backspace
                Some('f') => out.push('\x0c'), // form feed
                Some('v') => out.push('\x0b'), // vertical tab
                Some('0') => {
                    // \0NNN octal (up to 3 octal digits)
                    let mut val: u32 = 0;
                    let mut count = 0;
                    while count < 3 {
                        if let Some(&d) = chars.peek() {
                            if ('0'..='7').contains(&d) {
                                val = val * 8 + (d as u32 - '0' as u32);
                                chars.next();
                                count += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(ch) = char::from_u32(val) {
                        out.push(ch);
                    }
                }
                Some(other) => {
                    // Unknown escape: keep backslash + char literally
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_default_adds_trailing_newline() {
        // POSIX default: echo outputs its args followed by a newline.
        let out = echo_text(&["hello".to_string(), "world".to_string()], false, false);
        assert_eq!(out, "hello world\n");
    }

    #[test]
    fn echo_n_suppresses_newline() {
        let out = echo_text(&["hi".to_string()], true, false);
        assert_eq!(out, "hi");
    }

    #[test]
    fn echo_no_args_just_newline() {
        let out = echo_text(&[], false, false);
        assert_eq!(out, "\n");
    }

    #[test]
    fn echo_no_args_n_empty() {
        let out = echo_text(&[], true, false);
        assert_eq!(out, "");
    }

    #[test]
    fn echo_e_interpret_newline() {
        let out = echo_text(&["a\\nb".to_string()], false, true);
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn echo_e_interpret_tab_and_backslash() {
        let out = echo_text(&["x\\ty\\\\z".to_string()], false, true);
        assert_eq!(out, "x\ty\\z\n");
    }

    #[test]
    fn echo_e_unknown_escape_kept_literal() {
        let out = echo_text(&["a\\qb".to_string()], false, true);
        assert_eq!(out, "a\\qb\n");
    }

    #[test]
    fn echo_without_e_keeps_backslash_literal() {
        let out = echo_text(&["a\\nb".to_string()], false, false);
        assert_eq!(out, "a\\nb\n");
    }
}
