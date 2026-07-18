//! Plan 028 M2: `ash agent ...` CLI subcommand family.
//!
//! Subcommands:
//!   ash agent describe-tools [--format json|compact] [--filter file,git,...]
//!   ash agent describe-policy
//!   ash agent check "<command>"
//!   ash agent run "<command>" [--timeout N] [--format json|text]
//!
//! All subcommands return an `i32` exit code and call `std::process::exit`
//! from `main.rs`. Output goes to stdout as JSON (or plain text for
//! `run --format text`). Errors go to stderr.

pub mod describe;
pub mod run;

/// Dispatch the `ash agent <sub>` subcommand. Called from main.rs.
///
/// `args` is everything after `agent` on the command line.
/// Returns an exit code (0 = success, 1 = execution failure, 2 = usage error).
pub fn dispatch(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!(
            "usage: ash agent <subcommand>\n\
             \nsubcommands:\n\
             \n  describe-tools [--format json|compact] [--filter file,git,...]\n\
             \n      Export the tool catalog (names + JSON schemas).\n\
             \n  describe-policy\n\
             \n      Export the security policy summary (capabilities only).\n\
             \n  check \"<command>\"\n\
             \n      Dry-run: report whether <command> would be allowed.\n\
             \n  run \"<command>\" [--timeout N] [--format json|text]\n\
             \n      Execute <command> and return a structured JSON envelope."
        );
        return 2;
    }
    let sub = args[0].as_str();
    let rest = &args[1..];
    match sub {
        "describe-tools" | "describe" => describe::describe_tools(rest),
        "describe-policy" => describe::describe_policy(),
        "check" => run::check_command(rest),
        "run" => run::run_command(rest),
        other => {
            eprintln!("ash agent: unknown subcommand '{}'", other);
            eprintln!("run 'ash agent' with no args for usage.");
            2
        }
    }
}
