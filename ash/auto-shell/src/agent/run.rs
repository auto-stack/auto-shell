//! Plan 028 M2.4: `ash agent run` and `ash agent check`.

use std::time::Instant;

use ash_core::tool::envelope::build_envelope;
use ash_core::tool::{ErrorKind, ToolData, ToolResult};

/// `ash agent run "<command>" [--timeout N] [--format json|text]`
///
/// Executes a single command via `Shell::execute_for_agent` and wraps the
/// output in the Plan 028 response envelope (see `envelope::build_envelope`).
///
/// Exit code: 0 on success, 1 on failure/denied, 2 on usage error.
pub fn run_command(args: &[String]) -> i32 {
    let (command, _timeout, format) = match parse_run_args(args) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let mut shell = crate::shell::Shell::new();
    shell.load_env_persistence();

    let start = Instant::now();
    let exec_result = shell.execute_for_agent(&command, false);
    let elapsed = start.elapsed();
    let wall_ms = elapsed.as_millis() as u64;

    let mut result = match exec_result {
        Ok(Some(output)) => {
            if format == "text" {
                // Plain text mode: just print the output, no envelope.
                println!("{}", output);
                return 0;
            }
            // JSON mode: wrap text output as a Text data block.
            let mut r = ToolResult::success_json(serde_json::json!({
                "kind": "text",
                "atom_type": "Text",
                "value": output,
                "pipeline_hint": "pipeable to grep/head/tail/wc",
            }));
            r.timing.wall_ms = wall_ms;
            r
        }
        Ok(None) => {
            // No output (side-effect command like mkdir).
            let mut r = ToolResult::success_json(serde_json::json!({
                "kind": "empty",
                "atom_type": "Nothing",
                "value": serde_json::Value::Null,
            }));
            r.timing.wall_ms = wall_ms;
            r
        }
        Err(e) => {
            let msg = format!("{}", e);
            let kind = classify_error(&msg);
            let mut r = ToolResult::failed(kind, msg);
            r.timing.wall_ms = wall_ms;
            r
        }
    };

    // If the shell recorded a non-zero exit code, override status to Failed
    // (the command ran but signaled an error).
    let exit_code = shell.last_exit_code();
    if exit_code != 0 && result.is_success() {
        let mut r = ToolResult::failed(
            ErrorKind::NonzeroExit,
            format!("command exited with code {}", exit_code),
        );
        r.timing.wall_ms = wall_ms;
        result = r;
    }

    let envelope = build_envelope(&result, &command);
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());

    // Use the unused-variable warning suppression; data field is consumed by build_envelope.
    let _ = (ToolData::Empty,);

    if result.is_success() {
        0
    } else {
        1
    }
}

/// `ash agent check "<command>"`
///
/// Dry-run: evaluate the command against the security policy WITHOUT
/// executing. Returns whether it would be allowed, and the decision reason.
/// Always exits 0 (the check itself succeeded; the *decision* is in the JSON).
pub fn check_command(args: &[String]) -> i32 {
    let command = match args.get(0) {
        Some(c) => c.clone(),
        None => {
            eprintln!("ash agent check: missing command argument");
            eprintln!("usage: ash agent check \"<command>\"");
            return 2;
        }
    };

    let shell = crate::shell::Shell::new();
    // Parse the command into (name, args) using the existing helper.
    let parts = ash_core::cmd::external::parse_command(&command);
    if parts.is_empty() {
        let env = serde_json::json!({
            "command": command,
            "allowed": false,
            "decision": "deny",
            "denied_reasons": [{"rule_id": "empty-command", "message": "empty command"}],
        });
        println!("{}", serde_json::to_string_pretty(&env).unwrap());
        return 0;
    }
    let cmd_name = &parts[0];
    let cmd_args = &parts[1..];

    let is_external = shell.classify_is_external_pub(cmd_name);
    let result = shell.policy.check(cmd_name, cmd_args, is_external);
    let env = match result {
        Ok(ash_core::security::Decision::Allow) => serde_json::json!({
            "command": command,
            "allowed": true,
            "decision": "allow",
        }),
        Ok(ash_core::security::Decision::DryRun) => serde_json::json!({
            "command": command,
            "allowed": true,
            "decision": "dry_run",
            "note": "would be short-circuited under --dry-run",
        }),
        Err(e) => {
            let msg = format!("{}", e);
            serde_json::json!({
                "command": command,
                "allowed": false,
                "decision": "deny",
                "denied_reasons": [{
                    "rule_id": "security-policy",
                    "message": msg,
                }],
            })
        }
    };
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
    0
}

// ── helpers ──

fn parse_run_args(args: &[String]) -> Result<(String, Option<u64>, String), i32> {
    let mut command: Option<String> = None;
    let mut timeout: Option<u64> = None;
    let mut format = "json".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--timeout" => match args.get(i + 1) {
                Some(v) => {
                    timeout = Some(v.parse().map_err(|_| {
                        eprintln!("ash agent run: --timeout must be an integer");
                        2
                    })?);
                    i += 2;
                    continue;
                }
                None => {
                    eprintln!("ash agent run: --timeout requires a value");
                    return Err(2);
                }
            },
            "--format" => match args.get(i + 1).map(|s| s.as_str()) {
                Some("json") | Some("text") => {
                    format = args[i + 1].clone();
                    i += 2;
                    continue;
                }
                Some(_) => {
                    eprintln!("ash agent run: --format must be json|text");
                    return Err(2);
                }
                None => {
                    eprintln!("ash agent run: --format requires a value");
                    return Err(2);
                }
            },
            _ => {
                if command.is_none() {
                    command = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }
    match command {
        Some(c) => Ok((c, timeout, format)),
        None => {
            eprintln!("ash agent run: missing command argument");
            eprintln!("usage: ash agent run \"<command>\" [--timeout N] [--format json|text]");
            Err(2)
        }
    }
}

/// Heuristic error-classification from the error message string.
///
/// Used when `execute_for_agent` returns an `Err`. We match on common
/// substrings to pick an `ErrorKind` — not perfect, but good enough for
/// the Agent to choose a recovery strategy.
fn classify_error(msg: &str) -> ErrorKind {
    let lower = msg.to_lowercase();
    if lower.contains("no such file") || lower.contains("not found") {
        ErrorKind::NotFound
    } else if lower.contains("permission denied") {
        ErrorKind::PermissionDenied
    } else if lower.contains("security:") || lower.contains("denied") {
        ErrorKind::SandboxViolation
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ErrorKind::Timeout
    } else if lower.contains("invalid") || lower.contains("parse") {
        ErrorKind::InvalidArgs
    } else {
        ErrorKind::Internal
    }
}
