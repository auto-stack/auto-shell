//! Plan 028: Best-effort conversion from `auto_val::Value` to `serde_json::Value`.
//!
//! `auto_val::Value` is ash's own rich value type (designed for the AutoLang
//! scripting language — it carries Lambdas, Closures, Widgets, etc.). The
//! Agent-facing envelope needs plain JSON, so we convert the common cases and
//! degrade language-specific variants to descriptive strings.
//!
//! This is a one-way, lossy conversion. It does NOT touch the `auto_val::Value`
//! type itself.

use auto_val::Value as AutoValue;
use serde_json::{Map, Value as JsonValue};

/// Convert an `auto_val::Value` to a `serde_json::Value`.
///
/// Mapping:
/// - Numeric variants (Int/Uint/I64/Float/Double/Byte/etc.) → JSON number
/// - Bool → JSON boolean
/// - Str/String/Char → JSON string
/// - Array → JSON array (recursively)
/// - Obj → JSON object (recursively; keys stringified via ValueKey Display)
/// - Nil/Null/None/Void → JSON null
/// - Some(x) → unwrap to convert(x)
/// - Ok(x) → unwrap to convert(x)
/// - Everything else (Lambda, Closure, Widget, Model, Fn, ...) → descriptive
///   string like `"<auto_val:Lambda>"`
pub fn auto_value_to_json(v: &AutoValue) -> JsonValue {
    use auto_val::Value::*;
    match v {
        // ── nulls / empties ──
        Nil | Null | None | Void => JsonValue::Null,

        // ── unwrappers ──
        Some(inner) | Ok(inner) => auto_value_to_json(inner),

        // ── numbers ──
        Byte(b) => JsonValue::Number((*b).into()),
        Int(i) => JsonValue::Number((*i).into()),
        Uint(u) => JsonValue::Number((*u).into()),
        USize(u) => JsonValue::Number((*u).into()),
        I8(i) => JsonValue::Number((*i).into()),
        U8(u) => JsonValue::Number((*u).into()),
        I64(i) => serde_json::Number::from(*i).into(),
        // u32 has as_float fallback for very large; but we keep it integer.
        // Float/Double → JSON number (via f64; serde_json rejects NaN/Inf).
        Float(f) => serde_num(*f),
        Double(f) => serde_num(*f),

        // ── bool / char ──
        Bool(b) => JsonValue::Bool(*b),
        Char(c) => JsonValue::String(c.to_string()),

        // ── strings ──
        Str(s) => JsonValue::String(s.to_string()),
        String(s) => JsonValue::String(s.to_string()),
        StrSlice(s) => JsonValue::String(s.to_string()),
        CStr(s) => JsonValue::String(s.to_string()),

        // ── aggregates ──
        Array(arr) | Block(arr) => {
            JsonValue::Array(arr.iter().map(auto_value_to_json).collect())
        }
        Obj(obj) => {
            let mut m = Map::new();
            for (k, v) in obj.iter() {
                let key = k.to_string();
                m.insert(key, auto_value_to_json(v));
            }
            JsonValue::Object(m)
        }
        Pair(k, v) => {
            // A single pair degrades to {"key": value} with a stringified key.
            let mut m = Map::new();
            m.insert(k.to_string(), auto_value_to_json(v));
            JsonValue::Object(m)
        }

        // ── ranges ──
        Range(a, b) | RangeEq(a, b) => {
            serde_json::json!({ "start": a, "end": b })
        }

        // ── error ──
        Error(msg) => serde_json::json!({ "error": msg.to_string() }),
        Err(msg) => serde_json::json!({ "error": msg.to_string() }),

        // ── async: not yet resolved ──
        Future(_) => serde_json::json!({ "pending": "future not resolved" }),

        // ── language-level variants: degrade to a descriptive string ──
        // (These have no JSON representation; agents shouldn't see them often.)
        other => {
            let ty = discriminant_name(other);
            JsonValue::String(format!("<auto_val:{}>", ty))
        }
    }
}

/// Map NaN/Inf to Null (serde_json::Number can't hold them); otherwise
/// construct a JSON number from an f64.
fn serde_num(f: f64) -> JsonValue {
    if f.is_finite() {
        serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null)
    } else {
        JsonValue::Null
    }
}

/// Best-effort type-name for an auto_val::Value variant (for the descriptive
/// string fallback). Uses the Debug representation's leading token.
fn discriminant_name(v: &AutoValue) -> &'static str {
    use auto_val::Value::*;
    match v {
        Byte(_) | Int(_) | Uint(_) | USize(_) | I8(_) | U8(_) | I64(_) | Float(_)
        | Double(_) => "Number",
        Bool(_) => "Bool",
        Char(_) => "Char",
        Str(_) | String(_) | StrSlice(_) | CStr(_) => "String",
        Array(_) | Block(_) => "Array",
        Obj(_) => "Obj",
        Pair(_, _) => "Pair",
        Node(_) => "Node",
        Range(_, _) | RangeEq(_, _) => "Range",
        Fn(_) => "Fn",
        ExtFn(_) => "ExtFn",
        Type(_) => "Type",
        Nil => "Nil",
        Null => "Null",
        Lambda(_) => "Lambda",
        Void => "Void",
        Widget(_) => "Widget",
        Model(_) => "Model",
        View(_) => "View",
        Meta(_) => "Meta",
        Method(_) => "Method",
        Instance(_) => "Instance",
        Args(_) => "Args",
        Ref(_) => "Ref",
        Error(_) => "Error",
        Grid(_) => "Grid",
        VmRef(_) => "VmRef",
        ValueRef(_) => "ValueRef",
        Closure(_) => "Closure",
        Some(_) => "Some",
        None => "None",
        Ok(_) => "Ok",
        Err(_) => "Err",
        Future(_) => "Future",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auto_val::Value;

    #[test]
    fn converts_int() {
        let v = auto_value_to_json(&Value::Int(42));
        assert_eq!(v, JsonValue::Number(42.into()));
    }

    #[test]
    fn converts_bool() {
        assert_eq!(
            auto_value_to_json(&Value::Bool(true)),
            JsonValue::Bool(true)
        );
    }

    #[test]
    fn converts_string() {
        let v = auto_value_to_json(&Value::Str("hello".into()));
        assert_eq!(v, JsonValue::String("hello".into()));
    }

    #[test]
    fn converts_nil_to_null() {
        assert_eq!(auto_value_to_json(&Value::Nil), JsonValue::Null);
        assert_eq!(auto_value_to_json(&Value::Null), JsonValue::Null);
        assert_eq!(auto_value_to_json(&Value::Void), JsonValue::Null);
    }

    #[test]
    fn converts_some_by_unwrapping() {
        let v = auto_value_to_json(&Value::Some(Box::new(Value::Int(7))));
        assert_eq!(v, JsonValue::Number(7.into()));
    }

    #[test]
    fn converts_nan_to_null() {
        let v = auto_value_to_json(&Value::Float(f64::NAN));
        assert_eq!(v, JsonValue::Null);
    }

    #[test]
    fn converts_language_variant_to_descriptive_string() {
        let v = auto_value_to_json(&Value::Lambda("x".into()));
        // Should degrade, not panic.
        assert!(matches!(v, JsonValue::String(_)));
        if let JsonValue::String(s) = v {
            assert!(s.contains("auto_val"));
        }
    }
}
