use miette::Result;

fn main() -> Result<()> {
    // Set up miette for beautiful error reporting
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .build(),
        )
    }))
    .ok();

    let args: Vec<String> = std::env::args().collect();

    // ── CLI argument handling ──────────────────────
    //
    // ash               → interactive REPL
    // ash script.at     → execute script file (Plan 303)
    // ash -c "cmd"      → execute single command (Plan 304)
    // ash -s            → read script from stdin (Plan 304)
    // ash -l / --login  → login shell mode (Plan 304)
    // ash -h / --help   → help text
    // ash -v / --version → version

    let mut i = 1;
    let mut login_mode = false;
    // Plan 007: `--json` is a global flag that may appear anywhere on the
    // command line (`ash --json -c "ls"` or `ash -c "ls" --json`), so we
    // pre-scan it out before the positional `-c` / script parsing below.
    // (`-c` consumes its argument and returns, so a `--json` *after* it would
    // otherwise never be seen.)
    let json_mode = args.iter().any(|a| a == "--json");

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--json" => {
                // Already handled by the global pre-scan; skip here.
                i += 1;
                continue;
            }
            "-c" => {
                // Execute a single command string (Plan 007: --json for agent)
                if i + 1 >= args.len() {
                    eprintln!("ash -c: option requires an argument");
                    std::process::exit(2); // usage error
                }
                let command = &args[i + 1];
                let mut shell = auto_shell::Shell::new();
                shell.load_env_persistence(); // Plan 309 Task 1.2 P4: apply ~/.config/ash/env.at
                match shell.execute_for_agent(command, json_mode) {
                    Ok(output) => {
                        if let Some(s) = output {
                            println!("{}", s);
                        }
                    }
                    Err(e) => {
                        // Diagnostics to stderr; stdout stays clean for agent JSON parsing.
                        eprintln!("Error: {}", e);
                        std::process::exit(1); // command error
                    }
                }
                return Ok(());
            }
            "-s" => {
                // Read script from stdin
                let mut input = String::new();
                if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input) {
                    eprintln!("ash: failed to read stdin: {}", e);
                    std::process::exit(1);
                }
                let mut shell = auto_shell::Shell::new();
                shell.load_env_persistence();
                // Plan 007: --json serializes each command's output as a JSON
                // line (NDJSON) for agent consumers.
                shell.set_json_output(json_mode);
                shell.execute_script_content(&input)?;
                return Ok(());
            }
            "-l" | "--login" => {
                login_mode = true;
                i += 1;
                continue;
            }
            "-h" | "--help" => {
                println!("ash — AutoShell v0.1.0");
                println!();
                println!("USAGE:");
                println!("  ash               Start interactive REPL");
                println!("  ash <script.at>   Execute a script file");
                println!("  ash -c <cmd>      Execute a single command");
                println!("  ash -s            Read script from stdin");
                println!("  ash -l, --login   Start as login shell");
                println!("  ash -c <cmd> --json  Output pipeline result as JSON (agent mode)");
                println!("  ash -s --json        Output each command's result as JSON (NDJSON)");
                println!("  ash <script.at> --json  Script output as NDJSON");
                println!("  ash -h, --help    Show this help");
                println!("  ash -v, --version Show version");
                return Ok(());
            }
            "-v" | "--version" => {
                println!("ash (AutoShell) v0.1.0");
                return Ok(());
            }
            _ => {
                // Not a flag — treat as script file path
                break;
            }
        }
    }

    // Determine if we're running a script or entering REPL
    let script_arg = args.get(i).map(|s| s.as_str()).unwrap_or("");

    if !script_arg.is_empty() && !script_arg.starts_with('-') {
        // Script execution mode: ash hello.at [args...]
        let path = std::path::Path::new(script_arg);

        if !path.exists() {
            eprintln!("ash: {}: No such file", script_arg);
            std::process::exit(1);
        }

        let mut shell = auto_shell::Shell::new();
        shell.load_env_persistence();
        // Plan 007: --json serializes each command's output as a JSON
        // line (NDJSON) for agent consumers.
        shell.set_json_output(json_mode);
        shell.execute_script_file(path)?;
        return Ok(());
    }

    // Interactive REPL (with optional login mode)
    if login_mode {
        // Login shell: source /etc/profile, then ~/.ash_profile or ~/.ashrc
        #[cfg(unix)]
        {
            let etc_profile = std::path::Path::new("/etc/profile");
            if etc_profile.exists() {
                // Best-effort: source /etc/profile via external shell
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("source /etc/profile 2>/dev/null && env")
                    .output()
                    .ok();
            }
        }
    }

    println!("AutoShell v0.1.0");
    println!("Type 'exit' or press Ctrl+D to exit");
    println!();

    let mut repl = auto_shell::Repl::new()?;
    repl.run()?;

    Ok(())
}
