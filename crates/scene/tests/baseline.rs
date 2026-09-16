//! Compares scene with Excalidraw's own code at the pinned commit, recorded in
//! `tests/baseline/*.json` by tools/baseline/scene/generate.mjs.

use std::collections::HashSet;
use std::path::PathBuf;

use scene::color::{apply_dark_mode_filter, is_transparent};
use scene::element::{Element, Roundness, StrokeOptions};
use scene::env::Env;
use scene::fractional_index::{
    generate_key_between, generate_n_keys_between, sync_invalid_indices, sync_moved_indices,
};
use scene::new_element::{
    ElementProps, GenericKind, new_arrow_element, new_freedraw_element, new_generic_element,
    new_line_element,
};
use scene::shape::{
    ElementShape, PathOp, ShapeContext, freedraw_outline_points, generate_element_shape,
    generate_rough_options,
};
use serde_json::{Value, json};
use testkit::rough_json::{drawable_value, options_value};
use testkit::{Case, check_group, num, numbers, point_value, points_from, throws};

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

#[test]
fn colors() {
    check_group(&dir(), "colors", |case| {
        let color = case.args[0].as_str().expect("color");
        match case.call.as_str() {
            "applyDarkModeFilter" => json!(apply_dark_mode_filter(color)),
            "isTransparent" => json!(is_transparent(color)),
            other => panic!("unknown call {other}"),
        }
    });
}

/// Element JSON from a case, which must load as a typed element when its type is one scene
/// draws: a silent fallback to `Raw` would make every shape comparison vacuous.
fn element_from(value: &Value) -> Element {
    let element = Element::from_value(value.clone());
    let drawn = [
        "rectangle",
        "diamond",
        "ellipse",
        "line",
        "arrow",
        "freedraw",
        "text",
    ];
    if value["type"].as_str().is_some_and(|t| drawn.contains(&t)) {
        assert!(
            !matches!(element, Element::Raw(_)),
            "baseline element fell back to Raw"
        );
    }
    element
}

#[test]
fn rough_options() {
    check_group(&dir(), "rough_options", |case| {
        let element = element_from(&case.args[0]);
        let continuous = case.args[1].as_bool().expect("continuousPath");
        let dark = case.args[2].as_bool().expect("isDarkMode");
        options_value(&generate_rough_options(&element, continuous, dark).expect("drawable type"))
    });
}

fn path_op_value(op: &PathOp) -> Value {
    let (name, data): (&str, &[f64]) = match op {
        PathOp::Move(d) => ("move", d),
        PathOp::Line(d) => ("line", d),
        PathOp::Quad(d) => ("quad", d),
        PathOp::Close => ("close", &[]),
    };
    json!({ "op": name, "data": numbers(data) })
}

/// The JSON Excalidraw's ShapeCache returns for each element type.
fn shape_value(element: &Element, shape: &ElementShape) -> Value {
    match shape {
        ElementShape::None => Value::Null,
        ElementShape::Drawables(drawables) => match element {
            Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_) => {
                assert_eq!(drawables.len(), 1, "generic elements have one drawable");
                drawable_value(&drawables[0])
            }
            _ => Value::Array(drawables.iter().map(drawable_value).collect()),
        },
        ElementShape::Freedraw { fill, stroke } => {
            let mut items: Vec<Value> = fill.iter().map(|d| drawable_value(d)).collect();
            items.push(json!({ "svgPath": stroke.iter().map(path_op_value).collect::<Vec<_>>() }));
            Value::Array(items)
        }
        ElementShape::Placeholder => {
            unreachable!("no baseline element's geometry exceeds GEOMETRY_BOUND")
        }
    }
}

fn check_shapes(group: &str) {
    check_group(&dir(), group, |case| {
        let element = element_from(&case.args[0]);
        let context = &case.args[1];
        let ctx = ShapeContext {
            dark_mode: context["theme"] == "dark",
            canvas_background_color: context["canvasBackgroundColor"].as_str().expect("color"),
        };
        shape_value(&element, &generate_element_shape(&element, &ctx))
    });
}

#[test]
fn shapes_generic() {
    check_shapes("shapes_generic");
}

#[test]
fn shapes_other() {
    check_shapes("shapes_other");
}

#[test]
fn shapes_linear() {
    check_shapes("shapes_linear");
}

#[test]
fn shapes_freedraw() {
    check_shapes("shapes_freedraw");
}

#[test]
fn freedraw_outline() {
    check_group(&dir(), "freedraw_outline", |case| {
        let Element::Freedraw(element) = element_from(&case.args[0]) else {
            panic!("not a freedraw element");
        };
        Value::Array(
            freedraw_outline_points(&element)
                .into_iter()
                .map(point_value)
                .collect(),
        )
    });
}
