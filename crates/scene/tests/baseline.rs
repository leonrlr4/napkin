//! Compares scene with Excalidraw's own code at the pinned commit, recorded in
//! `tests/baseline/*.json` by tools/baseline/scene/generate.mjs.

use std::collections::HashSet;
use std::path::PathBuf;

use scene::element::{Element, Roundness, StrokeOptions};
use scene::env::Env;
use scene::fractional_index::{
    generate_key_between, generate_n_keys_between, sync_invalid_indices, sync_moved_indices,
};
use scene::new_element::{
    ElementProps, GenericKind, new_arrow_element, new_freedraw_element, new_generic_element,
    new_line_element,
};
use serde_json::{Value, json};
use testkit::{Case, check_group, num, points_from, throws};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
}

/// Random bytes all zero, clock pinned at 1 ms, matching the generator's `Date.now = () => 1`.
struct FixedEnv;

impl Env for FixedEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        bytes.fill(0);
    }

    fn now_ms(&mut self) -> f64 {
        1.0
    }
}

/// `newElement` options -> `ElementProps`. Keys the constructors read separately are skipped;
/// anything else unknown fails the case.
fn props_from(opts: &Value) -> ElementProps {
    let mut props = ElementProps::default();
    for (key, v) in opts.as_object().expect("opts object") {
        let s = || v.as_str().expect("string").to_owned();
        match key.as_str() {
            "x" => props.x = num(v),
            "y" => props.y = num(v),
            "width" => props.width = num(v),
            "height" => props.height = num(v),
            "angle" => props.angle = num(v),
            "strokeColor" => props.stroke_color = s(),
            "backgroundColor" => props.background_color = s(),
            "fillStyle" => props.fill_style = s(),
            "strokeWidth" => props.stroke_width = num(v),
            "strokeStyle" => props.stroke_style = s(),
            "roughness" => props.roughness = num(v),
            "opacity" => props.opacity = num(v),
            "groupIds" => props.group_ids = serde_json::from_value(v.clone()).expect("groupIds"),
            "roundness" => {
                props.roundness =
                    serde_json::from_value::<Option<Roundness>>(v.clone()).expect("roundness")
            }
            "locked" => props.locked = v.as_bool().expect("locked"),
            "type" | "id" | "seed" | "points" | "pressures" | "simulatePressure"
            | "strokeOptions" | "startArrowhead" | "endArrowhead" => {}
            other => panic!("unknown newElement option {other}"),
        }
    }
    props
}

#[test]
fn new_element() {
    check_group(&dir(), "new_element", |case| {
        let opts = &case.args[0];
        let props = props_from(opts);
        let points = || opts.get("points").map(points_from).unwrap_or_default();
        let head = |key| opts.get(key).and_then(Value::as_str).map(str::to_owned);
        let env = &mut FixedEnv;
        let element = match (case.call.as_str(), opts["type"].as_str()) {
            ("newElement", Some("rectangle")) => {
                new_generic_element(GenericKind::Rectangle, props, env)
            }
            ("newElement", Some("diamond")) => {
                new_generic_element(GenericKind::Diamond, props, env)
            }
            ("newElement", Some("ellipse")) => {
                new_generic_element(GenericKind::Ellipse, props, env)
            }
            ("newLinearElement", _) => new_line_element(props, points(), env),
            ("newArrowElement", _) => new_arrow_element(
                props,
                points(),
                head("startArrowhead"),
                head("endArrowhead"),
                env,
            ),
            ("newFreeDrawElement", _) => new_freedraw_element(
                props,
                points(),
                opts.get("pressures")
                    .map(|p| p.as_array().expect("pressures").iter().map(num).collect())
                    .unwrap_or_default(),
                opts["simulatePressure"]
                    .as_bool()
                    .expect("simulatePressure"),
                opts.get("strokeOptions").map(|o| {
                    serde_json::from_value::<StrokeOptions>(o.clone()).expect("strokeOptions")
                }),
                env,
            ),
            other => panic!("unknown constructor {other:?}"),
        };
        let mut value = element.to_value();
        // id and seed are random by design; the generator fixed them in opts.
        value["id"] = opts["id"].clone();
        value["seed"] = opts["seed"].clone();
        value
    });
}

fn key_arg(case: &Case, i: usize) -> Option<&str> {
    case.args[i].as_str()
}

/// Elements shaped like the generator's `{ id, type, index, version, versionNonce, updated }`.
/// They lack most fields, so they load as `Raw`, which is also the path images and frames take.
fn index_elements(indices: &Value) -> Vec<Element> {
    indices
        .as_array()
        .expect("indices")
        .iter()
        .enumerate()
        .map(|(i, index)| {
            Element::from_value(json!({
                "id": format!("e{i}"), "type": "rectangle", "index": index,
                "version": 1, "versionNonce": 0, "updated": 1,
            }))
        })
        .collect()
}

fn index_summary(elements: &[Element]) -> Value {
    Value::Array(
        elements
            .iter()
            .map(|e| {
                let v = e.to_value();
                json!({ "id": v["id"], "index": v["index"], "version": v["version"] })
            })
            .collect(),
    )
}

#[test]
fn fractional_index() {
    check_group(&dir(), "fractional_index", |case| {
        match case.call.as_str() {
            "generateKeyBetween" => {
                match generate_key_between(key_arg(case, 0), key_arg(case, 1)) {
                    Ok(key) => json!(key),
                    Err(e) => throws(e),
                }
            }
            "generateNKeysBetween" => {
                match generate_n_keys_between(
                    key_arg(case, 0),
                    key_arg(case, 1),
                    case.num(2) as usize,
                ) {
                    Ok(keys) => json!(keys),
                    Err(e) => throws(e),
                }
            }
            "syncMovedIndices" => {
                let mut elements = index_elements(&case.args[0]);
                let moved: HashSet<String> = case.args[1]
                    .as_array()
                    .expect("moved positions")
                    .iter()
                    .map(|i| format!("e{}", num(i)))
                    .collect();
                sync_moved_indices(&mut elements, &moved, &mut FixedEnv);
                index_summary(&elements)
            }
            "syncInvalidIndices" => {
                let mut elements = index_elements(&case.args[0]);
                sync_invalid_indices(&mut elements, &mut FixedEnv);
                index_summary(&elements)
            }
            other => panic!("unknown call {other}"),
        }
    });
}
