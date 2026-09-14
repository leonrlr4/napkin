//! Reads the JSON baselines written by `tools/baseline` and compares Rust output with them.
//!
//! A baseline group is `{ "source": ..., "cases": [ { name, call, args, compare, expected } ] }`.
//! JSON cannot hold NaN or ±Infinity, so the generator writes them as the strings `"NaN"`,
//! `"Infinity"` and `"-Infinity"`; [`num`] and [`to_value`] translate both ways.

pub mod rough_json;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde_json::{Map, Value};

/// Absolute tolerance for every compared number (spec §9.1). V8 and glibc may disagree in
/// the last bit of `sin`/`cos`; everything else in the ports is exact arithmetic.
pub const TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// Every number must be within [`TOLERANCE`].
    Exact,
    /// The case used `Math.random`: numbers are ignored, everything else must match.
    Structure,
}

#[derive(Debug)]
pub struct Case {
    pub name: String,
    pub call: String,
    pub args: Vec<Value>,
    pub compare: Compare,
    pub expected: Value,
}

impl Case {
    /// Positional argument `i` as a number.
    pub fn num(&self, i: usize) -> f64 {
        num(&self.args[i])
    }
}

pub fn load_group(path: &Path) -> Vec<Case> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read baseline {}: {e}", path.display()));
    let root: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid baseline {}: {e}", path.display()));
    root["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing cases array", path.display()))
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().expect("case name").to_owned(),
            call: case["call"].as_str().expect("case call").to_owned(),
            args: case["args"].as_array().expect("case args").clone(),
            compare: match case["compare"].as_str() {
                Some("exact") => Compare::Exact,
                Some("structure") => Compare::Structure,
                other => panic!("unknown compare mode {other:?}"),
            },
            expected: case["expected"].clone(),
        })
        .collect()
}

/// Decodes a baseline number, including the three non-finite spellings.
pub fn num(value: &Value) -> f64 {
    as_number(value).unwrap_or_else(|| panic!("expected a number, got {value}"))
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => match s.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// Encodes a number the way the baseline generator does.
pub fn to_value(x: f64) -> Value {
    if x.is_nan() {
        Value::from("NaN")
    } else if x == f64::INFINITY {
        Value::from("Infinity")
    } else if x == f64::NEG_INFINITY {
        Value::from("-Infinity")
    } else {
        Value::from(x)
    }
}

pub fn point_value(p: [f64; 2]) -> Value {
    Value::Array(vec![to_value(p[0]), to_value(p[1])])
}

pub fn points_from(value: &Value) -> Vec<[f64; 2]> {
    value
        .as_array()
        .expect("array of points")
        .iter()
        .map(|p| [num(&p[0]), num(&p[1])])
        .collect()
}

fn numbers_match(expected: f64, actual: f64) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return expected.is_nan() && actual.is_nan();
    }
    if expected.is_infinite() || actual.is_infinite() {
        return expected == actual;
    }
    (expected - actual).abs() <= TOLERANCE
}

/// The first difference between `expected` and `actual`, as `"path: description"`.
pub fn diff(expected: &Value, actual: &Value, compare: Compare) -> Option<String> {
    diff_at("$", expected, actual, compare)
}

fn diff_at(path: &str, expected: &Value, actual: &Value, compare: Compare) -> Option<String> {
    if let (Some(e), Some(a)) = (as_number(expected), as_number(actual)) {
        return match compare {
            Compare::Structure => None,
            Compare::Exact if numbers_match(e, a) => None,
            Compare::Exact => Some(format!(
                "{path}: expected {e:?}, got {a:?} (delta {:e})",
                a - e
            )),
        };
    }
    match (expected, actual) {
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!(
                    "{path}: expected {} items, got {}",
                    e.len(),
                    a.len()
                ));
            }
            e.iter()
                .zip(a)
                .enumerate()
                .find_map(|(i, (e, a))| diff_at(&format!("{path}[{i}]"), e, a, compare))
        }
        (Value::Object(e), Value::Object(a)) => diff_objects(path, e, a, compare),
        _ if expected == actual => None,
        _ => Some(format!("{path}: expected {expected}, got {actual}")),
    }
}

fn diff_objects(
    path: &str,
    expected: &Map<String, Value>,
    actual: &Map<String, Value>,
    compare: Compare,
) -> Option<String> {
    if let Some(key) = expected.keys().find(|k| !actual.contains_key(*k)) {
        return Some(format!("{path}: missing key {key:?}"));
    }
    if let Some(key) = actual.keys().find(|k| !expected.contains_key(*k)) {
        return Some(format!("{path}: unexpected key {key:?}"));
    }
    expected
        .iter()
        .find_map(|(k, e)| diff_at(&format!("{path}.{k}"), e, &actual[k], compare))
}

/// Runs every case of `dir/<group>.json` through `run` and panics with a report of each
/// failing case. A panic inside `run` fails that case only.
///
/// Set `BASELINE_CASE=<substring>` to run only cases whose name contains it.
pub fn check_group(dir: &Path, group: &str, run: impl Fn(&Case) -> Value) {
    let cases = load_group(&dir.join(format!("{group}.json")));
    assert!(!cases.is_empty(), "{group}: baseline has no cases");
    let filter = std::env::var("BASELINE_CASE").ok();
    let mut ran = 0;
    let mut failures = Vec::new();
    for case in &cases {
        if filter.as_deref().is_some_and(|f| !case.name.contains(f)) {
            continue;
        }
        ran += 1;
        match catch_unwind(AssertUnwindSafe(|| run(case))) {
            Ok(actual) => {
                if let Some(d) = diff(&case.expected, &actual, case.compare) {
                    failures.push(format!("{}: {d}", case.name));
                }
            }
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("<non-string panic>");
                failures.push(format!("{}: panicked: {message}", case.name));
            }
        }
    }
    assert!(ran > 0, "{group}: BASELINE_CASE matched no case");
    if !failures.is_empty() {
        let shown: Vec<_> = failures.iter().take(20).map(|f| format!("  {f}")).collect();
        panic!(
            "{group}: {} of {ran} cases failed\n{}{}",
            failures.len(),
            shown.join("\n"),
            if failures.len() > 20 { "\n  ..." } else { "" }
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn exact_rejects_difference_above_tolerance() {
        let d = diff(&json!([1.0]), &json!([1.0 + 2e-9]), Compare::Exact);
        assert!(d.unwrap().starts_with("$[0]: expected 1.0"));
    }

    #[test]
    fn exact_accepts_difference_within_tolerance() {
        assert_eq!(
            diff(
                &json!({"a": [1.0]}),
                &json!({"a": [1.0 + 5e-10]}),
                Compare::Exact
            ),
            None
        );
    }

    #[test]
    fn nan_matches_only_nan() {
        assert_eq!(
            diff(&json!("NaN"), &to_value(f64::NAN), Compare::Exact),
            None
        );
        assert!(diff(&json!("NaN"), &json!(0.0), Compare::Exact).is_some());
    }

    #[test]
    fn structure_ignores_numbers_but_not_shape() {
        assert_eq!(
            diff(&json!([1, 2]), &json!([5, 6]), Compare::Structure),
            None
        );
        assert!(diff(&json!([1, 2]), &json!([5]), Compare::Structure).is_some());
        assert!(
            diff(
                &json!({"op": "move"}),
                &json!({"op": "lineTo"}),
                Compare::Structure
            )
            .is_some()
        );
    }

    #[test]
    fn key_sets_must_match() {
        assert!(
            diff(&json!({"a": 1}), &json!({}), Compare::Exact)
                .unwrap()
                .contains("missing key")
        );
        assert!(
            diff(&json!({}), &json!({"a": 1}), Compare::Exact)
                .unwrap()
                .contains("unexpected key")
        );
    }

    #[test]
    fn check_group_reports_failing_case() {
        let dir = std::env::temp_dir().join(format!("testkit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("g.json"),
            r#"{"source":"t","cases":[
{"name":"ok","call":"id","args":[1],"compare":"exact","expected":1},
{"name":"bad","call":"id","args":[2],"compare":"exact","expected":3}
]}"#,
        )
        .unwrap();
        let result = catch_unwind(|| check_group(&dir, "g", |case| case.args[0].clone()));
        let message = *result.unwrap_err().downcast::<String>().unwrap();
        assert!(message.contains("1 of 2 cases failed"), "{message}");
        assert!(message.contains("bad: $: expected 3"), "{message}");
    }
}
