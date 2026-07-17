//! Plan 028: Interface reservation for the built-in F4 agent loop.
//!
//! This module defines ONLY traits — no implementation. The actual agent
//! loop (LLM polling → tool call → execute → refill) is Plan 029+.
//! By defining the contract here, Plan 029 can consume the ToolRegistry
//! without restructuring.

use serde_json::Value;

use crate::tool::{ToolContext, ToolResult};

/// The LLM provider whose tool-call format we're serializing for.
/// Different providers have subtly different tool-spec shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// Anthropic tool_use format.
    Anthropic,
    /// OpenAI function-calling format.
    OpenAI,
    /// MCP tools/list format (also our CLI catalog format).
    Mcp,
    /// Generic JSON Schema (no provider-specific wrapping).
    Generic,
}

/// Contract between a future F4 agent loop and the ToolRegistry.
/// Implemented by `ToolRegistry` (added in a later task).
pub trait ToolExecuting {
    /// Execute one tool call: validate args, check policy, run, return result.
    fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: &Value,
        ctx: &ToolContext,
    ) -> ToolResult;

    /// Export the full catalog in a provider-specific tool-spec format.
    fn export_for_provider(&self, provider: LlmProvider) -> Vec<Value>;
}
