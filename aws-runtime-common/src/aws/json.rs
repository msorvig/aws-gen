//! Traits for JSON serialization/deserialization without serde derives.
//! Uses serde_json::Value as the interchange format.

use serde_json::Value;

/// Parse a typed value from a JSON Value.
pub trait FromJsonValue: Sized {
    fn from_json(v: &Value) -> Self;
}

/// Serialize a typed value to a JSON Value.
pub trait ToJsonValue {
    fn to_json(&self) -> Value;
}

// ── Primitive FromJsonValue impls ──────────────────────────────────────

impl FromJsonValue for String {
    fn from_json(v: &Value) -> Self { v.as_str().unwrap_or("").to_string() }
}

impl FromJsonValue for i32 {
    fn from_json(v: &Value) -> Self { v.as_i64().unwrap_or(0) as i32 }
}

impl FromJsonValue for i64 {
    fn from_json(v: &Value) -> Self { v.as_i64().unwrap_or(0) }
}

impl FromJsonValue for f32 {
    fn from_json(v: &Value) -> Self { v.as_f64().unwrap_or(0.0) as f32 }
}

impl FromJsonValue for f64 {
    fn from_json(v: &Value) -> Self { v.as_f64().unwrap_or(0.0) }
}

impl FromJsonValue for bool {
    fn from_json(v: &Value) -> Self { v.as_bool().unwrap_or(false) }
}

// ── Primitive ToJsonValue impls ────────────────────────────────────────

impl ToJsonValue for String {
    fn to_json(&self) -> Value { Value::String(self.clone()) }
}

impl ToJsonValue for i32 {
    fn to_json(&self) -> Value { Value::Number((*self as i64).into()) }
}

impl ToJsonValue for i64 {
    fn to_json(&self) -> Value { Value::Number((*self).into()) }
}

impl ToJsonValue for f64 {
    fn to_json(&self) -> Value {
        serde_json::Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

impl ToJsonValue for bool {
    fn to_json(&self) -> Value { Value::Bool(*self) }
}

// ── Blob (Vec<u8>) impls ──────────────────────────────────────────────

impl FromJsonValue for Vec<u8> {
    fn from_json(v: &Value) -> Self {
        use crate::base64;
        match v.as_str() {
            Some(s) => base64::decode(s),
            None => Vec::new(),
        }
    }
}

impl ToJsonValue for Vec<u8> {
    fn to_json(&self) -> Value {
        use crate::base64;
        Value::String(base64::encode(self))
    }
}

// ── Option impls ───────────────────────────────────────────────────────

impl<T: FromJsonValue> FromJsonValue for Option<T> {
    fn from_json(v: &Value) -> Self {
        if v.is_null() { None } else { Some(T::from_json(v)) }
    }
}

impl<T: ToJsonValue> ToJsonValue for Option<T> {
    fn to_json(&self) -> Value {
        match self {
            Some(v) => v.to_json(),
            None => Value::Null,
        }
    }
}

// ── Unit impl (empty JSON object) ────────────────────────────��─────────

impl ToJsonValue for () {
    fn to_json(&self) -> Value { Value::Object(serde_json::Map::new()) }
}

// ── HashMap impls ──────────────────────────────────────────────────────

impl<V: FromJsonValue> FromJsonValue for std::collections::HashMap<String, V> {
    fn from_json(v: &Value) -> Self {
        v.as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), V::from_json(v))).collect())
            .unwrap_or_default()
    }
}

impl<V: ToJsonValue> ToJsonValue for std::collections::HashMap<String, V> {
    fn to_json(&self) -> Value {
        let m: serde_json::Map<String, Value> = self.iter()
            .map(|(k, v)| (k.clone(), v.to_json()))
            .collect();
        Value::Object(m)
    }
}
