//! Plan 028: JSON Schema types and signature-to-schema derivation.

use serde_json::{Map, Value};

/// A tool's self-description, as exported by `ToolRegistry::catalog()`.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: Map<String, Value>,
    pub output: Option<Map<String, Value>>,
    pub capabilities_json: Value,
}

impl ToolDescriptor {
    /// Serialize to the MCP-compatible `tools/list` item shape.
    pub fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("name".into(), Value::String(self.name.clone()));
        obj.insert(
            "description".into(),
            Value::String(self.description.clone()),
        );
        obj.insert("inputSchema".into(), Value::Object(self.parameters.clone()));
        if let Some(out) = &self.output {
            obj.insert("outputSchema".into(), Value::Object(out.clone()));
        }
        Value::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_to_json_has_mcp_shape() {
        let mut params = Map::new();
        params.insert("type".into(), Value::String("object".into()));
        let d = ToolDescriptor {
            name: "ls".into(),
            description: "list files".into(),
            parameters: params,
            output: None,
            capabilities_json: Value::Null,
        };
        let j = d.to_json();
        let obj = j.as_object().unwrap();
        assert_eq!(obj.get("name").unwrap(), &Value::String("ls".into()));
        assert_eq!(
            obj.get("description").unwrap(),
            &Value::String("list files".into())
        );
        assert!(obj.get("inputSchema").is_some());
        assert!(obj.get("outputSchema").is_none()); // None omitted
    }
}
