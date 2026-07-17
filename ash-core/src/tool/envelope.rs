//! Plan 028: The response envelope returned by `ash agent run`.
//!
//! Shape (see designs/028 §4.2):
//! ```json
//! {
//!   "schema_version": "1",
//!   "status": "success" | "failed" | "denied" | "partial",
//!   "data": { "kind": "...", "atom_type": "...", "value": ...,
//!             "pipeline_hint": "...", "truncation": {...} },
//!   "error": { ... } | null,
//!   "diagnostics": [...],
//!   "timing": { "wall_ms": ..., "user_ms": ..., "sys_ms": ... },
//!   "command_echo": "..."
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pipeline::AtomType;
use crate::tool::atom_kind::{atom_type_name, atom_type_to_kind};
use crate::tool::{DeniedReason, Diagnostic, DiagnosticLevel, ErrorKind, Timing, ToolError, ToolResult, ToolStatus};

pub const ENVELOPE_SCHEMA_VERSION: &str = "1";

/// Truncation info attached when output exceeded `max_output_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Truncation {
    pub truncated: bool,
    pub original_bytes: usize,
    pub returned_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_hint: Option<String>,
}

/// The `data` block of a success response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeData {
    /// snake_case semantic kind (e.g. "file_list").
    pub kind: String,
    /// PascalCase AtomType name (e.g. "FileList").
    pub atom_type: String,
    /// The actual payload.
    pub value: Value,
    /// Hint about what DSL ops this output can be piped into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
}

impl EnvelopeData {
    /// Build the data block from an AtomType and its JSON value.
    pub fn from_atom(atom_type: AtomType, value: Value) -> Self {
        Self {
            kind: atom_type_to_kind(atom_type).to_string(),
            atom_type: atom_type_name(atom_type).to_string(),
            value,
            pipeline_hint: pipeline_hint_for(atom_type),
            truncation: None,
        }
    }

    /// Plain-text data block (kind="text").
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            kind: "text".to_string(),
            atom_type: "Text".to_string(),
            value: Value::String(content.into()),
            pipeline_hint: Some("pipeable to grep/head/tail/wc".to_string()),
            truncation: None,
        }
    }

    /// Empty data block (kind="empty", for side-effect commands like mkdir).
    pub fn empty() -> Self {
        Self {
            kind: "empty".to_string(),
            atom_type: "Nothing".to_string(),
            value: Value::Null,
            pipeline_hint: None,
            truncation: None,
        }
    }
}

fn pipeline_hint_for(t: AtomType) -> Option<String> {
    match t {
        AtomType::FileList | AtomType::FileEntry => {
            Some("pipeable to filter/sort/select (e.g. filter .size > 1k)".into())
        }
        AtomType::ProcessList | AtomType::ProcessEntry => {
            Some("pipeable to filter/sort/select (e.g. filter .cpu > 1.0)".into())
        }
        AtomType::Table => Some("pipeable to select <cols> or to_csv".into()),
        AtomType::Record => Some("pipeable to get <field>".into()),
        AtomType::Text => Some("pipeable to grep/head/tail/wc".into()),
        _ => None,
    }
}

/// A diagnostic in the envelope (warning/info attached to a result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeDiagnostic {
    pub level: String, // "warning" | "info"
    pub message: String,
}

impl From<&Diagnostic> for EnvelopeDiagnostic {
    fn from(d: &Diagnostic) -> Self {
        Self {
            level: match d.level {
                DiagnosticLevel::Warning => "warning",
                DiagnosticLevel::Info => "info",
            }
            .to_string(),
            message: d.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeTiming {
    pub wall_ms: u64,
    pub user_ms: u64,
    pub sys_ms: u64,
}

impl From<&Timing> for EnvelopeTiming {
    fn from(t: &Timing) -> Self {
        Self {
            wall_ms: t.wall_ms,
            user_ms: t.user_ms,
            sys_ms: t.sys_ms,
        }
    }
}

/// The top-level status string in the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeStatus {
    Success,
    Failed,
    Denied,
    Partial,
}

impl EnvelopeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvelopeStatus::Success => "success",
            EnvelopeStatus::Failed => "failed",
            EnvelopeStatus::Denied => "denied",
            EnvelopeStatus::Partial => "partial",
        }
    }
}

/// Build the final envelope JSON from a ToolResult + command echo.
///
/// This is the single function that turns an in-process ToolResult into the
/// wire format Agents consume.
///
/// Data block logic:
/// - For Success/Partial: if the ToolData::Json already carries an object with
///   a "kind" field (i.e. the caller pre-built an EnvelopeData), pass it
///   through. Otherwise wrap raw text as EnvelopeData::text, or wrap raw JSON
///   as a generic Record.
/// - For Failed/Denied: data is null, error is populated.
pub fn build_envelope(result: &ToolResult, command_echo: &str) -> Value {
    use serde_json::json;

    let (status, error_block) = match &result.status {
        ToolStatus::Success => (EnvelopeStatus::Success, None),
        ToolStatus::PartialSuccess(msg) => (
            EnvelopeStatus::Partial,
            Some(json!({ "partial_message": msg })),
        ),
        ToolStatus::Denied(reason) => (
            EnvelopeStatus::Denied,
            Some(serde_json::to_value(reason).unwrap_or(Value::Null)),
        ),
        ToolStatus::Failed(kind, msg) => {
            let err = ToolError::new(*kind, msg.as_str(), command_echo);
            (
                EnvelopeStatus::Failed,
                Some(serde_json::to_value(&err).unwrap_or(Value::Null)),
            )
        }
    };

    // Build the data block.
    let data_field = if status == EnvelopeStatus::Success || status == EnvelopeStatus::Partial {
        match &result.data {
            crate::tool::ToolData::Json(v) => {
                if v.get("kind").and_then(|k| k.as_str()).is_some() {
                    // Caller pre-built an EnvelopeData-shaped object; pass through.
                    v.clone()
                } else {
                    // Wrap raw JSON value as a generic Record.
                    serde_json::to_value(EnvelopeData::from_atom(AtomType::Record, v.clone()))
                        .unwrap_or(Value::Null)
                }
            }
            crate::tool::ToolData::Text(s) => {
                serde_json::to_value(EnvelopeData::text(s)).unwrap_or(Value::Null)
            }
            crate::tool::ToolData::Empty => {
                serde_json::to_value(EnvelopeData::empty()).unwrap_or(Value::Null)
            }
        }
    } else {
        Value::Null
    };

    json!({
        "schema_version": ENVELOPE_SCHEMA_VERSION,
        "status": status.as_str(),
        "data": data_field,
        "error": error_block,
        "diagnostics": result.diagnostics.iter().map(EnvelopeDiagnostic::from).collect::<Vec<_>>(),
        "timing": serde_json::to_value(EnvelopeTiming::from(&result.timing)).unwrap_or(Value::Null),
        "command_echo": command_echo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{DeniedReason, ToolData, ToolResult, ToolStatus};
    use serde_json::json;

    #[test]
    fn success_envelope_has_schema_version_and_status() {
        let r = ToolResult::success_text("hello");
        let env = build_envelope(&r, "echo hello");
        assert_eq!(env["schema_version"], "1");
        assert_eq!(env["status"], "success");
        assert_eq!(env["data"]["kind"], "text");
        assert_eq!(env["data"]["value"], "hello");
        assert_eq!(env["command_echo"], "echo hello");
        assert!(env["error"].is_null());
    }

    #[test]
    fn success_with_empty_data_has_empty_kind() {
        let r = ToolResult {
            status: ToolStatus::Success,
            data: ToolData::Empty,
            diagnostics: vec![],
            timing: Timing::default(),
        };
        let env = build_envelope(&r, "mkdir foo");
        assert_eq!(env["status"], "success");
        assert_eq!(env["data"]["kind"], "empty");
        assert!(env["data"]["value"].is_null());
    }

    #[test]
    fn success_with_prebuilt_envelope_data_passes_through() {
        // When the caller already built a {kind, atom_type, value} object,
        // build_envelope must NOT re-wrap it.
        let prebuilt = json!({
            "kind": "file_list",
            "atom_type": "FileList",
            "value": [{"name": "a.txt"}]
        });
        let r = ToolResult::success_json(prebuilt.clone());
        let env = build_envelope(&r, "ls");
        assert_eq!(env["data"]["kind"], "file_list");
        assert_eq!(env["data"]["value"][0]["name"], "a.txt");
    }

    #[test]
    fn success_with_raw_json_wraps_as_record() {
        // Raw JSON without a "kind" field gets wrapped as a generic Record.
        let raw = json!({"foo": "bar"});
        let r = ToolResult::success_json(raw);
        let env = build_envelope(&r, "some cmd");
        assert_eq!(env["data"]["kind"], "record");
        assert_eq!(env["data"]["atom_type"], "Record");
        assert_eq!(env["data"]["value"]["foo"], "bar");
    }

    #[test]
    fn denied_envelope_has_denied_reason() {
        let r = ToolResult::denied(
            DeniedReason::new("path-outside-sandbox", "denied")
                .with_remediation("use /sandbox/x"),
        );
        let env = build_envelope(&r, "rm /etc/foo");
        assert_eq!(env["status"], "denied");
        assert_eq!(env["error"]["rule_id"], "path-outside-sandbox");
        assert_eq!(env["error"]["remediation"], "use /sandbox/x");
        assert!(env["data"].is_null());
    }

    #[test]
    fn failed_envelope_has_error_kind() {
        let r = ToolResult::failed(ErrorKind::NotFound, "no such file");
        let env = build_envelope(&r, "cat missing.txt");
        assert_eq!(env["status"], "failed");
        assert_eq!(env["error"]["kind"], "not_found");
        assert_eq!(env["error"]["message"], "no such file");
        assert_eq!(env["error"]["command"], "cat missing.txt");
    }

    #[test]
    fn envelope_data_from_atom_sets_kind_and_atom_type() {
        let d = EnvelopeData::from_atom(AtomType::FileList, json!([{"name": "a"}]));
        assert_eq!(d.kind, "file_list");
        assert_eq!(d.atom_type, "FileList");
        assert!(d.pipeline_hint.is_some());
    }

    #[test]
    fn envelope_data_text_has_hint() {
        let d = EnvelopeData::text("body");
        assert_eq!(d.kind, "text");
        assert!(d.pipeline_hint.unwrap().contains("grep"));
    }

    #[test]
    fn envelope_data_empty_is_null_value() {
        let d = EnvelopeData::empty();
        assert_eq!(d.kind, "empty");
        assert_eq!(d.value, Value::Null);
        assert!(d.pipeline_hint.is_none());
    }
}
