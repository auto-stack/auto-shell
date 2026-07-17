//! Plan 028: ToolRegistry — the in-memory catalog of all Tools.

use std::collections::HashMap;
use std::sync::Arc;

use crate::tool::schema::ToolDescriptor;
use crate::tool::{Capabilities, Tool};

/// Registry of all Tools available to AI Agents.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// bash-compat aliases (e.g. "ll" -> "ls"). Resolved on lookup.
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Register a tool under its `name()`.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Register an alias that resolves to a registered tool's name.
    pub fn register_alias(&mut self, alias: impl Into<String>, target: impl Into<String>) {
        self.aliases.insert(alias.into(), target.into());
    }

    /// Look up a tool by name, resolving aliases.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some(t) = self.tools.get(name) {
            return Some(Arc::clone(t));
        }
        if let Some(target) = self.aliases.get(name) {
            return self.tools.get(target).cloned();
        }
        None
    }

    /// Export every tool's descriptor (full schemas). Order is unspecified.
    pub fn catalog(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
                output: t.output_schema(),
                capabilities_json: capabilities_to_json(&t.capabilities()),
            })
            .collect()
    }

    /// Export only tool names + descriptions (no parameter schemas), for
    /// context-budget-constrained Agents.
    pub fn catalog_compact(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: serde_json::Map::new(), // empty = compact
                output: None,
                capabilities_json: serde_json::Value::Null,
            })
            .collect()
    }

    /// Number of registered tools (excluding aliases).
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// All registered tool names (excluding aliases), sorted for stable output.
    pub fn names_sorted(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn capabilities_to_json(caps: &Capabilities) -> serde_json::Value {
    serde_json::json!({
        "reads_fs": caps.reads_fs,
        "writes_fs": caps.writes_fs,
        "spawns_process": caps.spawns_process,
        "uses_network": caps.uses_network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolContext, ToolResult};
    use serde_json::json;

    struct DummyTool {
        nm: &'static str,
        desc: &'static str,
    }
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            self.nm
        }
        fn description(&self) -> &str {
            self.desc
        }
        fn parameters_schema(&self) -> serde_json::Map<String, serde_json::Value> {
            let mut m = serde_json::Map::new();
            m.insert("type".into(), json!("object"));
            m
        }
        fn invoke(&self, _a: &serde_json::Value, _c: &ToolContext) -> ToolResult {
            ToolResult::success_json(json!({}))
        }
    }

    #[test]
    fn register_and_get_by_name() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        assert!(reg.get("ls").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn alias_resolves_to_target() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        reg.register_alias("ll", "ls");
        assert!(reg.get("ll").is_some());
        assert_eq!(reg.get("ll").unwrap().name(), "ls");
    }

    #[test]
    fn catalog_has_full_schema() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        let cat = reg.catalog();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].name, "ls");
        assert!(!cat[0].parameters.is_empty());
    }

    #[test]
    fn catalog_compact_omits_schema() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        let cat = reg.catalog_compact();
        assert!(cat[0].parameters.is_empty());
    }

    #[test]
    fn names_sorted_returns_sorted_list() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "zeta", desc: "" }));
        reg.register(Arc::new(DummyTool { nm: "alpha", desc: "" }));
        assert_eq!(reg.names_sorted(), vec!["alpha", "zeta"]);
    }
}
