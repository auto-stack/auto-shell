use crate::cmd::{fs, Command, PipelineData, Signature};
use crate::shell::Shell;
use ash_core::pipeline::{Atom, AtomPipeline};
use miette::Result;
use std::path::Path;

pub struct LsCommand;

impl Command for LsCommand {
    fn name(&self) -> &str {
        "ls"
    }

    fn signature(&self) -> Signature {
        Signature::new("ls", "List directory contents")
            .optional("path", "Path to list")
            .flag_with_short("all", 'a', "Show all files including hidden (starts with .)")
            .flag_with_short("almost-all", 'A', "Show hidden files except . and ..")
            .flag_with_short("long", 'l', "Long listing format (permissions, owner, size, time)")
            .flag_with_short("human-readable", 'h', "Human-readable file sizes (1K, 234M, 2G)")
            .flag_with_short("time", 't', "Sort by modification time (newest first)")
            .flag_with_short("reverse", 'r', "Reverse sort order")
            .flag_with_short("recursive", 'R', "List subdirectories recursively")
    }

    fn run(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        let all = args.has_flag("all") || args.has_flag("almost-all");
        let long = args.has_flag("long");
        let time = args.has_flag("time");
        let reverse = args.has_flag("reverse");
        let recursive = args.has_flag("recursive");

        let value = collect_ls_value(args, &shell.pwd(), all, long, time, reverse, recursive)?;
        Ok(PipelineData::from_value(value))
    }

    fn run_atom(
        &self,
        args: &crate::cmd::parser::ParsedArgs,
        _input: AtomPipeline,
        shell: &mut Shell,
    ) -> Result<AtomPipeline> {
        let all = args.has_flag("all") || args.has_flag("almost-all");
        let long = args.has_flag("long");
        let time = args.has_flag("time");
        let reverse = args.has_flag("reverse");
        let recursive = args.has_flag("recursive");

        let value = collect_ls_value(args, &shell.pwd(), all, long, time, reverse, recursive)?;
        Ok(AtomPipeline::from_atom(Atom::file_list(value)))
    }
}

/// Collect file-list Value across ALL positional path arguments.
///
/// Bug A fix: previously ls only took `positionals[0]`, so `ls a.txt b.txt`
/// and `ls *.txt` (glob-expanded to multiple files) silently listed only the
/// first. bash lists every argument; this merges each path's entries into one
/// array. With no positionals, defaults to the current directory (`.`).
fn collect_ls_value(
    args: &crate::cmd::parser::ParsedArgs,
    current_dir: &Path,
    all: bool,
    long: bool,
    time_sort: bool,
    reverse: bool,
    recursive: bool,
) -> Result<auto_val::Value> {
    // No positionals → list the current directory.
    if args.positionals.is_empty() {
        return fs::ls_command_value(
            Path::new("."),
            current_dir,
            all,
            long,
            time_sort,
            reverse,
            recursive,
        );
    }

    // Single positional → unchanged behavior.
    if args.positionals.len() == 1 {
        let path = Path::new(&args.positionals[0]);
        return fs::ls_command_value(
            path, current_dir, all, long, time_sort, reverse, recursive,
        );
    }

    // Multiple positionals → list each, merge entries into one array.
    let mut merged: Vec<auto_val::Value> = Vec::new();
    for path_arg in &args.positionals {
        let path = Path::new(path_arg);
        let value = fs::ls_command_value(
            path, current_dir, all, long, time_sort, reverse, recursive,
        )?;
        if let auto_val::Value::Array(arr) = value {
            merged.extend(arr.values);
        } else {
            merged.push(value);
        }
    }
    Ok(auto_val::Value::Array(auto_val::Array { values: merged }))
}
