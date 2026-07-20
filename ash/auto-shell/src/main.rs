use miette::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    // Pre-warm syntect syntax/theme caches in the background as early as
    // possible, so that `ash -c "show file.rs"` can overlap loading with CLI
    // parsing and shell setup, and the REPL has them ready before the first
    // prompt even appears.
    auto_shell::cmd::commands::code_highlight::warmup();

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

    // Plan 008 (MS2-A): parse security flags anywhere on the command line.
    // They augment the policy loaded from config (`[security]` section).
    let mut policy = parse_security_flags(&args);

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--json" => {
                // Already handled by the global pre-scan; skip here.
                i += 1;
                continue;
            }
            "--allow" | "--deny" | "--audit" | "--sandbox" => {
                // Consumed by parse_security_flags; skip value here.
                i += 2;
                continue;
            }
            "--no-exec" | "--no-network" | "--dry-run" | "--read-only" => {
                // Consumed by parse_security_flags.
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
                // Plan 008: apply CLI security policy.
                shell.set_policy(std::mem::take(&mut policy));
                match shell.execute_for_agent(command, json_mode) {
                    Ok(output) => {
                        if let Some(s) = output {
                            println!("{}", s);
                        }
                        // Plan 008: a security denial or command that set a
                        // non-zero exit code must propagate to the process
                        // exit code (agents rely on it).
                        let code = shell.last_exit_code();
                        if code != 0 {
                            std::process::exit(code);
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
                // Plan 008: apply CLI security policy.
                shell.set_policy(std::mem::take(&mut policy));
                shell.execute_script_content(&input)?;
                // Plan 011: honor AutoLang `exit(code)`.
                if shell.script_exit_requested() {
                    std::process::exit(shell.script_exit_code());
                }
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
                println!();
                println!("  On first start, ash creates ~/.ashrc with example functions.");
                println!("  Edit it to define your own functions (like .bashrc).");
                println!();
                println!("  --json            Output as JSON (agent mode; may appear anywhere)");
                println!("  ash -c <cmd> --json      Pipeline result as JSON");
                println!("  ash -s --json           Each command as NDJSON");
                println!("  ash <script.at> --json  Script output as NDJSON");
                println!();
                println!("  SECURITY (Plan 008):");
                println!("  --allow <cmd>     Only allow listed commands (default-deny)");
                println!("  --deny <cmd>      Deny a command (repeatable)");
                println!("  --no-exec         Block all external commands");
                println!("  --no-network      Block network commands (http_*, curl, wget, ssh...)");
                println!("  --read-only       Block write commands (rm/mv/cp/mkdir/touch...)");
                println!("  --sandbox <dir>   Confine all file operations to <dir> (Plan 009)");
                println!("  --dry-run         Print what would run, don't execute writes/spawns");
                println!("  --audit <file>    Append each command to a JSON-lines audit log");
                println!();
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
        // Plan 008: apply CLI security policy.
        shell.set_policy(std::mem::take(&mut policy));
        shell.execute_script_file(path)?;
        // Plan 011: honor AutoLang `exit(code)`.
        if shell.script_exit_requested() {
            std::process::exit(shell.script_exit_code());
        }
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
    // Plan 008: apply CLI security policy to the REPL shell too.
    if policy.active() {
        repl.set_policy(policy);
    }
    repl.run()?;

    Ok(())
}

/// Plan 008 (MS2-A): Pre-scan command-line args for security flags and build
/// a policy that augments the config-loaded policy. Returns a default (no-op)
/// policy when no security flags are present.
fn parse_security_flags(args: &[String]) -> ash_core::security::SecurityPolicy {
    // Start from the config-loaded policy so config `[security]` settings form
    // the base; CLI flags then turn additional restrictions on.
    let cfg = auto_shell::config::AshShellConfig::load();
    let mut policy = cfg.security.to_policy();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--allow" => {
                if let Some(val) = args.get(i + 1) {
                    // CLI --allow replaces the config allow-list (more specific).
                    if !policy.allow.iter().any(|a| a == val) {
                        policy.allow.push(val.clone());
                    }
                    i += 2;
                    continue;
                }
            }
            "--deny" => {
                if let Some(val) = args.get(i + 1) {
                    if !policy.deny.iter().any(|d| d == val) {
                        policy.deny.push(val.clone());
                    }
                    i += 2;
                    continue;
                }
            }
            "--audit" => {
                if let Some(val) = args.get(i + 1) {
                    policy.audit_file = Some(PathBuf::from(val));
                    i += 2;
                    continue;
                }
            }
            "--sandbox" => {
                if let Some(val) = args.get(i + 1) {
                    policy.sandbox_dir = Some(PathBuf::from(val));
                    i += 2;
                    continue;
                }
            }
            "--no-exec" => policy.no_exec = true,
            "--no-network" => policy.no_network = true,
            "--read-only" => policy.read_only = true,
            "--dry-run" => policy.dry_run = true,
            _ => {}
        }
        i += 1;
    }
    policy
}

