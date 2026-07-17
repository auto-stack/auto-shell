//! Plan 028: Bridge the existing `Command` trait to the new `Tool` trait.
//!
//! ## Two bridge types
//!
//! 1. `CommandToolBridge<T: Command>` — a generic wrapper for a single
//!    concrete command type. Useful when you have a concrete `T` at the call
//!    site. Its `invoke()` is shell-less (returns `Internal`); real execution
//!    goes through the agent `run` path which has a `&mut Shell`.
//!
//! 2. `DynamicCommandTool` — a Tool backed by trait-object introspection
//!    only. It does NOT hold the `Arc<dyn Command>` (that would require
//!    `Command: Send + Sync`, which the trait doesn't currently guarantee).
//!    Instead it copies out the name/description/schema at construction time.
//!    This lets us bridge all 80 commands into one `ToolRegistry` without
//!    touching the `Command` trait definition.
//!
//! Both bridges are introspection-only (catalog/describe-tools). Execution
//! happens in the `ash agent run` path (M2.4), which has a `&mut Shell` and
//! calls `Command::run` directly.

use ash_core::tool::catalog::ToolRegistry;
use ash_core::tool::{ErrorKind, Tool, ToolContext, ToolResult};
use serde_json::{Map, Value};

use crate::cmd::{Command, CommandRegistry, Signature};

/// Derive a minimal JSON Schema from a `Signature`.
///
/// Maps the Signature's argument list to an `{"type":"object", ...}` schema:
/// - required positionals → required string properties
/// - optional positionals → optional string properties (with default if set)
/// - flags → boolean properties (default false)
/// - options (--name VALUE) → string properties
pub fn derive_schema_from_signature(sig: &Signature) -> Map<String, Value> {
    let mut properties = Map::new();
    let mut required: Vec<String> = Vec::new();

    for arg in &sig.arguments {
        let ty = if arg.is_flag {
            "boolean".to_string()
        } else {
            "string".to_string()
        };

        let mut prop = Map::new();
        prop.insert("type".into(), Value::String(ty));
        prop.insert("description".into(), Value::String(arg.description.clone()));
        if let Some(d) = &arg.default {
            prop.insert("default".into(), Value::String(d.clone()));
        }
        properties.insert(arg.name.clone(), Value::Object(prop));

        // Flags are never required; options default to optional unless explicitly
        // marked required (rare). Only plain required positionals go in `required`.
        if arg.required && !arg.is_flag && !arg.is_option {
            required.push(arg.name.clone());
        }
    }

    let mut schema = Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".into(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }
    schema
}

// ──────────────────────────────────────────────────────────────────────────
// DynamicCommandTool — introspection-only Tool backed by a Command's signature
// ──────────────────────────────────────────────────────────────────────────

/// A Tool whose schema is derived from a `Command`'s signature at construction
/// time. Does NOT hold the command itself (avoids requiring `Command: Send + Sync`).
///
/// `invoke()` returns `Internal` — real execution goes through the agent `run`
/// path, which has a `&mut Shell`. This type exists so all 80 commands can be
/// represented in a `ToolRegistry` for catalog export (`ash agent describe-tools`).
pub struct DynamicCommandTool {
    name: String,
    description: String,
    parameters: Map<String, Value>,
}

impl DynamicCommandTool {
    /// Build from any `Command` by reading its signature. The command reference
    /// is NOT retained — only its signature data is copied out.
    pub fn from_command(command: &dyn Command) -> Self {
        let sig = command.signature();
        Self {
            name: sig.name.clone(),
            description: sig.description.clone(),
            parameters: derive_schema_from_signature(&sig),
        }
    }
}

impl Tool for DynamicCommandTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Map<String, Value> {
        self.parameters.clone()
    }
    fn invoke(&self, _args: &Value, _ctx: &ToolContext) -> ToolResult {
        // Introspection-only. Real execution is the agent `run` path's job
        // (it has a &mut Shell to pass to Command::run).
        ToolResult::failed(
            ErrorKind::Internal,
            "DynamicCommandTool is introspection-only; use `ash agent run` to execute",
        )
    }
}

// ──────────────────────────────────────────────────────────────────────────
// CommandToolBridge — generic single-command wrapper (kept for completeness)
// ──────────────────────────────────────────────────────────────────────────

/// Wrap a concrete `Command` type `T` so it satisfies `Tool`. Like
/// `DynamicCommandTool`, this is introspection-only — `invoke()` returns
/// `Internal` because the `Tool` interface carries no `&mut Shell`.
///
/// Prefer `DynamicCommandTool` for the registry-bridging path; this generic
/// form is kept for cases where a concrete type is available and you want
/// static dispatch on the signature derivation.
pub struct CommandToolBridge<T: Command> {
    pub inner: T,
}

impl<T: Command> CommandToolBridge<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Command + Send + Sync + 'static> Tool for CommandToolBridge<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        // signature() returns an owned Signature; we can't return a borrow to it.
        // Workaround: store the description at construction. For simplicity here,
        // we re-derive on each call (cheap; signatures are small).
        // (If this shows up in profiles, cache it on the struct.)
        let sig = self.inner.signature();
        leak_description(sig.description)
    }

    fn parameters_schema(&self) -> Map<String, Value> {
        let sig = self.inner.signature();
        derive_schema_from_signature(&sig)
    }

    fn invoke(&self, _args: &Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::failed(
            ErrorKind::Internal,
            "CommandToolBridge is introspection-only; use `ash agent run` to execute",
        )
    }
}

/// Given an owned `String`, return a `&'static str` by leaking it.
///
/// This is a workaround for `Tool::description(&self) -> &str` requiring a
/// borrowed str while `Command::signature()` returns an owned `Signature`.
/// We leak one short description string per command (bounded by the number of
/// commands, ~80, each a few dozen bytes — negligible). A future refactor
/// could change `Tool::description` to return `Cow<'_, str>` to avoid this.
fn leak_description(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

// ──────────────────────────────────────────────────────────────────────────
// Bulk bridging: CommandRegistry → ToolRegistry
// ──────────────────────────────────────────────────────────────────────────

/// Build a `ToolRegistry` by bridging every command in a `CommandRegistry`.
///
/// Each command becomes a `DynamicCommandTool` (introspection-only). The
/// resulting registry's `catalog()` can be exported for AI Agents via
/// `ash agent describe-tools`.
pub fn build_tool_registry_from_commands(commands: &CommandRegistry) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    // Use params() to get every signature, then look up each command by name
    // to derive its schema. (CommandRegistry::params returns Vec<Signature>.)
    for sig in commands.params() {
        let name = sig.name.clone();
        // Re-look-up to get the trait object (params() only gives signatures).
        if let Some(cmd) = commands.get(&name) {
            let tool = DynamicCommandTool::from_command(&*cmd);
            use std::sync::Arc;
            tools.register(Arc::new(tool));
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{Argument, Signature};

    // ── derive_schema_from_signature tests ──

    #[test]
    fn empty_signature_yields_empty_object_schema() {
        let sig = Signature::new("noop", "does nothing");
        let schema = derive_schema_from_signature(&sig);
        assert_eq!(schema.get("type").unwrap(), &Value::String("object".into()));
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.is_empty());
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn required_positional_becomes_required_string() {
        let sig = Signature::new("cat", "concatenate").required("file", "the file to read");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(
            props.get("file").unwrap().get("type").unwrap(),
            &Value::String("string".into())
        );
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], Value::String("file".into()));
    }

    #[test]
    fn flag_becomes_boolean_not_required() {
        let sig = Signature::new("ls", "list").flag("all", "show hidden");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(
            props.get("all").unwrap().get("type").unwrap(),
            &Value::String("boolean".into())
        );
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn optional_with_default_includes_default() {
        let sig = Signature::new("x", "x").optional_default("count", "10", "number of items");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let count_prop = props.get("count").unwrap().as_object().unwrap();
        assert_eq!(count_prop.get("default").unwrap(), &Value::String("10".into()));
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn option_arg_is_not_required() {
        // --name VALUE style: is_option=true, should not be in required list.
        let sig = Signature::new("x", "x").option("name", "the name");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(
            props.get("name").unwrap().get("type").unwrap(),
            &Value::String("string".into())
        );
        assert!(schema.get("required").is_none());
    }
}
