//! JSON plumbing for the file layer: a field that remembers whether it was absent, and
//! the comparison and number formatting that make "read then write" lossless.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};

/// A field that may be absent, `null`, or hold a value. Excalidraw added fields over the
/// years (`index`, `created`, ...), so an older file lacks keys a newer one writes as
/// `null`; writing either back as the other would change the file.
///
/// Use with `#[serde(default, skip_serializing_if = "Slot::is_missing")]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Slot<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T> Slot<T> {
    pub fn is_missing(&self) -> bool {
        matches!(self, Slot::Missing)
    }

    /// The value, treating absent and `null` alike (JS `?.` / `??`).
    pub fn value(&self) -> Option<&T> {
        match self {
            Slot::Value(v) => Some(v),
            _ => None,
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Slot<T> {
    /// Only called when the key is present: `null` becomes `Null`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            None => Slot::Null,
            Some(v) => Slot::Value(v),
        })
    }
}

impl<T: Serialize> Serialize for Slot<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Value(v) => v.serialize(serializer),
            Slot::Missing | Slot::Null => serializer.serialize_none(),
        }
    }
}

/// Equality as a JS reader sees two JSON documents: key order is irrelevant and numbers
/// compare as f64 (`1` and `1.0` are equal). Spec §9.2's round-trip criterion.
pub fn semantic_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| semantic_eq(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| semantic_eq(v, w)))
        }
        _ => a == b,
    }
}

/// Largest integer JS numbers hold exactly.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Rewrites integral floats as JSON integers, so a saved file reads like
/// `JSON.stringify` output (`"version": 2`, not `2.0`). Values are unchanged.
pub fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|_| n.is_f64())
                && f.fract() == 0.0
                && f.abs() <= MAX_SAFE_INTEGER
            {
                *n = Number::from(f as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_numbers),
        Value::Object(map) => map.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn semantic_eq_ignores_key_order_and_number_spelling() {
        assert!(semantic_eq(
            &json!({"a": 1, "b": [2.0]}),
            &json!({"b": [2], "a": 1.0})
        ));
        assert!(!semantic_eq(&json!({"a": 1}), &json!({"a": 1, "b": null})));
        assert!(!semantic_eq(&json!([1, 2]), &json!([2, 1])));
    }

    #[test]
    fn normalize_numbers_writes_integers_like_javascript() {
        let mut v = json!({"version": 2.0, "x": -0.0, "y": 0.5, "big": 1e300});
        normalize_numbers(&mut v);
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"version":2,"x":0,"y":0.5,"big":1e+300}"#
        );
    }
}
