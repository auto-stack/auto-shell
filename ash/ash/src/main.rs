use miette::Result;
use std::path::PathBuf;

/// Plan 037 M2.2: wire a `Shell` with the terminal frontend's hooks and
/// terminal-only commands. The `ash` binary is the composition root — it pulls
/// the pure Shell logic (auto-shell) together with the terminal frontend
/// modules (src/frontend/, ex-ash-tui per Plan 071). Every code path that
/// builds a `Shell` (`-c`, `-s`, script, REPL)
/// must call this so terminal-dependent commands (`less`/`more`/`color`) and
/// structured rendering behave identically to before the crate split.
fn wire_shell(shell: &mut auto_shell::Shell) {
    shell.set_render_hook(Box::new(ash::frontend::renderer::TuiRenderHook));
    shell.set_pager_hook(Box::new(ash::frontend::commands::TuiPagerHook));
    shell.register_commands(ash::frontend::commands::terminal_commands());
}

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
    // Plan 036 P1: --bash-compat renders structured commands (ls/grep/wc/ps)
    // as bash-style plain text instead of a ratatui table (for parity tests).
    let bash_compat = args.iter().any(|a| a == "--bash-compat");

    // Plan 008 (MS2-A): parse security flags anywhere on the command line.
    // They augment the policy loaded from config (`[security]` section).
    let mut policy = parse_security_flags(&args);
    // PLAN-081 (R1): writable whitelist roots must exist — a typo would
    // otherwise silently deny every write (fail loudly, not silently).
    if let Err(msg) = validate_writable_roots(&policy) {
        eprintln!("ash: {msg}");
        std::process::exit(2); // usage/config error
    }

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            // Plan 033: `ash plugin <subcommand>` — plugin manager
            // (install/list/enable/disable/remove/update/show). Consumes all
            // remaining args.
            "plugin" => {
                let plugin_args: Vec<String> = args[i + 1..].to_vec();
                // Plan 072 M2: plugin management mutates disk and installs
                // executable code — mutating subcommands honor the security
                // flags (previously this early-return bypassed them entirely).
                let sub = plugin_args.first().map(String::as_str).unwrap_or("");
                let mutating = matches!(
                    sub,
                    "install" | "update" | "remove" | "enable" | "disable"
                );
                if mutating && (policy.read_only || policy.no_exec || policy.dry_run) {
                    eprintln!(
                        "security: 'ash plugin {sub}' is disabled under \
                         --read-only/--no-exec/--dry-run"
                    );
                    std::process::exit(1);
                }
                return auto_shell::plugin::cli::run(&plugin_args);
            }
            // Plan 029 §6: `ash ask "<nl>"` — NL→AutoLang. Generates and runs
            // an AutoLang script via the cloud model + eval_auto tool.
            "ask" => {
                let ask_args: Vec<String> = args[i + 1..].to_vec();
                // Plan 072 M2 (S-5): the agent's shell runs under the CLI
                // security policy.
                return auto_shell::ai::ask::run(&ask_args, policy);
            }
            "--json" => {
                // Already handled by the global pre-scan; skip here.
                i += 1;
                continue;
            }
            "--bash-compat" => {
                // Already handled by the global pre-scan; skip here.
                i += 1;
                continue;
            }
            "--allow" | "--deny" | "--audit" | "--sandbox" | "--writable"
            | "--policy-file" => {
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
                wire_shell(&mut shell); // Plan 037 M2.2: hooks + terminal commands
                shell.load_env_persistence(); // Plan 309 Task 1.2 P4: apply ~/.config/ash/env.at
                // Plan 008: apply CLI security policy.
                shell.set_policy(std::mem::take(&mut policy));
                match shell.execute_for_agent(command, json_mode, bash_compat) {
                    Ok(output) => {
                        if let Some(s) = output {
                            // Avoid spurious trailing blank line when output
                            // already ends in '\n' (e.g. echo). Same fix as R4.
                            auto_shell::shell::print_command_output(&s);
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
                wire_shell(&mut shell); // Plan 037 M2.2: hooks + terminal commands
                shell.load_env_persistence();
                // Plan 007: --json serializes each command's output as a JSON
                // line (NDJSON) for agent consumers.
                shell.set_json_output(json_mode);
                // Plan 036 P1: --bash-compat renders structured commands as text.
                shell.set_bash_compat(bash_compat);
                // Plan 008: apply CLI security policy.
                shell.set_policy(std::mem::take(&mut policy));
                shell.execute_script_content(&input)?;
                // Plan 011: honor AutoLang `exit(code)`.
                if shell.script_exit_requested() {
                    std::process::exit(shell.script_exit_code());
                }
                // PLAN-081 T-02: a script whose commands/blocks failed must
                // not exit 0 (agents judge scripts by the process exit code).
                if shell.script_had_error() {
                    std::process::exit(1);
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
                println!("  --bash-compat     Render structured commands (ls/grep/wc/ps) as");
                println!("                    bash-style plain text instead of a table");
                println!();
                println!("  SECURITY (Plan 008):");
                println!("  --allow <cmd>     Only allow listed commands (default-deny)");
                println!("  --deny <cmd>      Deny a command (repeatable)");
                println!("  --no-exec         Block all external commands");
                println!("  --no-network      Block network commands (http_*, curl, wget, ssh...)");
                println!("  --read-only       Block write commands (rm/mv/cp/mkdir/touch...)");
                println!("  --sandbox <dir>   Confine all file operations to <dir> (Plan 009)");
                println!("  --writable <dir>  Writable whitelist root (repeatable). Writes outside");
                println!("                    every root are denied; reads stay open (Plan 081)");
                println!("  --policy-file <f> Load security policy from a JSON file (Plan 081)");
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
        wire_shell(&mut shell); // Plan 037 M2.2: hooks + terminal commands
        shell.load_env_persistence();
        // Plan 007: --json serializes each command's output as a JSON
        // line (NDJSON) for agent consumers.
        shell.set_json_output(json_mode);
        // Plan 036 P1: --bash-compat renders structured commands as text.
        shell.set_bash_compat(bash_compat);
        // Plan 008: apply CLI security policy.
        shell.set_policy(std::mem::take(&mut policy));
        // Plan 034 Bug 2: pass positional args to the script ($1, $@, $#).
        let script_args: Vec<String> = args[(i + 1)..].to_vec();
        shell.set_script_args(script_args);
        shell.execute_script_file(path)?;
        // Plan 011: honor AutoLang `exit(code)`.
        if shell.script_exit_requested() {
            std::process::exit(shell.script_exit_code());
        }
        // PLAN-081 T-02: script failure must propagate (was exit 0 — the gap
        // auto-ai's FailureClassifier models as PreExecFailure/RanFailed).
        if shell.script_had_error() {
            std::process::exit(1);
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

    let mut repl = ash::frontend::Repl::new()?;
    // Plan 008: apply CLI security policy to the REPL shell too.
    if policy.active() {
        repl.set_policy(policy);
    }
    repl.run()?;

    Ok(())
}

/// PLAN-081 (R2): CLI security flags, scanned first, applied last.
///
/// Precedence across the three policy layers is: config `[security]` <
/// `--policy-file` < CLI flags. Each layer only ever adds restrictions
/// (booleans OR, lists union) or relocates single-value paths (sandbox /
/// audit) — a more specific layer never weakens a broader one.
#[derive(Default)]
struct CliSecurity {
    allow: Vec<String>,
    deny: Vec<String>,
    writable: Vec<PathBuf>,
    audit: Option<PathBuf>,
    sandbox: Option<PathBuf>,
    no_exec: bool,
    no_network: bool,
    read_only: bool,
    dry_run: bool,
    policy_file: Option<PathBuf>,
}

impl CliSecurity {
    fn merge_over(self, policy: &mut ash_core::security::SecurityPolicy) {
        for a in self.allow {
            if !policy.allow.contains(&a) {
                policy.allow.push(a);
            }
        }
        for d in self.deny {
            if !policy.deny.contains(&d) {
                policy.deny.push(d);
            }
        }
        for w in self.writable {
            if !policy.writable_roots.contains(&w) {
                policy.writable_roots.push(w);
            }
        }
        if self.audit.is_some() {
            policy.audit_file = self.audit;
        }
        if self.sandbox.is_some() {
            policy.sandbox_dir = self.sandbox;
        }
        policy.no_exec |= self.no_exec;
        policy.no_network |= self.no_network;
        policy.read_only |= self.read_only;
        policy.dry_run |= self.dry_run;
    }
}

/// Plan 008 (MS2-A): Pre-scan command-line args for security flags and build
/// the assembled policy: config base ← policy file ← CLI flags (PLAN-081
/// R2). Returns a default (no-op) policy when nothing is configured.
fn parse_security_flags(args: &[String]) -> ash_core::security::SecurityPolicy {
    // Layer 1: config-loaded policy (`[security]` section) as the base.
    let cfg = auto_shell::config::AshShellConfig::load();
    let mut policy = cfg.security.to_policy();

    let mut cli = CliSecurity::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--allow" => {
                if let Some(val) = args.get(i + 1) {
                    if !cli.allow.contains(val) {
                        cli.allow.push(val.clone());
                    }
                    i += 2;
                    continue;
                }
            }
            "--deny" => {
                if let Some(val) = args.get(i + 1) {
                    if !cli.deny.contains(val) {
                        cli.deny.push(val.clone());
                    }
                    i += 2;
                    continue;
                }
            }
            "--audit" => {
                if let Some(val) = args.get(i + 1) {
                    cli.audit = Some(PathBuf::from(val));
                    i += 2;
                    continue;
                }
            }
            "--sandbox" => {
                if let Some(val) = args.get(i + 1) {
                    cli.sandbox = Some(PathBuf::from(val));
                    i += 2;
                    continue;
                }
            }
            "--writable" => {
                // PLAN-081 (R1): repeatable; roots union across layers.
                if let Some(val) = args.get(i + 1) {
                    let val = PathBuf::from(val);
                    if !cli.writable.contains(&val) {
                        cli.writable.push(val);
                    }
                    i += 2;
                    continue;
                }
            }
            "--policy-file" => {
                // PLAN-081 (R2): JSON policy file, merged over config,
                // overridden by CLI flags.
                if let Some(val) = args.get(i + 1) {
                    cli.policy_file = Some(PathBuf::from(val));
                    i += 2;
                    continue;
                }
            }
            "--no-exec" => cli.no_exec = true,
            "--no-network" => cli.no_network = true,
            "--read-only" => cli.read_only = true,
            "--dry-run" => cli.dry_run = true,
            _ => {}
        }
        i += 1;
    }

    // Layer 2: policy file over config. A load failure is fatal (exit 2) —
    // running with a partially-applied security config is worse than not
    // starting (072 S-6 lesson).
    if let Some(pf_path) = &cli.policy_file {
        match auto_shell::policy_file::PolicyFile::load(pf_path) {
            Ok(pf) => pf.merge_over(&mut policy),
            Err(e) => {
                eprintln!("ash: {e}");
                std::process::exit(2);
            }
        }
    }

    // Layer 3: CLI flags over everything.
    cli.merge_over(&mut policy);
    policy
}

/// PLAN-081 (R1): every writable whitelist root must exist (and be a
/// directory) — canonicalization is the existence check. A silently-typoed
/// root would deny every write; refuse to start instead (usage error, exit 2).
fn validate_writable_roots(
    policy: &ash_core::security::SecurityPolicy,
) -> Result<(), String> {
    for root in &policy.writable_roots {
        match root.canonicalize() {
            Ok(canon) if canon.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "--writable {}: not a directory",
                    root.display()
                ));
            }
            Err(e) => {
                return Err(format!("--writable {}: {}", root.display(), e));
            }
        }
    }
    Ok(())
}

