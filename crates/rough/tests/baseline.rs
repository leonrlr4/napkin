//! Compares the port with roughjs@4.6.4 output recorded in `tests/baseline/*.json`.
//! One test per baseline group, so each porting task turns exactly its groups green.

use std::fmt::Display;
use std::path::PathBuf;

use rough::math::Random;
use rough::path_data::{self, Segment};
use serde_json::{Value, json};
use testkit::{check_group, to_value};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
}

fn throws(error: impl Display) -> Value {
    json!({ "throws": error.to_string() })
}

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(to_value).collect())
}

fn segments_value(segments: &[Segment]) -> Value {
    Value::Array(
        segments
            .iter()
            .map(|s| json!({ "key": s.key.to_string(), "data": numbers(&s.data) }))
            .collect(),
    )
}

#[test]
fn random() {
    check_group(&dir(), "random", |case| {
        let mut random = Random::new(case.num(0));
        Value::Array(
            (0..case.num(1) as usize)
                .map(|_| to_value(random.next()))
                .collect(),
        )
    });
}

#[test]
fn path_data() {
    check_group(&dir(), "path_data", |case| {
        let parsed = match path_data::parse_path(case.args[0].as_str().expect("path")) {
            Ok(segments) => segments,
            Err(e) => return throws(e),
        };
        match case.call.as_str() {
            "parsePath" => segments_value(&parsed),
            "absolutize" => segments_value(&path_data::absolutize(&parsed)),
            "normalize" => segments_value(&path_data::normalize(&path_data::absolutize(&parsed))),
            other => panic!("unknown call {other}"),
        }
    });
}
