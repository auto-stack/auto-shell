//! PLAN-081 (R2): JSON policy-file support (`--policy-file <file>`).
//!
//! A policy file is a single JSON document, schema version `"1"`:
//!
//! ```json
//! {
//!   "schema_version": "1",
//!   "sandbox": null,
//!   "writable": ["D:/proj/a", "D:/proj/b"],
//!   "read_only": false, "no_exec": false, "no_network": false, "dry_run": false,
//!   "allow": [], "deny": [],
//!   "audit": null
//! }
//! ```
//!
//! Ownership boundary (designs/038 §4.2): the agent side (auto-ai) merges
//! "built-in defaults (cwd) < project config < session approvals" and writes
//! this file; ash only loads and enforces it. The merge here is therefore the
//! simple documented rule — over a base policy: booleans OR (stricter wins),
//! name/root lists union (dedup), single-value paths override when present.
//! Precedence across layers: config < policy-file < CLI flags.
//!
//! The parser lives in `auto-shell` (which already depends on serde_json);
//! `ash-core` stays dependency-light by design.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Schema version this loader understands. A mismatch is a hard error —
/// silently ignoring a policy written for a newer schema would drop
/// restrictions (the 072 S-6 lesson: security config must fail loudly).
pub const SUPPORTED_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyFile {
    pub schema_version: String,
    /// Plan 009 single-root confinement (optional in the file).
    #[serde(default)]
    pub sandbox: Option<PathBuf>,
    /// PLAN-081 (R1): writable whitelist roots. Non-empty ⇒ writes
    /// default-deny, allowed only inside these roots.
    #[serde(default)]
    pub writable: Vec<PathBuf>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub no_exec: bool,
    #[serde(default)]
    pub no_network: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub audit: Option<PathBuf>,
}

impl PolicyFile {
    /// Load and validate a policy file. Missing/unreadable file, invalid
    /// JSON, unknown top-level shape, or a non-`"1"` `schema_version` are
    /// hard errors — callers must refuse to start rather than run unsandboxed.
    pub fn load(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            miette::miette!(
                "policy-file: cannot read {}: {}",
                path.display(),
                e
            )
        })?;
        let pf: PolicyFile = serde_json::from_str(&content).map_err(|e| {
            miette::miette!("policy-file: invalid JSON in {}: {}", path.display(), e)
        })?;
        if pf.schema_version != SUPPORTED_SCHEMA_VERSION {
            miette::bail!(
                "policy-file: unsupported schema_version {:?} in {} (supported: {:?})",
                pf.schema_version,
                path.display(),
                SUPPORTED_SCHEMA_VERSION
            );
        }
        Ok(pf)
    }

    /// Merge this file's settings over `base` (mutated in place).
    ///
    /// Rules (designs/038 §5.2): booleans OR — once a layer enables a
    /// restriction it stays on; lists union with dedup — layers only ever add
    /// allowed-roots/denied-commands, never remove; single-value paths
    /// (`sandbox`, `audit`) override when present — the more specific layer
    /// picks the location.
    pub fn merge_over(&self, base: &mut ash_core::security::SecurityPolicy) {
        if self.sandbox.is_some() {
            base.sandbox_dir = self.sandbox.clone();
        }
        for root in &self.writable {
            if !base.writable_roots.contains(root) {
                base.writable_roots.push(root.clone());
            }
        }
        base.read_only |= self.read_only;
        base.no_exec |= self.no_exec;
        base.no_network |= self.no_network;
        base.dry_run |= self.dry_run;
        for a in &self.allow {
            if !base.allow.contains(a) {
                base.allow.push(a.clone());
            }
        }
        for d in &self.deny {
            if !base.deny.contains(d) {
                base.deny.push(d.clone());
            }
        }
        if self.audit.is_some() {
            base.audit_file = self.audit.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ash-policy-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    fn base_policy() -> ash_core::security::SecurityPolicy {
        ash_core::security::SecurityPolicy::default()
    }

    #[test]
    fn loads_valid_v1_file() {
        let p = write_tmp(
            "ok.json",
            r#"{"schema_version":"1","writable":["/tmp/a","/tmp/b"],"no_network":true}"#,
        );
        let pf = PolicyFile::load(&p).unwrap();
        assert_eq!(pf.writable.len(), 2);
        assert!(pf.no_network);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let p = write_tmp(
            "bad-schema.json",
            r#"{"schema_version":"2"}"#,
        );
        let err = format!("{}", PolicyFile::load(&p).unwrap_err());
        assert!(err.contains("unsupported schema_version"), "{err}");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn rejects_invalid_json_and_missing_file() {
        let p = write_tmp("bad.json", "{ not json");
        assert!(PolicyFile::load(&p).is_err());
        assert!(PolicyFile::load(Path::new("Z:/no/such/policy.json")).is_err());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn merge_unions_lists_ors_bools_overrides_singles() {
        let pf = PolicyFile {
            schema_version: "1".into(),
            sandbox: Some(PathBuf::from("/box")),
            writable: vec!["/box/w".into(), "/box/shared".into()],
            read_only: false,
            no_exec: true,
            no_network: true,
            dry_run: false,
            allow: vec!["ls".into()],
            deny: vec!["rm".into(), "dd".into()],
            audit: Some(PathBuf::from("/box/audit.jsonl")),
        };
        let mut base = base_policy();
        base.no_exec = false;
        base.read_only = true; // base-only restriction must survive the merge
        base.writable_roots = vec!["/box/shared".into()]; // dedup target
        base.allow = vec!["cat".into()];
        base.deny = vec!["rm".into()];
        pf.merge_over(&mut base);

        assert_eq!(base.sandbox_dir, Some(PathBuf::from("/box")));
        assert_eq!(base.writable_roots.len(), 2, "roots union + dedup");
        assert!(base.no_exec, "file ORs onto base");
        assert!(base.read_only, "base-only restriction survives");
        assert_eq!(base.allow.len(), 2);
        assert_eq!(base.deny.len(), 2, "deny union dedups");
        assert_eq!(base.audit_file, Some(PathBuf::from("/box/audit.jsonl")));
        assert!(base.active(), "merged policy must count as active");
    }
}
