//! Plan 031 M0.2 — unified `Format` trait for the 10 format converters.
//!
//! The 10 `from_*`/`to_*` commands (`from_json`, `to_json`, `from_csv`, ...)
//! share the same shape: parse text into a [`Value`], or serialize a
//! [`Value`] to text. Previously each command inlined that logic (and most
//! routed through the lossy `atom_to_pipeline_data` bridge that silently
//! swallowed read errors). This module defines a single [`Format`] trait plus
//! a [`FormatRegistry`] so the conversion logic is reusable (e.g. by the lazy
//! pipeline's Source/serialize ends) and uniformly error-checked.
//!
//! The trait lives in `auto-shell` (not `ash-core` as the design doc §5.1
//! suggested) because the implementations reuse the existing hand-written
//! parsers here — `toml` needs the `toml` crate, and moving ~1900 lines of
//! parsers into `ash-core` is out of scope for M0. The trait is still a pure
//! interface over `auto_val::Value`; `ash-core`'s lazy layer never needs it
//! (Format sits at the *ends* of a lazy chain, per design §5.5).

use std::sync::Arc;

use auto_val::Value;

use crate::cmd::commands::{
    from_csv::parse_csv, from_json::parse_json, from_toml::parse_toml, from_xml::parse_xml,
    from_yaml::parse_yaml, to_csv::value_to_csv, to_json::value_to_json, to_toml::value_to_toml,
    to_xml::value_to_xml, to_yaml::value_to_yaml,
};

/// Error returned by a [`Format`] parse operation.
#[derive(Debug, Clone)]
pub struct FormatError(pub String);

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "format error: {}", self.0)
    }
}

impl std::error::Error for FormatError {}

/// Unified text ⇄ [`Value`] conversion for a structured format.
///
/// `serialize` uses each format's conventional defaults (pretty indent 2,
/// CSV delimiter `,` with header, XML root element `root`). Commands that
/// expose user flags (e.g. `--indent`, `--delimiter`) keep their own
/// parameterized paths; this trait is the common denominator.
pub trait Format: Send + Sync {
    /// Canonical name, e.g. `"json"` / `"csv"` / `"yaml"` / `"xml"` / `"toml"`.
    fn name(&self) -> &str;
    /// Parse text into a structured [`Value`].
    fn parse(&self, text: &str) -> Result<Value, FormatError>;
    /// Serialize a [`Value`] to text using the format's default settings.
    fn serialize(&self, value: &Value) -> String;
}

// ── JSON ───────────────────────────────────────────────────────────────────

/// JSON format (object/array/scalars), pretty-printed with 2-space indent.
pub struct JsonFormat;

impl Format for JsonFormat {
    fn name(&self) -> &str {
        "json"
    }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        parse_json(text).map_err(|e| FormatError(e.to_string()))
    }
    fn serialize(&self, value: &Value) -> String {
        value_to_json(value, 2, 0)
    }
}

// ── CSV ────────────────────────────────────────────────────────────────────

/// CSV format: parses into a table [`Value::Array`] of row objects.
/// Delimiter `,`, first row treated as header.
pub struct CsvFormat;

impl Format for CsvFormat {
    fn name(&self) -> &str {
        "csv"
    }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        let table = parse_csv(text, ",", true).map_err(|e| FormatError(e.to_string()))?;
        Ok(Value::Array(table))
    }
    fn serialize(&self, value: &Value) -> String {
        // CSV serialize is fallible (non-tabular input); default-config errors
        // surface as an empty string here. Callers needing error handling use
        // `value_to_csv` directly.
        value_to_csv(value, ",", true).unwrap_or_default()
    }
}

// ── YAML ───────────────────────────────────────────────────────────────────

/// YAML format, top-level indent 0.
pub struct YamlFormat;

impl Format for YamlFormat {
    fn name(&self) -> &str {
        "yaml"
    }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        parse_yaml(text).map_err(|e| FormatError(e.to_string()))
    }
    fn serialize(&self, value: &Value) -> String {
        value_to_yaml(value, 0)
    }
}

// ── XML ────────────────────────────────────────────────────────────────────

/// XML format, root element `root`, 2-space indent.
pub struct XmlFormat;

impl Format for XmlFormat {
    fn name(&self) -> &str {
        "xml"
    }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        parse_xml(text).map_err(|e| FormatError(e.to_string()))
    }
    fn serialize(&self, value: &Value) -> String {
        value_to_xml(value, "root", 2, 0)
    }
}

// ── TOML ───────────────────────────────────────────────────────────────────

/// TOML format, top-level (no section path), depth 0.
pub struct TomlFormat;

impl Format for TomlFormat {
    fn name(&self) -> &str {
        "toml"
    }
    fn parse(&self, text: &str) -> Result<Value, FormatError> {
        parse_toml(text).map_err(|e| FormatError(e.to_string()))
    }
    fn serialize(&self, value: &Value) -> String {
        value_to_toml(value, &[], 0)
    }
}

// ── Registry ───────────────────────────────────────────────────────────────

/// Registry of available [`Format`]s keyed by canonical name.
pub struct FormatRegistry {
    formats: std::collections::HashMap<String, Arc<dyn Format>>,
}

impl FormatRegistry {
    /// Build a registry preloaded with the five built-in formats.
    pub fn new() -> Self {
        let mut r = Self {
            formats: std::collections::HashMap::new(),
        };
        r.register(Arc::new(JsonFormat));
        r.register(Arc::new(CsvFormat));
        r.register(Arc::new(YamlFormat));
        r.register(Arc::new(XmlFormat));
        r.register(Arc::new(TomlFormat));
        r
    }

    /// Register an additional format under its `name()`.
    pub fn register(&mut self, format: Arc<dyn Format>) {
        self.formats.insert(format.name().to_string(), format);
    }

    /// Look up a format by canonical name (e.g. `"json"`).
    pub fn get(&self, name: &str) -> Option<Arc<dyn Format>> {
        self.formats.get(name).cloned()
    }

    /// Iterate over the registered format names.
    pub fn names(&self) -> Vec<String> {
        self.formats.keys().cloned().collect()
    }
}

impl Default for FormatRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auto_val::{Obj, Value};

    fn sample_obj() -> Value {
        // {"name": "alice", "age": 30}
        let mut o = Obj::new();
        o.set("name", Value::str("alice"));
        o.set("age", Value::Int(30));
        Value::Obj(o)
    }

    // ── JSON ──

    #[test]
    fn json_roundtrip_preserves_object() {
        let fmt = JsonFormat;
        let original = sample_obj();
        let text = fmt.serialize(&original);
        let parsed = fmt.parse(&text).expect("json parse");
        // Re-serialize must be stable (canonical form matches).
        assert_eq!(fmt.serialize(&parsed), text);
        // Field values survive the round trip.
        match parsed {
            Value::Obj(o) => {
                assert_eq!(o.get("name").unwrap().as_str(), "alice");
                assert_eq!(o.get("age").unwrap().to_string(), "30");
            }
            other => panic!("expected Obj, got {other:?}"),
        }
    }

    #[test]
    fn json_parse_error_is_reported() {
        let fmt = JsonFormat;
        let err = fmt.parse("{ not valid json").unwrap_err();
        assert!(!err.0.is_empty());
    }

    #[test]
    fn json_name() {
        assert_eq!(JsonFormat.name(), "json");
    }

    // ── TOML ──

    #[test]
    fn toml_roundtrip_preserves_object() {
        let fmt = TomlFormat;
        let original = sample_obj();
        let text = fmt.serialize(&original);
        let parsed = fmt.parse(&text).expect("toml parse");
        assert_eq!(fmt.serialize(&parsed), text);
    }

    #[test]
    fn toml_name() {
        assert_eq!(TomlFormat.name(), "toml");
    }

    // ── YAML ──

    #[test]
    fn yaml_roundtrip_preserves_object() {
        let fmt = YamlFormat;
        let original = sample_obj();
        let text = fmt.serialize(&original);
        let parsed = fmt.parse(&text).expect("yaml parse");
        assert_eq!(fmt.serialize(&parsed), text);
    }

    // ── XML ──

    #[test]
    fn xml_roundtrip_preserves_object() {
        let fmt = XmlFormat;
        let original = sample_obj();
        let text = fmt.serialize(&original);
        let parsed = fmt.parse(&text).expect("xml parse");
        assert_eq!(fmt.serialize(&parsed), text);
    }

    // ── CSV ──

    #[test]
    fn csv_roundtrip_preserves_table() {
        let fmt = CsvFormat;
        // Two row objects with the same keys.
        let mut a = Obj::new();
        a.set("name", Value::str("alice"));
        a.set("age", Value::Int(30));
        let mut b = Obj::new();
        b.set("name", Value::str("bob"));
        b.set("age", Value::Int(25));
        let table = Value::Array(
            [Value::Obj(a), Value::Obj(b)]
                .into_iter()
                .collect::<Vec<_>>()
                .into(),
        );
        let text = fmt.serialize(&table);
        let parsed = fmt.parse(&text).expect("csv parse");
        assert_eq!(fmt.serialize(&parsed), text);
    }

    // ── Registry ──

    #[test]
    fn registry_contains_five_builtin_formats() {
        let r = FormatRegistry::new();
        for name in ["json", "csv", "yaml", "xml", "toml"] {
            assert!(r.get(name).is_some(), "missing format {name:?}");
        }
    }

    #[test]
    fn registry_unknown_returns_none() {
        let r = FormatRegistry::new();
        assert!(r.get("nope").is_none());
    }

    #[test]
    fn registry_lookup_roundtrips_json() {
        let r = FormatRegistry::new();
        let fmt = r.get("json").expect("json registered");
        let text = fmt.serialize(&sample_obj());
        let parsed = fmt.parse(&text).expect("parse via registry");
        assert_eq!(fmt.serialize(&parsed), text);
    }
}
