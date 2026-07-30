//! `ash smart` subcommand handler (Plan 029 §A.7).
//!
//! Invoked from `main.rs` when the first CLI arg is `smart`. Supports:
//! - `ash smart list` — list discovered SmartCommands
//! - `ash smart run <name> [args...]` — run a command's body with args
//! - `ash smart "<nl>"` — NLU match (v1 stub: not yet implemented)
//!
//! The remaining argv after `smart` is passed here as `args`.

use miette::Result;

use crate::shell::Shell;

use super::executor;
use super::loader;

/// Dispatch the `ash smart` subcommand. `args` is everything after `smart`
/// (e.g. `["list"]` or `["run", "git.finish-worktree", "main"]`).
pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_usage();
        return Ok(());
    }
    match args[0].as_str() {
        "list" => cmd_list(),
        "run" => {
            let rest = &args[1..];
            if rest.is_empty() {
                eprintln!("ash smart run: missing command name");
                eprintln!("  usage: ash smart run <name> [args...]");
                std::process::exit(2);
            }
            let name = &rest[0];
            let cmd_args = rest[1..].to_vec();
            cmd_run(name, &cmd_args)
        }
        "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => {
            // Could be natural-language input ("ash smart deploy to prod"),
            // but v1 has no NLU routing yet.
            eprintln!(
                "ash smart: unknown subcommand '{other}'.\n\
                 Natural-language routing is not yet implemented; use 'ash smart run <name>'."
            );
            print_usage();
            std::process::exit(2);
        }
    }
}

/// `ash smart list` — discover and print all SmartCommands.
fn cmd_list() -> Result<()> {
    let specs = loader::load_all();
    if specs.is_empty() {
        println!("No SmartCommands found.");
        println!("Add .at files to ./smart/ or ~/.config/ash/smart/");
        return Ok(());
    }
    println!("SmartCommands:");
    for spec in &specs {
        let args = if spec.args.is_empty() {
            String::new()
        } else {
            format!(" {}", spec.args.iter().map(|a| format!("<{a}>")).collect::<Vec<_>>().join(" "))
        };
        println!("  {}{args}", spec.name);
        println!("    {}", spec.description);
    }
    Ok(())
}

/// `ash smart run <name> [args...]` — find and execute a SmartCommand.
fn cmd_run(name: &str, args: &[String]) -> Result<()> {
    let specs = loader::load_all();
    let spec = specs
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| miette::miette!("SmartCommand '{name}' not found. Try 'ash smart list'."))?;
    let mut shell = Shell::new();
    shell.load_env_persistence();
    executor::execute(spec, args, &mut shell)
}

fn print_usage() {
    println!("usage: ash smart <subcommand> [args]");
    println!();
    println!("subcommands:");
    println!("  list              list discovered SmartCommands");
    println!("  run <name> [args] run a SmartCommand's body with positional args");
    println!("  help              show this help");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    // These tests change the process-global cwd, so they must not run in
    // parallel with each other (or with anything else that reads cwd).
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// Set up a temp dir with a smart/ command + body, return (cwd, name).
    /// Caller must hold CWD_LOCK.
    fn setup_one_command() -> (std::path::PathBuf, String) {
        let tmp = std::env::temp_dir().join("ash_smart_cli_test");
        let _ = fs::remove_dir_all(&tmp);
        let smart = tmp.join("smart");
        fs::create_dir_all(&smart).unwrap();
        fs::write(
            smart.join("hello.at"),
            "command \"hello\" {\n    description : \"say hello\"\n    body        : \"hello.ash\"\n}\n",
        )
        .unwrap();
        fs::write(smart.join("hello.ash"), "> echo hello-world\n").unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        (tmp, "hello".to_string())
    }

    #[test]
    fn list_finds_command() {
        let _guard = CWD_LOCK.lock().unwrap();
        let (tmp, _) = setup_one_command();
        let result = cmd_list();
        assert!(result.is_ok(), "{:?}", result);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_executes_command() {
        let _guard = CWD_LOCK.lock().unwrap();
        let (tmp, name) = setup_one_command();
        let result = run(&["run".into(), name]);
        assert!(result.is_ok(), "run should execute: {:?}", result);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_unknown_command_errors() {
        let _guard = CWD_LOCK.lock().unwrap();
        let (tmp, _) = setup_one_command();
        let result = run(&["run".into(), "nonexistent".into()]);
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_args_prints_usage() {
        let result = run(&[]);
        assert!(result.is_ok(), "no args should print usage and return ok");
    }
}
