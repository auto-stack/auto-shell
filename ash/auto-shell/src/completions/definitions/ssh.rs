//! SSH/SCP completion specification

use ash_core::completions::spec::*;

pub fn ssh_spec() -> CompletionSpec {
    CompletionSpec::new("ssh")
        .desc("OpenSSH remote login client")
        .flag(FlagSpec::both("p", "port").desc("Port to connect to").takes_arg("port"))
        .flag(FlagSpec::both("i", "identity").desc("Identity file").takes_arg("file"))
        .flag(FlagSpec::both("v", "verbose").desc("Verbose mode"))
        .flag(FlagSpec::both("q", "quiet").desc("Quiet mode"))
        .flag(FlagSpec::both("C", "compress").desc("Enable compression"))
        .flag(FlagSpec::long("config").desc("Config file").takes_arg("file"))
        .flag(FlagSpec::both("L", "local-forward").desc("Local port forwarding").takes_arg("forward"))
        .flag(FlagSpec::both("R", "remote-forward").desc("Remote port forwarding").takes_arg("forward"))
        .flag(FlagSpec::both("N", "no-command").desc("No remote command"))
        .flag(FlagSpec::both("T", "no-tty").desc("Disable pseudo-TTY allocation"))
        .flag(FlagSpec::both("t", "tty").desc("Force pseudo-TTY allocation"))
        .flag(FlagSpec::both("o", "option").desc("Set an SSH option").takes_arg("option"))
        // Plan 032 M3.1: complete destination hosts from ~/.ssh/config +
        // ~/.ssh/known_hosts. Pure-Rust parse (no shell-out) so it works the
        // same on Windows and Unix; resolved to a Static source at spec build.
        .arg(ArgSpec::new(0).desc("Destination (user@host)").source(ssh_hosts_source()))
}

pub fn scp_spec() -> CompletionSpec {
    CompletionSpec::new("scp")
        .desc("Secure copy")
        .flag(FlagSpec::both("r", "recursive").desc("Copy directories recursively"))
        .flag(FlagSpec::both("P", "port").desc("Port").takes_arg("port"))
        .flag(FlagSpec::both("i", "identity").desc("Identity file").takes_arg("file"))
        .flag(FlagSpec::both("v", "verbose").desc("Verbose mode"))
        .flag(FlagSpec::both("C", "compress").desc("Enable compression"))
        .flag(FlagSpec::both("q", "quiet").desc("Quiet mode"))
        // Plan 032 M3.1: host completion for scp source/destination too.
        .arg(ArgSpec::new(0).desc("Source").source(ssh_hosts_source()))
        .arg(ArgSpec::new(1).desc("Destination").source(ssh_hosts_source()))
}

/// Build a [`CompletionSource`] listing SSH host aliases known to this user
/// (Plan 032 M3.1).
///
/// Resolved eagerly to a `Static` list by parsing `~/.ssh/config` `Host` lines
/// and `~/.ssh/known_hosts` first columns. Wildcard host patterns (`*`, `?`)
/// from ssh config are skipped — they aren't completable destinations.
fn ssh_hosts_source() -> CompletionSource {
    CompletionSource::Static(ssh_hosts())
}

/// Collect SSH host aliases from the user's ssh config + known_hosts.
///
/// Reads `$HOME/.ssh/config` (lines like `Host alias1 alias2`) and
/// `$HOME/.ssh/known_hosts` (first whitespace field of each line). Returns a
/// deduplicated, alphabetically sorted list. Best-effort: any read error or
/// missing file contributes nothing (the completion just offers fewer hosts).
pub(crate) fn ssh_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();

    if let Some(ssh_dir) = ssh_dir() {
        // ~/.ssh/config — collect Host aliases (skip wildcards).
        let config = ssh_dir.join("config");
        if let Ok(content) = std::fs::read_to_string(&config) {
            for line in content.lines() {
                let line = line.trim();
                // ssh config keywords are case-insensitive; match `Host`/`host`.
                let lower = line.to_ascii_lowercase();
                if let Some(rest) = lower.strip_prefix("host ") {
                    // Operate on the ORIGINAL line to preserve the aliases'
                    // case, but only over the host-alias region (same length
                    // as `rest` after the keyword).
                    let aliases_region = &line[5..]; // skip "Host " (case varies)
                    // Skip if the slice math doesn't line up (non-ASCII edge).
                    if aliases_region.len() >= rest.len() {
                        for alias in aliases_region.split_whitespace() {
                            if !is_wildcard_host(alias) {
                                push_unique(&mut hosts, alias.to_string());
                            }
                        }
                    }
                }
            }
        }

        // ~/.ssh/known_hosts — first field of each line (the host/key pattern).
        // Entries can be comma-separated lists or hashed (`|1|...`); we only
        // take plain hostnames to avoid garbage in the menu.
        let known = ssh_dir.join("known_hosts");
        if let Ok(content) = std::fs::read_to_string(&known) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('|') {
                    continue; // hashed host entry — not human-readable
                }
                if let Some(first) = line.split_whitespace().next() {
                    // known_hosts may list multiple hosts comma-separated.
                    for host in first.split(',') {
                        if !is_wildcard_host(host) {
                            push_unique(&mut hosts, host.to_string());
                        }
                    }
                }
            }
        }
    }

    hosts.sort();
    hosts.dedup();
    hosts
}

/// Resolve the user's `~/.ssh` directory across platforms.
fn ssh_dir() -> Option<std::path::PathBuf> {
    // On all platforms ssh lives under the home directory.
    let home = if cfg!(windows) {
        // Windows: %USERPROFILE% (dirs handles this), fall back to env.
        dirs::home_dir()
    } else {
        dirs::home_dir()
    };
    home.map(|h| h.join(".ssh"))
}

/// A host alias is worth completing only if it has no glob metacharacters
/// (ssh config `Host *` / `Host 192.168.*` patterns aren't destinations).
fn is_wildcard_host(host: &str) -> bool {
    host.contains('*') || host.contains('?') || host.contains('!')
}

/// Push `item` into `out` only if it isn't already present (case-sensitive).
fn push_unique(out: &mut Vec<String>, item: String) {
    if !out.iter().any(|h| h == &item) {
        out.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_spec_registers_destination_source() {
        // The spec's first arg must now carry a host source (previously it
        // had none — completing `ssh <TAB>` offered nothing).
        let spec = ssh_spec();
        assert_eq!(spec.command, "ssh");
        assert!(
            spec.args.iter().any(|a| a.source.is_some()),
            "ssh spec should now have a host completion source"
        );
    }

    #[test]
    fn scp_spec_registers_sources_on_both_args() {
        let spec = scp_spec();
        assert_eq!(spec.args.len(), 2);
        assert!(spec.args.iter().all(|a| a.source.is_some()));
    }

    #[test]
    fn wildcard_hosts_are_filtered() {
        assert!(is_wildcard_host("*"));
        assert!(is_wildcard_host("*.example.com"));
        assert!(is_wildcard_host("192.168.*"));
        assert!(is_wildcard_host("host?"));
        assert!(is_wildcard_host("!negated"));
        assert!(!is_wildcard_host("prod"));
        assert!(!is_wildcard_host("build-server-1"));
    }

    #[test]
    fn push_unique_dedupes() {
        let mut out = Vec::new();
        push_unique(&mut out, "a".into());
        push_unique(&mut out, "b".into());
        push_unique(&mut out, "a".into()); // dup, ignored
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn ssh_hosts_runs_without_crashing() {
        // In the test environment there's likely no ~/.ssh; the function must
        // degrade to an empty list rather than panic.
        let hosts = ssh_hosts();
        // Just assert it returned a Vec (may be empty on CI).
        let _ = hosts;
    }
}
