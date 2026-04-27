//! Format conversion helpers for storage operations.
//!
//! This module provides pure format-conversion functions (JSON ↔ TOML) that are
//! shared across multiple storage types. All functions are free of any IO logic.

use serde_json::Value as JsonValue;
use thiserror::Error;

/// Error produced by format-conversion operations.
///
/// Each variant carries a human-readable message describing the failed step.
#[derive(Debug, Error)]
pub enum FormatConvertError {
    /// Failed to serialize a JSON value to an intermediate string.
    #[error("json→toml serialize: {0}")]
    Serialize(String),

    /// Failed to deserialize an intermediate string into the target TOML value.
    #[error("json→toml deserialize: {0}")]
    Deserialize(String),

    /// Failed to parse a TOML string into a `toml::Value`.
    ///
    /// Reserved for conversion paths that use `toml::from_str` directly.
    #[error("toml parse: {0}")]
    TomlParse(String),
}

/// Convert a `serde_json::Value` to a `toml::Value`.
///
/// Uses a two-step round-trip through JSON string representation:
/// `JsonValue` → JSON string → `toml::Value` via `serde_json::from_str`.
///
/// # Arguments
///
/// * `json_value` - A reference to the JSON value to convert.
///
/// # Returns
///
/// Returns `Ok(toml::Value)` on success, or a `FormatConvertError` describing
/// which step of the conversion failed.
///
/// # Errors
///
/// - `FormatConvertError::Serialize` — when `serde_json::to_string` fails.
/// - `FormatConvertError::Deserialize` — when `serde_json::from_str::<toml::Value>` fails.
pub fn json_to_toml(json_value: &JsonValue) -> Result<toml::Value, FormatConvertError> {
    let json_str = serde_json::to_string(json_value)
        .map_err(|e| FormatConvertError::Serialize(e.to_string()))?;
    let toml_value: toml::Value = serde_json::from_str(&json_str)
        .map_err(|e| FormatConvertError::Deserialize(e.to_string()))?;
    Ok(toml_value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // T1: happy path
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_to_toml_simple_object() {
        let json = json!({"key": "value", "count": 42});
        let toml_val = json_to_toml(&json).expect("conversion must succeed");
        assert_eq!(toml_val["key"].as_str(), Some("value"));
        assert_eq!(toml_val["count"].as_integer(), Some(42));
    }

    #[test]
    fn test_json_to_toml_empty_object() {
        let json = json!({});
        // T2: boundary — empty object
        let result = json_to_toml(&json);
        assert!(result.is_ok(), "empty object must convert successfully");
        let toml_val = result.unwrap();
        // An empty TOML table
        assert!(toml_val.as_table().map(|t| t.is_empty()).unwrap_or(false));
    }

    #[test]
    fn test_json_to_toml_nested_object() {
        let json = json!({"outer": {"inner": true}});
        let toml_val = json_to_toml(&json).expect("nested object must convert");
        let outer = toml_val.get("outer").expect("outer key must exist");
        assert_eq!(outer["inner"].as_bool(), Some(true));
    }

    // -----------------------------------------------------------------------
    // T2: boundary / edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_json_to_toml_null_value_is_boundary() {
        // JSON null has no direct TOML equivalent; serde may reject it.
        // We document the behaviour but do not assert success or failure,
        // because the conversion outcome is implementation-defined.
        let json = json!(null);
        let _ = json_to_toml(&json); // just must not panic
    }

    #[test]
    fn test_json_to_toml_string_with_unicode() {
        let json = json!({"emoji": "🦀", "text": "日本語"});
        let toml_val = json_to_toml(&json).expect("unicode must convert");
        assert_eq!(toml_val["emoji"].as_str(), Some("🦀"));
        assert_eq!(toml_val["text"].as_str(), Some("日本語"));
    }

    // -----------------------------------------------------------------------
    // T3: error path — FormatConvertError variants
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_convert_error_serialize_display() {
        let err = FormatConvertError::Serialize("bad input".to_string());
        assert!(err.to_string().contains("serialize"));
        assert!(err.to_string().contains("bad input"));
    }

    #[test]
    fn test_format_convert_error_deserialize_display() {
        let err = FormatConvertError::Deserialize("unexpected char".to_string());
        assert!(err.to_string().contains("deserialize"));
        assert!(err.to_string().contains("unexpected char"));
    }

    #[test]
    fn test_format_convert_error_toml_parse_display() {
        let err = FormatConvertError::TomlParse("invalid toml".to_string());
        assert!(err.to_string().contains("toml parse"));
        assert!(err.to_string().contains("invalid toml"));
    }

    #[test]
    fn test_format_convert_error_is_std_error() {
        let err = FormatConvertError::Serialize("x".to_string());
        let _: &dyn std::error::Error = &err;
    }
}
