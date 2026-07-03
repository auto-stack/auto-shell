//! Plan 008 (MS2-A): Security policy framework.
//!
//! Central [`SecurityPolicy`] that intercepts commands *before* they spawn or
//! dispatch, giving AI agents a safe, auditable execution surface. A policy
//! with all fields empty/false is a no-op (full pass-through) — preserving
//! backward compatibility when no security flags are set.
//!
//! ## Decision pipeline
//!
//! Every command passes through [`SecurityPolicy::check`] before execution:
//! 1. Dangerous pattern detection (always — highest priority)
//! 2. allow/deny name lists
//! 3. Capability switches (`no_exec` / `no_network` / `read_only`)
//! 4. dry-run is handled by the caller (policy only reports intent)
//!
//! See `plans/008-ms2a-security-policy.md`.

use std::path::PathBuf;

/// External command names that perform network I/O. Used by `--no-network` to
/// block network-capable processes spawned via `external.rs`. (The built-in
/// `http_*` registry commands are blocked separately at dispatch, since they
/// call `curl` directly and bypass `external.rs`.)
pub const NETWORK_EXTERNALS: &[&str] = &[
    "curl", "wget", "ssh", "scp", "sftp", "rsync", "nc", "netcat", "ftp", "telnet",
    "tftp", "dig", "nslookup", "host", "ping", "traceroute", "tracepath",
];

/// Registry command names that perform network I/O (the `http_*` family).
/// `--no-network` blocks these at the registry dispatch layer.
pub const NETWORK_COMMANDS: &[&str] =
    &["http get", "http post", "http put", "http delete", "http head"];

/// Built-in/registry command names that write to the filesystem. `--read-only`
/// (command-name level, Plan 008) blocks these. Path-level write interception
/// is deferred to Plan 009 (`Shell::resolve_path` with `for_write`).
pub const WRITE_COMMANDS: &[&str] =
    &["rm", "mv", "cp", "mkdir", "touch", "ln", "tee", "truncate", "chmod", "chown"];

/// A centralized security policy. Stored on `Shell` and consulted before every
/// command spawn/dispatch. All fields default to "off" — a freshly-constructed
/// policy is a complete no-op (full pass-through, backward compatible).
#[derive(Debug, Clone, Default)]
pub struct SecurityPolicy {
    /// Command-name allow-list. When non-empty, **only** listed commands may
    /// run (default-deny). CLI `--allow <cmd>` appends here.
    pub allow: Vec<String>,
    /// Command-name deny-list. Any match is refused. CLI `--deny <cmd>`.
    pub deny: Vec<String>,
    /// Disable all external process execution (`--no-exec`). Built-in and
    /// registry commands still run.
    pub no_exec: bool,
    /// Disable network-capable commands (`--no-network`): the `http_*` family
    /// plus the externals in [`NETWORK_EXTERNALS`].
    pub no_network: bool,
    /// Read-only mode (`--read-only`). Plan 008 blocks known write commands by
    /// name; Plan 009 adds path-level write interception.
    pub read_only: bool,
    /// Dry-run mode (`--dry-run`). The caller short-circuits execution of
    /// writing/spawning commands and prints intent instead.
    pub dry_run: bool,
    /// Audit log destination. When set, every checked command is appended as a
    /// JSON line (`--audit <file>`).
    pub audit_file: Option<PathBuf>,
    /// Plan 009: path sandbox root. When set, all file operations (read/write/
    /// cd) are confined to this directory (after canonicalization). Symlinks
    /// that resolve outside the sandbox are refused. CLI `--sandbox <dir>`.
    pub sandbox_dir: Option<PathBuf>,
}

/// The outcome of a policy check for a single command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Command may proceed normally.
    Allow,
    /// Command is a write/spawn operation and `dry_run` is on — the caller
    /// should print intent and skip actual execution.
    DryRun,
}

impl SecurityPolicy {
    /// Returns true when *any* security restriction is active. When false,
    /// callers may skip [`check`](Self::check) entirely (fast path / backward
    /// compat).
    pub fn active(&self) -> bool {
        !self.allow.is_empty()
            || !self.deny.is_empty()
            || self.no_exec
            || self.no_network
            || self.read_only
            || self.dry_run
            || self.audit_file.is_some()
    }

    /// Check a command against the policy **before** it runs.
    ///
    /// `cmd_name` is the resolved command name (e.g. `"ls"`, `"http get"`).
    /// `args` are the remaining arguments. `is_external` is true when the
    /// command will be spawned as an external process (gates `no_exec`).
    ///
    /// Returns `Ok(Allow)` to proceed, `Ok(DryRun)` to short-circuit with a
    /// printed intent, or `Err` to refuse (caller surfaces stderr + non-zero
    /// exit). Errors carry a human-readable reason.
    pub fn check(
        &self,
        cmd_name: &str,
        args: &[String],
        is_external: bool,
    ) -> miette::Result<Decision> {
        // ② Dangerous patterns (always checked, highest priority).
        if is_dangerous(cmd_name, args) {
            miette::bail!(
                "security: refused dangerous command '{} {}' (matches dangerous-pattern list)",
                cmd_name,
                args.join(" ")
            );
        }

        // ③ allow/deny.
        if self.deny.iter().any(|d| d == cmd_name) {
            miette::bail!("security: '{}' is denied by --deny", cmd_name);
        }
        if !self.allow.is_empty() && !self.allow.iter().any(|a| a == cmd_name) {
            miette::bail!(
                "security: '{}' not in allow-list (default-deny active)",
                cmd_name
            );
        }

        // ④ Capability switches.
        if self.no_exec && is_external {
            miette::bail!(
                "security: external command '{}' blocked by --no-exec",
                cmd_name
            );
        }
        if self.no_network && is_network(cmd_name, is_external) {
            miette::bail!(
                "security: network command '{}' blocked by --no-network",
                cmd_name
            );
        }
        if self.read_only && is_write_command(cmd_name) {
            miette::bail!(
                "security: write command '{}' blocked by --read-only",
                cmd_name
            );
        }

        // ⑤ dry-run: short-circuit writing/spawning commands.
        if self.dry_run && (is_write_command(cmd_name) || is_external) {
            return Ok(Decision::DryRun);
        }

        Ok(Decision::Allow)
    }

    /// Append a JSON-lines audit record to the configured file (if any).
    /// Best-effort: a write failure is surfaced as a warning on stderr but
    /// does not abort the command.
    pub fn audit(&self, record: &AuditRecord) {
        let Some(ref path) = self.audit_file else {
            return;
        };
        let line = record.to_jsonl();
        use std::io::Write;
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(line.as_bytes()).and_then(|_| f.write_all(b"\n")));
        if let Err(e) = result {
            eprintln!("security: failed to write audit log {}: {}", path.display(), e);
        }
    }
}

/// True if the command is network-capable. Registry `http_*` names (multi-word)
/// and known network externals match.
fn is_network(cmd_name: &str, is_external: bool) -> bool {
    if NETWORK_COMMANDS.contains(&cmd_name) {
        return true;
    }
    if is_external && NETWORK_EXTERNALS.contains(&cmd_name) {
        return true;
    }
    false
}

/// True if the command is a known filesystem writer (Plan 008 name-level set).
fn is_write_command(cmd_name: &str) -> bool {
    // Normalize multi-word registry names to their first token for matching
    // against WRITE_COMMANDS (e.g. "http post" is network, not write).
    let name = cmd_name.split_whitespace().next().unwrap_or(cmd_name);
    WRITE_COMMANDS.contains(&name)
}

/// Detect known dangerous command patterns. Conservative: prefers false
/// negatives over false positives (we do not want to block `rm -rf /tmp/old`).
pub fn is_dangerous(cmd_name: &str, args: &[String]) -> bool {
    let name = cmd_name.split_whitespace().next().unwrap_or(cmd_name);
    match name {
        "rm" => {
            // Inspect each arg; flags may be grouped (-rf, -fr) or separate
            // (-r -f). Collect the set of short flags seen.
            let mut has_recursive = false;
            let mut has_force = false;
            let mut targets: Vec<&str> = Vec::new();
            for a in args {
                if let Some(rest) = a.strip_prefix('-') {
                    // A short-flag group like "-rf" or "--recursive".
                    if rest.starts_with('-') {
                        // long flag: match by name
                        let long = rest.trim_start_matches('-');
                        if long == "recursive" {
                            has_recursive = true;
                        } else if long == "force" {
                            has_force = true;
                        }
                    } else {
                        // short-flag group: each char is a flag
                        for c in rest.chars() {
                            match c {
                                'r' | 'R' => has_recursive = true,
                                'f' => has_force = true,
                                _ => {}
                            }
                        }
                    }
                } else {
                    targets.push(a.as_str());
                }
            }
            // `rm -rf /` / `rm -rf /*` — wipe root.
            let targets_root = targets.iter().any(|t| {
                *t == "/" || *t == "/*" || t.starts_with("/*")
            });
            if has_recursive && has_force && targets_root {
                return true;
            }
            // `rm -rf ~` / `rm -rf ~/*` / `rm -rf $HOME`
            let targets_home = targets.iter().any(|t| {
                *t == "~" || *t == "~/*" || t.starts_with("~/")
                    || *t == "$HOME" || t.starts_with("$HOME")
            });
            if has_recursive && has_force && targets_home {
                return true;
            }
            false
        }
        "mkfs" => true,
        "dd" => {
            // dd if=... of=/dev/sd* — writing to a raw block device.
            let joined = args.join(" ");
            joined.contains("of=/dev/sd")
                || joined.contains("of=/dev/nvm")
                || joined.contains("of=/dev/hd")
                || joined.contains("of=/dev/disk")
        }
        _ => {
            // fork bomb `:(){ :|:& };:` — detect the signature token sequence.
            let joined = args.join(" ");
            joined.contains(":(){") || joined.contains(":|:&")
        }
    }
}

/// A single audit-log entry, serialized as one JSON line.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub command: String,
    pub timestamp: String,
    pub decision: String,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
}

impl AuditRecord {
    /// Render as a single JSON line (no trailing newline).
    pub fn to_jsonl(&self) -> String {
        // Hand-built JSON (ash-core avoids serde_json to stay dependency-light).
        format!(
            r#"{{"cmd":{},"ts":{},"decision":{},"exit":{}{}}}"#,
            json_string(&self.command),
            json_string(&self.timestamp),
            json_string(&self.decision),
            match self.exit_code {
                Some(c) => c.to_string(),
                None => "null".to_string(),
            },
            self.reason
                .as_ref()
                .map(|r| format!(r#","reason":{}"#, json_string(r)))
                .unwrap_or_default(),
        )
    }
}

/// Minimal JSON string escaping (no external dep).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!(r"\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    // ---- active() / no-op fast path ----

    #[test]
    fn default_policy_is_inactive_noop() {
        let p = SecurityPolicy::default();
        assert!(!p.active(), "default policy must be inactive");
        // A check on an inactive policy still returns Allow.
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
    }

    // ---- allow / deny ----

    #[test]
    fn deny_blocks_matching_command() {
        let p = SecurityPolicy {
            deny: vec!["rm".into()],
            ..Default::default()
        };
        assert!(p.check("rm", &[], false).is_err());
        // Non-denied commands still pass.
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
    }

    #[test]
    fn allow_list_defaults_deny() {
        let p = SecurityPolicy {
            allow: vec!["ls".into(), "cat".into()],
            ..Default::default()
        };
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
        assert!(p.check("rm", &[], false).is_err(), "rm not in allow-list");
    }

    #[test]
    fn empty_allow_does_not_default_deny() {
        // Empty allow-list must NOT enter default-deny mode (backward compat).
        let p = SecurityPolicy {
            allow: vec![],
            deny: vec!["rm".into()],
            ..Default::default()
        };
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
    }

    // ---- capability switches ----

    #[test]
    fn no_exec_blocks_external_only() {
        let p = SecurityPolicy {
            no_exec: true,
            ..Default::default()
        };
        assert!(p.check("git", &[], true).is_err(), "external blocked");
        assert_eq!(
            p.check("ls", &[], false).unwrap(),
            Decision::Allow,
            "builtin still allowed"
        );
    }

    #[test]
    fn no_network_blocks_http_commands() {
        let p = SecurityPolicy {
            no_network: true,
            ..Default::default()
        };
        assert!(
            p.check("http get", &[], false).is_err(),
            "http get blocked"
        );
        assert!(
            p.check("curl", &[], true).is_err(),
            "curl external blocked"
        );
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
    }

    #[test]
    fn read_only_blocks_write_commands() {
        let p = SecurityPolicy {
            read_only: true,
            ..Default::default()
        };
        assert!(p.check("touch", &["f".to_string()], false).is_err());
        assert!(p.check("rm", &["f".to_string()], false).is_err());
        assert_eq!(p.check("cat", &["f".to_string()], false).unwrap(), Decision::Allow);
    }

    // ---- dry-run ----

    #[test]
    fn dry_run_short_circuits_writes_and_externals() {
        let p = SecurityPolicy {
            dry_run: true,
            ..Default::default()
        };
        assert_eq!(p.check("touch", &["f".to_string()], false).unwrap(), Decision::DryRun);
        assert_eq!(p.check("git", &[], true).unwrap(), Decision::DryRun);
        // Read-only builtins still execute for real.
        assert_eq!(p.check("ls", &[], false).unwrap(), Decision::Allow);
    }

    // ---- dangerous patterns (always on) ----

    #[test]
    fn dangerous_rm_rf_root_is_blocked_even_without_policy() {
        // Dangerous patterns are detected by the helper regardless of policy.
        let args = arg("-rf /");
        assert!(is_dangerous("rm", &args));
    }

    #[test]
    fn dangerous_rm_rf_root_blocked_with_active_policy() {
        let p = SecurityPolicy {
            dry_run: true,
            ..Default::default()
        };
        let args = arg("-rf /");
        assert!(p.check("rm", &args, false).is_err(), "rm -rf / must be refused");
    }

    #[test]
    fn dangerous_rm_rf_tmp_is_not_blocked() {
        // Must not false-positive on `rm -rf /tmp/old`.
        let args = arg("-rf /tmp/old");
        assert!(!is_dangerous("rm", &args));
    }

    #[test]
    fn dangerous_rm_rf_home_blocked() {
        let args = arg("-rf ~");
        assert!(is_dangerous("rm", &args));
        let args = arg("-rf $HOME");
        assert!(is_dangerous("rm", &args));
    }

    #[test]
    fn dangerous_mkfs_always_blocked() {
        assert!(is_dangerous("mkfs", &arg("/dev/sda1")));
    }

    #[test]
    fn dangerous_dd_to_raw_device_blocked() {
        assert!(is_dangerous("dd", &arg("if=x of=/dev/sda")));
        // dd to a normal file is fine.
        assert!(!is_dangerous("dd", &arg("if=x of=img.bin")));
    }

    #[test]
    fn dangerous_fork_bomb_blocked() {
        assert!(is_dangerous("sh", &arg("-c :(){ :|:& };:")));
    }

    // ---- dangerous with inactive policy: check() still inspects ----

    #[test]
    fn inactive_policy_still_refuses_dangerous() {
        // Even an inactive policy must refuse dangerous commands: check()
        // guards are unconditional.
        let p = SecurityPolicy::default();
        let args = arg("-rf /");
        assert!(
            p.check("rm", &args, false).is_err(),
            "dangerous commands refused regardless of active()"
        );
    }

    // ---- audit ----

    #[test]
    fn audit_record_renders_valid_jsonl() {
        let r = AuditRecord {
            command: r#"echo "hi""#.into(),
            timestamp: "2026-07-02T12:00:00Z".into(),
            decision: "allowed".into(),
            exit_code: Some(0),
            reason: None,
        };
        let line = r.to_jsonl();
        assert!(line.starts_with('{'));
        assert!(line.contains(r#""cmd":"echo \"hi\"""#));
        assert!(line.contains(r#""decision":"allowed""#));
        assert!(line.contains(r#""exit":0"#));
        assert!(!line.contains('\n'), "no trailing newline in jsonl body");
    }

    #[test]
    fn audit_record_with_reason() {
        let r = AuditRecord {
            command: "rm".into(),
            timestamp: "t".into(),
            decision: "denied".into(),
            exit_code: None,
            reason: Some("blocked by --deny".into()),
        };
        let line = r.to_jsonl();
        assert!(line.contains(r#""exit":null"#));
        assert!(line.contains(r#""reason":"blocked by --deny""#));
    }

    #[test]
    fn audit_writes_to_file_when_configured() {
        let dir = std::env::temp_dir().join(format!(
            "ash-audit-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        let p = SecurityPolicy {
            audit_file: Some(path.clone()),
            ..Default::default()
        };
        p.audit(&AuditRecord {
            command: "ls".into(),
            timestamp: "t1".into(),
            decision: "allowed".into(),
            exit_code: Some(0),
            reason: None,
        });
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(r#""cmd":"ls""#));
        assert!(content.ends_with('\n'), "append newline");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audit_noop_without_file() {
        // No audit_file → no panic, no file written.
        let p = SecurityPolicy::default();
        p.audit(&AuditRecord {
            command: "ls".into(),
            timestamp: "t".into(),
            decision: "allowed".into(),
            exit_code: Some(0),
            reason: None,
        });
    }

    // ---- helper predicates ----

    #[test]
    fn is_network_predicates() {
        assert!(is_network("http get", false));
        assert!(is_network("curl", true));
        assert!(!is_network("ls", false));
        assert!(!is_network("git", true));
    }

    #[test]
    fn is_write_command_handles_multiword() {
        assert!(is_write_command("touch"));
        assert!(is_write_command("rm"));
        // Multi-word: take first token.
        assert!(!is_write_command("http post"));
    }
}
