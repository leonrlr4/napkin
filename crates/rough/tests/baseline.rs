//! Compares the port with roughjs@4.6.4 output recorded in `tests/baseline/*.json`.
//! One test per baseline group, so each porting task turns exactly its groups green.

use std::path::PathBuf;

use rough::math::Random;
use serde_json::Value;
use testkit::{check_group, to_value};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
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
