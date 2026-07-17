//! Plan 028: Tool Registry — the unified description layer for AI Agents.
//!
//! A `Tool` is a single invocable unit that an AI Agent (external CLI like
//! Claude Code, or the built-in F4 chat loop) can call. Every one of ash's
//! 80 built-in commands becomes a Tool via `CommandToolBridge` (in auto-shell).
//!
//! See `designs/028-agent-execution-engine.md` for the full design.

pub mod agent_loop;
pub mod atom_kind;
pub mod catalog;
pub mod error;
pub mod schema;
pub mod value_convert;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::pipeline::AtomPipeline;
use crate::security::SecurityPolicy;
pub use error::{DeniedReason, ErrorKind, ToolError};

/// A capability that a Tool may require. Used by the policy layer to decide
/// whether to allow execution without running the full command parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub reads_fs: bool,
    pub writes_fs: bool,
    pub spawns_process: bool,
    pub uses_network: bool,
}

/// How the tool layer should handle a policy denial.
/// External Agents default to `None` (silent deny); the built-in F4 loop
/// defaults to `Interactive` (prompt the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationMode {
    /// Silent: obey policy, return Denied on violation.
    None,
    /// On dangerous ops, surface a y/n prompt to the user (REPL only).
    Interactive,
    /// Ask the user before every tool call (paranoid mode).
    AlwaysConfirm,
}

impl Default for ConfirmationMode {
    fn default() -> Self {
        ConfirmationMode::None
    }
}

/// Hard limits to prevent an Agent from running away (e.g. cat-ing a 10GB log).
#[derive(Debug, Clone)]
pub struct OutputLimits {
    pub max_output_bytes: usize,
    pub max_command_seconds: u64,
    pub max_recursion_depth: u32,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 1_048_576, // 1 MiB
            max_command_seconds: 60,
            max_recursion_depth: 8,
        }
    }
}

/// Per-invocation context passed to every `Tool::invoke`.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub policy: SecurityPolicy,
    pub env: HashMap<String, String>,
    pub output_format: OutputFormat,
    pub timeout: Option<Duration>,
    pub confirmation_mode: ConfirmationMode,
    pub limits: OutputLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Atom,
    Text,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            policy: SecurityPolicy::default(),
            env: std::env::vars().collect(),
            output_format: OutputFormat::Json,
            timeout: None,
            confirmation_mode: ConfirmationMode::None,
            limits: OutputLimits::default(),
        }
    }
}

/// The data payload of a successful ToolResult.
///
/// Note: we deliberately do NOT carry `AtomPipeline` here, because
/// `AtomPipeline` does not implement `Clone` (and we avoid touching existing
/// pipeline types). Commands that produce Atoms convert them to JSON at the
/// call site via `value_convert::auto_value_to_json`, then hand the JSON
/// here. (Future in-process loop work may revisit this — see Plan 029.)
#[derive(Debug, Clone)]
pub enum ToolData {
    /// Already-schema'd structured JSON.
    Json(Value),
    /// Plain text (legacy/external command output).
    Text(String),
    /// No data (side-effect-only commands like `mkdir`).
    Empty,
}

impl ToolData {
    /// Convert to a JSON value for the response envelope.
    pub fn into_json(self) -> Value {
        match self {
            ToolData::Json(v) => v,
            ToolData::Text(s) => Value::String(s),
            ToolData::Empty => Value::Null,
        }
    }

    /// Best-effort conversion of an AtomPipeline to a ToolData::Json. Commands
    /// that produce Atoms call this to bridge into the envelope path. Exposed
    /// here so callers don't need to know about `value_convert`.
    pub fn from_atom_pipeline(atom: &AtomPipeline) -> Self {
        ToolData::Json(atom_pipeline_to_json(atom))
    }
}

/// Best-effort conversion of an AtomPipeline to JSON. Structured Atoms carry
/// an `auto_val::Value` (ash's own value type); we convert it to serde_json
/// via `value_convert::auto_value_to_json`. (Does NOT touch the Atom type
/// itself — no derive added.)
pub fn atom_pipeline_to_json(atom: &AtomPipeline) -> Value {
    match atom {
        AtomPipeline::Atom(a) => value_convert::auto_value_to_json(&a.value),
        AtomPipeline::Text(s) => Value::String(s.clone()),
        AtomPipeline::Empty => Value::Null,
        AtomPipeline::Stream(_) | AtomPipeline::ExternalStream(_) => {
            // Streams must be collected before serialization; if not, degrade
            // to null. (Callers should collect_stream() first.)
            Value::Null
        }
    }
}

/// The outcome category of a Tool invocation.
#[derive(Debug, Clone)]
pub enum ToolStatus {
    /// Fully succeeded.
    Success,
    /// Policy denied this call. Agent should stop retrying this exact call.
    Denied(DeniedReason),
    /// Execution failed. Agent may retry or try a different approach.
    Failed(ErrorKind, String),
    /// Partially completed (e.g. dry-run, or some items in a batch failed).
    PartialSuccess(String),
}

/// Timing info, included in every ToolResult so Agents can make cost decisions.
#[derive(Debug, Clone, Default)]
pub struct Timing {
    pub wall_ms: u64,
    pub user_ms: u64,
    pub sys_ms: u64,
}

/// A non-fatal warning attached to a successful result.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Info,
}

/// The full result of a Tool invocation.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: ToolData,
    pub diagnostics: Vec<Diagnostic>,
    pub timing: Timing,
}

impl ToolResult {
    /// Convenience: a successful result carrying JSON data.
    pub fn success_json(value: Value) -> Self {
        Self {
            status: ToolStatus::Success,
            data: ToolData::Json(value),
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a successful result carrying text.
    pub fn success_text(text: impl Into<String>) -> Self {
        Self {
            status: ToolStatus::Success,
            data: ToolData::Text(text.into()),
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a denied result.
    pub fn denied(reason: DeniedReason) -> Self {
        Self {
            status: ToolStatus::Denied(reason),
            data: ToolData::Empty,
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a failed result.
    pub fn failed(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            status: ToolStatus::Failed(kind, message.into()),
            data: ToolData::Empty,
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self.status, ToolStatus::Success)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self.status, ToolStatus::Denied(_))
    }
}

// ──────────────────────────────────────────────────────────────────────────
// The Tool trait
// ──────────────────────────────────────────────────────────────────────────

/// A single invocable unit that an AI Agent can call.
///
/// Implementations live in two places:
/// - `auto-shell::agent::meta` — the three meta-tools (run_command, check,
///   describe_policy).
/// - `auto-shell::tool_bridge` — `CommandToolBridge<T: Command>` wraps each of
///   the 80 existing commands automatically.
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "ls", "grep", "run_command").
    fn name(&self) -> &str;

    /// One-line description the LLM reads to decide when to call this tool.
    fn description(&self) -> &str;

    /// Parameters as a JSON Schema object.
    /// Shape: `{"type":"object", "properties":{...}, "required":[...]}`.
    fn parameters_schema(&self) -> Map<String, Value>;

    /// Output schema (optional). None means free-form output.
    fn output_schema(&self) -> Option<Map<String, Value>> {
        None
    }

    /// Execute the tool. `args` is the parsed JSON arguments object.
    fn invoke(&self, args: &Value, ctx: &ToolContext) -> ToolResult;

    /// Capabilities this tool needs (for pre-execution policy decisions).
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_data_json_passes_through() {
        let v = json!({"a": 1});
        let d = ToolData::Json(v.clone());
        assert_eq!(d.into_json(), v);
    }

    #[test]
    fn tool_data_text_becomes_string() {
        let d = ToolData::Text("hello".into());
        assert_eq!(d.into_json(), Value::String("hello".into()));
    }

    #[test]
    fn tool_data_empty_becomes_null() {
        let d = ToolData::Empty;
        assert_eq!(d.into_json(), Value::Null);
    }

    #[test]
    fn tool_result_success_json_helper() {
        let r = ToolResult::success_json(json!([1, 2, 3]));
        assert!(r.is_success());
        assert!(matches!(r.data, ToolData::Json(_)));
    }

    #[test]
    fn tool_result_denied_helper() {
        let r = ToolResult::denied(DeniedReason::new("test-rule", "nope"));
        assert!(r.is_denied());
        assert!(!r.is_success());
    }

    #[test]
    fn output_limits_defaults_are_sane() {
        let l = OutputLimits::default();
        assert_eq!(l.max_output_bytes, 1_048_576);
        assert_eq!(l.max_command_seconds, 60);
        assert_eq!(l.max_recursion_depth, 8);
    }

    #[test]
    fn confirmation_mode_default_is_none() {
        assert_eq!(ConfirmationMode::default(), ConfirmationMode::None);
    }

    /// A minimal Tool impl for testing the trait shape end-to-end.
    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo_test"
        }
        fn description(&self) -> &str {
            "test echo"
        }
        fn parameters_schema(&self) -> Map<String, Value> {
            let mut m = Map::new();
            m.insert("type".into(), "object".into());
            m.insert("properties".into(), json!({}).into());
            m
        }
        fn invoke(&self, args: &Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::success_json(args.clone())
        }
    }

    #[test]
    fn tool_trait_can_be_implemented_and_invoked() {
        let t = EchoTool;
        let ctx = ToolContext::default();
        let args = json!({"msg": "hi"});
        let result = t.invoke(&args, &ctx);
        assert!(result.is_success());
        assert_eq!(result.data.into_json(), args);
    }
}
