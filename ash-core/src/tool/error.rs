//! Plan 028: Standardized error type for all Tool invocations.
//!
//! Single source of truth: CLI path, in-process agent loop, and ToolRegistry
//! internals all produce/consume `ToolError`. The `ErrorKind` enum lets an
//! Agent match on recovery strategy without parsing free-text.

use serde::{Deserialize, Serialize};

/// The category of a tool failure. Agents match on this to choose recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Command executed but returned non-zero exit.
    NonzeroExit,
    /// Command name or file path not found.
    NotFound,
    /// OS-level permission denied (NOT sandbox).
    PermissionDenied,
    /// Argument parsing failed.
    InvalidArgs,
    /// Command exceeded its time limit.
    Timeout,
    /// Command hit the SecurityPolicy (sandbox / read-only / no-network).
    SandboxViolation,
    /// Output parsing failed (e.g. from_json on invalid JSON).
    ParseError,
    /// ash internal bug (should not happen; report it).
    Internal,
}

impl ErrorKind {
    /// snake_case string form (matches serde serialization).
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::NonzeroExit => "nonzero_exit",
            ErrorKind::NotFound => "not_found",
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::InvalidArgs => "invalid_args",
            ErrorKind::Timeout => "timeout",
            ErrorKind::SandboxViolation => "sandbox_violation",
            ErrorKind::ParseError => "parse_error",
            ErrorKind::Internal => "internal",
        }
    }
}

/// A standardized tool error. Serialized into the `error` field of the
/// response envelope (see designs/028 §4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub kind: ErrorKind,
    pub message: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
}

impl ToolError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            command: command.into(),
            exit_code: None,
            remediation: None,
            stderr_excerpt: None,
        }
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        let s: String = stderr.into();
        // Cap at 500 chars to avoid bloating Agent context.
        let excerpt = if s.len() > 500 {
            format!("{}...(truncated)", &s[..500])
        } else {
            s
        };
        self.stderr_excerpt = Some(excerpt);
        self
    }
}

/// Why a command was denied by the security policy. Distinct from `ToolError`
/// because denials are a first-class `ToolStatus::Denied`, not a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeniedReason {
    /// Stable rule identifier, e.g. "path-outside-sandbox".
    pub rule_id: String,
    /// Human-readable explanation.
    pub message: String,
    /// Machine-readable recovery suggestion (Agent-actionable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl DeniedReason {
    pub fn new(rule_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.into(),
            message: message.into(),
            remediation: None,
        }
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_serializes_to_snake_case() {
        let kind = ErrorKind::NonzeroExit;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"nonzero_exit\"");
    }

    #[test]
    fn error_kind_roundtrips() {
        for kind in [
            ErrorKind::NonzeroExit,
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::InvalidArgs,
            ErrorKind::Timeout,
            ErrorKind::SandboxViolation,
            ErrorKind::ParseError,
            ErrorKind::Internal,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ErrorKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind, "roundtrip failed for {:?}", kind);
        }
    }

    #[test]
    fn tool_error_omits_none_fields() {
        let e = ToolError::new(ErrorKind::NotFound, "no such file", "cat missing.txt");
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("exit_code"));
        assert!(!json.contains("remediation"));
        assert!(!json.contains("stderr_excerpt"));
        assert!(json.contains("\"kind\":\"not_found\""));
        assert!(json.contains("\"command\":\"cat missing.txt\""));
    }

    #[test]
    fn tool_error_includes_fields_when_set() {
        let e = ToolError::new(ErrorKind::NonzeroExit, "exit 2", "grep")
            .with_exit_code(2)
            .with_remediation("try: grep -i")
            .with_stderr("long stderr output");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"exit_code\":2"));
        assert!(json.contains("\"remediation\":\"try: grep -i\""));
        assert!(json.contains("\"stderr_excerpt\""));
    }

    #[test]
    fn stderr_is_truncated_at_500_chars() {
        let long = "x".repeat(1000);
        let e = ToolError::new(ErrorKind::Internal, "x", "x").with_stderr(&long);
        let excerpt = e.stderr_excerpt.as_ref().expect("stderr_excerpt should be set");
        assert!(excerpt.len() < 600); // 500 + suffix
        assert!(excerpt.contains("truncated"));
    }

    #[test]
    fn denied_reason_omits_remediation_when_none() {
        let d = DeniedReason::new("path-outside-sandbox", "denied");
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("remediation"));
    }
}
