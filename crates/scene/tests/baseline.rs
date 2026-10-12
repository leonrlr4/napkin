//! Compares scene with Excalidraw's own code at the pinned commit, recorded in
//! `tests/baseline/*.json` by tools/baseline/scene/generate.mjs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use scene::batch::add_elements;
use scene::color::{apply_dark_mode_filter, is_transparent};
use scene::duplicate::{DuplicateMode, duplicate_elements};
use scene::editor::{ArrowType, EdgeStyle, ItemStyle};
use scene::element::{Element, LinearEnd, Roundness, StrokeOptions};
use scene::env::Env;
use scene::fractional_index::{
    generate_key_between, generate_n_keys_between, sync_invalid_indices, sync_moved_indices,
};
use scene::new_element::{
    ElementProps, GenericKind, ImageProps, TextProps, new_arrow_element, new_freedraw_element,
    new_generic_element, new_image_element, new_line_element, new_text_element,
};
use scene::sample::{self, CharWidthMeasure};
use scene::selection::Selection;
use scene::shape::{
    ElementShape, PathOp, ShapeContext, freedraw_outline_points, generate_element_shape,
    generate_rough_options,
};
use scene::transform::HandleKind;
use scene::zindex::{self, Direction};
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
            | "strokeOptions" | "startArrowhead" | "endArrowhead" | "text" | "fontSize"
            | "fontFamily" | "textAlign" | "verticalAlign" | "containerId" | "lineHeight"
            | "fileId" | "status" | "scale" => {}
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
            ("newImageElement", _) => new_image_element(
                props,
                ImageProps {
                    file_id: head("fileId"),
                    status: head("status"),
                    scale: opts.get("scale").map(|s| {
                        let s = s.as_array().expect("scale");
                        [num(&s[0]), num(&s[1])]
                    }),
                },
                env,
            ),
            ("newTextElement", _) => new_text_element(
                props,
                TextProps {
                    text: opts["text"].as_str().expect("text").to_owned(),
                    font_size: opts.get("fontSize").map(num),
                    font_family: opts.get("fontFamily").map(num),
                    text_align: head("textAlign"),
                    vertical_align: head("verticalAlign"),
                    container_id: head("containerId"),
                    line_height: opts.get("lineHeight").map(num),
                },
                &mut CharWidthMeasure,
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
/// They lack most fields, so they load as `Raw`, which is also the path frames take.
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

/// A complete rectangle (or `t`'s text label, or the `frame1` frame) element for the zindex
/// cases, mirroring `tools/baseline/scene/generate.mjs`'s `zindexElement`: `g1`/`g2` are in
/// group `"G"`, `t` is `r`'s bound label, `del`/`fbDel` are soft-deleted, `frame1` is a `frame`
/// element, `fa`/`fb`/`fbDel` are its children, and every other field but `index` (`a0`, `a1`,
/// ... in array order) is a fixed default. Built from `sample`'s full-field JSON so it loads as
/// a typed element, not `Raw` (`frame1` is the one exception: napkin has no typed frame
/// element, so it falls back to `Raw` the same way it would loading a real `.excalidraw` file).
fn zindex_element(id: &str, position: usize) -> Value {
    let mut value = if id == "t" {
        sample::text(id, [0.0, 0.0, 10.0, 10.0], "hi", Some("r"))
    } else {
        sample::generic("rectangle", id, [0.0, 0.0, 10.0, 10.0])
    };
    value["index"] = json!(format!("a{position}"));
    value["version"] = json!(1);
    value["versionNonce"] = json!(0);
    value["updated"] = json!(1);
    if id == "g1" || id == "g2" {
        value["groupIds"] = json!(["G"]);
    }
    if id == "del" || id == "fbDel" {
        value["isDeleted"] = json!(true);
    }
    if id == "r" {
        value["boundElements"] = json!([{"id": "t", "type": "text"}]);
    }
    if matches!(id, "fa" | "fb" | "fbDel") {
        value["frameId"] = json!("frame1");
    }
    if id == "frame1" {
        value["type"] = json!("frame");
    }
    value
}

#[test]
fn zindex() {
    check_group(&dir(), "zindex", |case| {
        let ids: Vec<&str> = case.args[0]
            .as_array()
            .expect("ids")
            .iter()
            .map(|v| v.as_str().expect("id"))
            .collect();
        let selected: Vec<&str> = case.args[1]
            .as_array()
            .expect("selected")
            .iter()
            .map(|v| v.as_str().expect("id"))
            .collect();
        let direction = match case.args[2].as_str().expect("direction") {
            "left" => Direction::Left,
            "right" => Direction::Right,
            other => panic!("unknown direction {other}"),
        };
        let elements: Vec<Element> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| element_from(&zindex_element(id, i)))
            .collect();
        let selection = Selection::from_ids(selected);
        let result =
            zindex::move_one(&elements, &selection, direction, &mut FixedEnv).unwrap_or(elements);
        index_summary(&result)
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

/// `generate.mjs`'s `normalizeSkeletonOutput`: drops `seed` and `versionNonce`, renames ids to
/// `e<position>` in order (references included), and renames every `groupId` to `g<n>`
/// (1-based) in first-occurrence order, scanning each element's own `groupIds` in the same
/// output-order pass. Shared by the `skeleton` and `duplicate` groups.
fn normalize_skeleton_output(elements: &[Element]) -> Value {
    let values: Vec<Value> = elements.iter().map(Element::to_value).collect();
    let rename: HashMap<String, String> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (v["id"].as_str().expect("id").to_owned(), format!("e{i}")))
        .collect();
    let id = |v: &Value| {
        json!(
            rename
                .get(v.as_str().unwrap_or(""))
                .cloned()
                .unwrap_or_default()
        )
    };
    let mut group_rename: HashMap<String, String> = HashMap::new();
    let mut group_id = |raw: &str| -> String {
        if let Some(existing) = group_rename.get(raw) {
            return existing.clone();
        }
        let name = format!("g{}", group_rename.len() + 1);
        group_rename.insert(raw.to_owned(), name.clone());
        name
    };
    Value::Array(
        values
            .into_iter()
            .map(|mut v| {
                let map = v.as_object_mut().expect("object");
                map.remove("seed");
                map.remove("versionNonce");
                map["id"] = id(&map["id"]);
                if map.get("containerId").is_some_and(Value::is_string) {
                    map["containerId"] = id(&map["containerId"]);
                }
                if let Some(Value::Array(bound)) = map.get_mut("boundElements") {
                    for b in bound {
                        b["id"] = id(&b["id"]);
                    }
                }
                for key in ["startBinding", "endBinding"] {
                    if let Some(binding) = map.get_mut(key).filter(|b| b.is_object()) {
                        binding["elementId"] = id(&binding["elementId"]);
                    }
                }
                if let Some(Value::Array(group_ids)) = map.get_mut("groupIds") {
                    for g in group_ids.iter_mut() {
                        if let Some(s) = g.as_str() {
                            *g = json!(group_id(s));
                        }
                    }
                }
                v
            })
            .collect(),
    )
}

/// Random bytes that advance on every call, unlike [`FixedEnv`]'s constant zero bytes: the
/// skeleton conversion creates several elements per case, and `normalize_skeleton_output`
/// needs each one's generated id to be distinct in order to rename them correctly. `FixedEnv`
/// itself can stay constant because every other group either ignores the generated id
/// (`new_element` overwrites it from the case's own `opts.id`) or never generates one
/// (`fractional_index`'s elements get an explicit `id` directly).
#[derive(Default)]
struct DistinctIdEnv {
    next: u8,
}

impl Env for DistinctIdEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        bytes.fill(self.next);
        self.next = self.next.wrapping_add(1);
    }

    fn now_ms(&mut self) -> f64 {
        1.0
    }
}

#[test]
fn skeleton() {
    let style = ItemStyle {
        edges: EdgeStyle::Sharp,
        arrow_type: ArrowType::Sharp,
        ..ItemStyle::default()
    };
    check_group(&dir(), "skeleton", |case| {
        let skeletons = case.args[0].as_array().expect("skeletons");
        let pairs: Vec<(usize, &Value)> = skeletons.iter().enumerate().collect();
        let mut file = scene::SceneFile::new();
        add_elements(
            &mut file,
            &pairs,
            &style,
            &mut CharWidthMeasure,
            &mut DistinctIdEnv::default(),
        )
        .unwrap_or_else(|errors| panic!("{errors:?}"));
        normalize_skeleton_output(&file.elements)
    });
}

#[test]
fn duplicate() {
    check_group(&dir(), "duplicate", |case| {
        let elements: Vec<Element> = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let ids: Vec<String> = case.args[1]
            .as_array()
            .expect("ids")
            .iter()
            .map(|v| v.as_str().expect("id").to_owned())
            .collect();
        let selection = Selection::from_ids(ids);
        let mode = match case.args[2].as_str().expect("mode") {
            "in-place" => DuplicateMode::InPlace {
                offset: [
                    scene::duplicate::DEFAULT_GRID_SIZE / 2.0,
                    scene::duplicate::DEFAULT_GRID_SIZE / 2.0,
                ],
            },
            "everything" => DuplicateMode::Everything,
            other => panic!("unknown mode {other}"),
        };
        let mut env = DistinctIdEnv::default();
        let mut duplicated = duplicate_elements(&elements, &selection, mode, &mut env);
        let moved: HashSet<String> = duplicated.new_ids.iter().cloned().collect();
        sync_moved_indices(&mut duplicated.elements, &moved, &mut env);
        normalize_skeleton_output(&duplicated.elements)
    });
}

#[test]
fn text_wrap() {
    check_group(&dir(), "text_wrap", |case| match case.call.as_str() {
        "parseTokens" => json!(scene::text_wrap::parse_tokens(
            case.args[0].as_str().expect("line")
        )),
        "wrapText" => json!(scene::text_wrap::wrap_text(
            case.args[0].as_str().expect("text"),
            case.num(2),
            case.num(1),
            case.num(3),
            &mut CharWidthMeasure,
        )),
        other => panic!("unknown call {other}"),
    });
}

#[test]
fn bound_text() {
    check_group(&dir(), "bound_text", |case| {
        let mut file = scene::SceneFile::new();
        file.elements = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let op = &case.args[1];
        let position = |id: &str| {
            file.elements
                .iter()
                .position(|e| e.id() == Some(id))
                .expect("element id")
        };
        let mut env = FixedEnv;
        if let Some(text_id) = op.get("redraw").and_then(Value::as_str) {
            let text = position(text_id);
            let container = op.get("container").and_then(Value::as_str).map(position);
            scene::bound_text::redraw_text_bounding_box(
                &mut file,
                text,
                container,
                &mut CharWidthMeasure,
                &mut env,
            );
        } else {
            let container = position("c");
            let geometry = op["resize"].as_object().expect("resize geometry");
            let base = file.elements[container]
                .base_mut()
                .expect("typed container");
            for (key, v) in geometry {
                match key.as_str() {
                    "x" => base.x = num(v),
                    "y" => base.y = num(v),
                    "width" => base.width = num(v),
                    "height" => base.height = num(v),
                    other => panic!("unknown geometry key {other}"),
                }
            }
            let handle = match op["handle"].as_str().expect("handle") {
                "n" => HandleKind::N,
                "s" => HandleKind::S,
                "e" => HandleKind::E,
                "w" => HandleKind::W,
                "nw" => HandleKind::Nw,
                "ne" => HandleKind::Ne,
                "sw" => HandleKind::Sw,
                "se" => HandleKind::Se,
                other => panic!("unknown handle {other}"),
            };
            let flag = |key: &str| op[key].as_bool().expect("flag");
            scene::bound_text::handle_bind_text_resize(
                &mut file,
                container,
                Some(handle),
                flag("keepAspect"),
                flag("fromCenter"),
                flag("flipY"),
                &mut CharWidthMeasure,
                &mut env,
            );
        }
        Value::Array(
            file.elements
                .iter()
                .map(|e| {
                    let p = e.placement().expect("placement");
                    let mut out = json!({
                        "id": e.id(), "x": p.x, "y": p.y, "width": p.width, "height": p.height
                    });
                    if let Element::Text(t) = e {
                        out["text"] = json!(t.text);
                    }
                    out
                })
                .collect(),
        )
    });
}

#[test]
fn arrow_label() {
    check_group(&dir(), "arrow_label", |case| {
        let mut file = scene::SceneFile::new();
        file.elements = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let mut geometry = scene::geometry::GeometryCache::default();
        scene::bound_text::sync_arrow_label(
            &mut geometry,
            &mut file,
            0,
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        let arrow = file.elements[0].placement().expect("arrow placement");
        let Element::Text(label) = &file.elements[1] else {
            panic!("typed label");
        };
        json!([
            {"id": "w", "height": arrow.height},
            {
                "id": "t", "text": label.text, "width": label.base.width,
                "height": label.base.height, "x": label.base.x, "y": label.base.y
            },
        ])
    });
}

#[test]
fn transform() {
    check_group(&dir(), "transform", |case| {
        let mut start = scene::SceneFile::new();
        start.elements = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let mut targets: Vec<usize> = case.args[1]
            .as_array()
            .expect("ids")
            .iter()
            .map(|id| {
                start
                    .elements
                    .iter()
                    .position(|e| e.id() == id.as_str())
                    .expect("element id")
            })
            .collect();
        targets.sort_unstable();
        let handle = case.args[2].as_str().expect("handle");
        let point = |v: &Value| [num(&v[0]), num(&v[1])];
        let pointer = point(&case.args[3]);
        let center = point(&case.args[4]);
        let flag = |key: &str| case.args[5][key].as_bool().expect("flag");

        let mut file = start.clone();
        let mut env = FixedEnv;
        if handle == "rotation" {
            scene::transform::rotate_elements(
                &mut scene::geometry::GeometryCache::default(),
                &mut file,
                &start,
                &targets,
                pointer,
                center,
                flag("shift"),
                &mut CharWidthMeasure,
                &mut env,
            );
        } else {
            let handle = match handle {
                "n" => HandleKind::N,
                "s" => HandleKind::S,
                "e" => HandleKind::E,
                "w" => HandleKind::W,
                "nw" => HandleKind::Nw,
                "ne" => HandleKind::Ne,
                "sw" => HandleKind::Sw,
                "se" => HandleKind::Se,
                other => panic!("unknown handle {other}"),
            };
            scene::transform::resize_elements(
                &mut scene::geometry::GeometryCache::default(),
                &mut file,
                &start,
                &targets,
                handle,
                pointer,
                scene::transform::ResizeOptions {
                    keep_aspect_ratio: flag("keepAspect"),
                    from_center: flag("fromCenter"),
                },
                &mut CharWidthMeasure,
                &mut env,
            );
        }
        Value::Array(
            file.elements
                .iter()
                .map(|e| {
                    let p = e.placement().expect("placement");
                    let mut out = json!({
                        "id": e.id(), "x": p.x, "y": p.y, "width": p.width,
                        "height": p.height, "angle": p.angle
                    });
                    match e {
                        Element::Line(l) | Element::Arrow(l) => out["points"] = json!(l.points),
                        Element::Freedraw(f) => out["points"] = json!(f.points),
                        Element::Text(t) => {
                            out["fontSize"] = json!(t.font_size);
                            out["text"] = json!(t.text);
                        }
                        Element::Image(i) => out["scale"] = json!(i.scale()),
                        _ => {}
                    }
                    if e.kind() == "arrow" {
                        out["startBinding"] = json!(e.binding_target(LinearEnd::Start));
                        out["endBinding"] = json!(e.binding_target(LinearEnd::End));
                    }
                    out["boundElements"] = json!(
                        e.bound_elements()
                            .into_iter()
                            .map(|(id, _)| id)
                            .collect::<Vec<_>>()
                    );
                    out
                })
                .collect(),
        )
    });
}

#[test]
fn bound_follow() {
    check_group(&dir(), "bound_follow", |case| {
        let mut start = scene::SceneFile::new();
        start.elements = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let ids: Vec<&str> = case.args[1]
            .as_array()
            .expect("ids")
            .iter()
            .map(|id| id.as_str().expect("id"))
            .collect();
        let op = &case.args[2];
        let point = |v: &Value| [num(&v[0]), num(&v[1])];

        let mut file = start.clone();
        let mut geometry = scene::geometry::GeometryCache::default();
        let mut env = FixedEnv;
        if let Some(offset) = op.get("drag") {
            let targets = scene::edit::drag_targets(&start, &Selection::from_ids(ids));
            scene::edit::apply_drag(
                &mut geometry,
                &mut file,
                &start,
                &targets,
                point(offset),
                &mut CharWidthMeasure,
                &mut env,
            );
        } else {
            let args = &op["transform"];
            let handle = args[0].as_str().expect("handle");
            let pointer = point(&args[1]);
            let center = point(&args[2]);
            let flag = |key: &str| args[3][key].as_bool().expect("flag");
            let mut targets: Vec<usize> = ids
                .iter()
                .map(|id| {
                    start
                        .elements
                        .iter()
                        .position(|e| e.id() == Some(*id))
                        .expect("element id")
                })
                .collect();
            targets.sort_unstable();
            if handle == "rotation" {
                scene::transform::rotate_elements(
                    &mut geometry,
                    &mut file,
                    &start,
                    &targets,
                    pointer,
                    center,
                    flag("shift"),
                    &mut CharWidthMeasure,
                    &mut env,
                );
            } else {
                let handle = match handle {
                    "n" => HandleKind::N,
                    "s" => HandleKind::S,
                    "e" => HandleKind::E,
                    "w" => HandleKind::W,
                    "nw" => HandleKind::Nw,
                    "ne" => HandleKind::Ne,
                    "sw" => HandleKind::Sw,
                    "se" => HandleKind::Se,
                    other => panic!("unknown handle {other}"),
                };
                scene::transform::resize_elements(
                    &mut geometry,
                    &mut file,
                    &start,
                    &targets,
                    handle,
                    pointer,
                    scene::transform::ResizeOptions {
                        keep_aspect_ratio: flag("keepAspect"),
                        from_center: flag("fromCenter"),
                    },
                    &mut CharWidthMeasure,
                    &mut env,
                );
            }
        }
        Value::Array(
            file.elements
                .iter()
                .map(|e| {
                    let p = e.placement().expect("placement");
                    let mut out = json!({
                        "id": e.id(), "x": p.x, "y": p.y, "width": p.width,
                        "height": p.height, "angle": p.angle
                    });
                    match e {
                        Element::Line(l) | Element::Arrow(l) => out["points"] = json!(l.points),
                        Element::Freedraw(f) => out["points"] = json!(f.points),
                        Element::Text(t) => {
                            out["fontSize"] = json!(t.font_size);
                            out["text"] = json!(t.text);
                        }
                        Element::Image(i) => out["scale"] = json!(i.scale()),
                        _ => {}
                    }
                    if e.kind() == "arrow" {
                        for (key, end) in [
                            ("startBinding", LinearEnd::Start),
                            ("endBinding", LinearEnd::End),
                        ] {
                            out[key] = e.binding_value(end).map_or(Value::Null, |binding| {
                                json!({
                                    "elementId": binding["elementId"],
                                    "mode": binding["mode"],
                                    "fixedPoint": binding["fixedPoint"],
                                })
                            });
                        }
                    }
                    out["boundElements"] = json!(
                        e.bound_elements()
                            .into_iter()
                            .map(|(id, _)| id)
                            .collect::<Vec<_>>()
                    );
                    out
                })
                .collect(),
        )
    });
}

#[test]
fn intersect() {
    check_group(&dir(), "intersect", |case| {
        let elements: Vec<Element> = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .cloned()
            .map(Element::from_value)
            .collect();
        let point = |v: &Value| [num(&v[0]), num(&v[1])];
        let mut geometry = scene::geometry::GeometryCache::default();
        match case.call.as_str() {
            "intersectElementWithLineSegment" => {
                let id = case.args[1].as_str().expect("id");
                let element = elements
                    .iter()
                    .find(|e| e.id() == Some(id))
                    .expect("element id");
                let segment = [point(&case.args[2][0]), point(&case.args[2][1])];
                let hits = scene::collision::intersect_element_with_line_segment(
                    &mut geometry,
                    element,
                    segment,
                    num(&case.args[3]),
                );
                json!(hits)
            }
            "getHoveredElementForBinding" => {
                let hit = scene::collision::hovered_element_for_binding(
                    &mut geometry,
                    &elements,
                    point(&case.args[1]),
                    num(&case.args[2]),
                );
                json!(hit.map(|index| elements[index].id()))
            }
            "hitElementItself" => {
                let id = case.args[1].as_str().expect("id");
                let element = elements
                    .iter()
                    .find(|e| e.id() == Some(id))
                    .expect("element id");
                json!(scene::collision::hit_element_itself_with(
                    &mut geometry,
                    element,
                    point(&case.args[2]),
                    num(&case.args[3]),
                    case.args[4].as_bool().expect("override flag"),
                ))
            }
            other => panic!("unknown call {other}"),
        }
    });
}

/// One `arrow_binding` gesture replayed through the scene API: the same event sequence the
/// generator drives through Excalidraw's `handlePointDragging`/`handlePointerMove`/
/// `actionFinalize`, with `delta = pointer - offset - current point` computed the way
/// `LinearElementEditor.createPointAt` does for an unrotated arrow.
struct ArrowGesture {
    file: scene::SceneFile,
    geometry: scene::geometry::GeometryCache,
    env: FixedEnv,
    state: scene::binding::LinearDragState,
    enabled: bool,
    zoom: f64,
    arrow_id: String,
    /// The point being dragged.
    index: usize,
    suggested: Option<String>,
}

impl ArrowGesture {
    fn arrow(&self) -> usize {
        self.file
            .elements
            .iter()
            .position(|e| e.id() == Some(self.arrow_id.as_str()))
            .expect("arrow")
    }

    fn input(&self, pointer: [f64; 2], angle_locked: bool) -> scene::binding::BindingInput {
        scene::binding::BindingInput {
            pointer,
            zoom: self.zoom,
            angle_locked,
            enabled: self.enabled,
        }
    }

    /// A drag step to `pointer` (`Shift` locks the angle around the neighbouring point).
    fn drag(&mut self, pointer: [f64; 2], shift: bool) {
        let arrow = self.arrow();
        let Element::Arrow(l) = &self.file.elements[arrow] else {
            panic!("an arrow")
        };
        assert_eq!(l.base.angle, 0.0, "the replay assumes an unrotated arrow");
        let origin = [l.base.x, l.base.y];
        let current = l.points[self.index];
        let target = if shift {
            let pivot = l.points[if self.index == 0 { 1 } else { self.index - 1 }];
            let locked = scene::edit::lock_linear_angle(
                [origin[0] + pivot[0], origin[1] + pivot[1]],
                pointer,
            );
            [locked[0] - origin[0], locked[1] - origin[1]]
        } else {
            let offset = self.state.pointer_offset;
            [
                pointer[0] - offset[0] - origin[0],
                pointer[1] - offset[1] - origin[1],
            ]
        };
        let delta = [target[0] - current[0], target[1] - current[1]];
        let input = self.input(pointer, shift);
        let suggestion = scene::binding::drag_points(
            &mut self.geometry,
            &mut self.file,
            arrow,
            &[self.index],
            delta,
            input,
            &mut self.state,
            &mut self.env,
        );
        self.suggested = suggestion.map(|s| {
            self.file.elements[s.target]
                .id()
                .expect("target id")
                .to_owned()
        });
    }

    /// `actionFinalize`'s first branch: the pointer-up at `up`.
    fn finalize(&mut self, up: [f64; 2], shift: bool) {
        let arrow = self.arrow();
        let Element::Arrow(l) = &self.file.elements[arrow] else {
            panic!("an arrow")
        };
        let last = l.points.len() - 1;
        if self.index != 0 && self.index != last {
            // A dragged middle point leaves `segmentMidPointHoveredCoords` set, which skips the
            // binding step.
            return;
        }
        let local = if shift {
            l.points[self.index]
        } else {
            let offset = self.state.pointer_offset;
            [up[0] - offset[0] - l.base.x, up[1] - offset[1] - l.base.y]
        };
        let input = self.input(up, shift);
        scene::binding::finalize_binding(
            &mut self.geometry,
            &mut self.file,
            arrow,
            &[(self.index, local)],
            input,
            &self.state,
            false,
            &mut self.env,
        );
    }

    /// App.tsx's pointer-down with the arrow tool.
    fn press_arrow_tool(&mut self, origin: [f64; 2]) {
        let props = ElementProps {
            x: origin[0],
            y: origin[1],
            roundness: Some(Roundness {
                kind: 2.0,
                value: scene::json::Slot::Missing,
                extra: Default::default(),
            }),
            ..ElementProps::default()
        };
        let mut arrow = new_arrow_element(
            props,
            vec![[0.0, 0.0], [0.0, 0.0]],
            None,
            Some("arrow".into()),
            &mut self.env,
        );
        if let Element::Arrow(l) = &mut arrow {
            l.base.id = self.arrow_id.clone();
        }
        let position = scene::edit::append_element(&mut self.file, arrow, &mut self.env);
        let input = self.input(origin, false);
        scene::binding::finalize_binding(
            &mut self.geometry,
            &mut self.file,
            position,
            &[(0, [0.0, 0.0])],
            input,
            &scene::binding::LinearDragState::default(),
            true,
            &mut self.env,
        );
        self.state = scene::binding::LinearDragState {
            new_arrow: true,
            origin: Some(origin),
            ..Default::default()
        };
        self.index = 1;
    }

    /// `mutateElement(arrow, {points})`: size follows the points.
    fn set_points(&mut self, points: Vec<[f64; 2]>) {
        let arrow = self.arrow();
        if let Element::Arrow(l) = &mut self.file.elements[arrow] {
            let [width, height] = scene::geometry::size_from_points(&points);
            l.points = points;
            l.base.width = width;
            l.base.height = height;
        }
        scene::new_element::bump_version(&mut self.file.elements[arrow], &mut self.env);
    }
}

fn arrow_binding_point(value: &Value) -> ([f64; 2], bool) {
    (
        [num(&value[0]), num(&value[1])],
        value.get(2).and_then(Value::as_bool).unwrap_or(false),
    )
}

fn arrow_binding_output(gesture: &ArrowGesture, suggested: Option<&str>) -> Value {
    let elements: Vec<Value> = gesture
        .file
        .elements
        .iter()
        .map(|e| {
            let p = e.placement().expect("placement");
            let mut out = json!({
                "id": e.id(), "x": p.x, "y": p.y, "width": p.width, "height": p.height
            });
            if let Element::Line(l) | Element::Arrow(l) = e {
                out["points"] = json!(l.points);
            }
            if e.kind() == "arrow" {
                for (key, end) in [
                    ("startBinding", LinearEnd::Start),
                    ("endBinding", LinearEnd::End),
                ] {
                    out[key] = e.binding_value(end).map_or(Value::Null, |binding| {
                        json!({
                            "elementId": binding["elementId"],
                            "mode": binding["mode"],
                            "fixedPoint": binding["fixedPoint"],
                        })
                    });
                }
            }
            out["boundElements"] = json!(
                e.bound_elements()
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>()
            );
            out
        })
        .collect();
    json!({"elements": elements, "suggestedBinding": suggested})
}

#[test]
fn arrow_binding() {
    check_group(&dir(), "arrow_binding", |case| {
        let mut file = scene::SceneFile::new();
        file.elements = case.args[0]
            .as_array()
            .expect("elements")
            .iter()
            .map(element_from)
            .collect();
        let spec = &case.args[1];
        let kind = spec["kind"].as_str().expect("gesture kind");
        let mut gesture = ArrowGesture {
            file,
            geometry: scene::geometry::GeometryCache::default(),
            env: FixedEnv,
            state: Default::default(),
            enabled: !spec["ctrl"].as_bool().unwrap_or(false),
            zoom: spec["zoom"].as_f64().unwrap_or(1.0),
            arrow_id: "new".into(),
            index: 0,
            suggested: None,
        };
        let moves = |key: &str| -> Vec<([f64; 2], bool)> {
            spec[key]
                .as_array()
                .expect("moves")
                .iter()
                .map(arrow_binding_point)
                .collect()
        };

        match kind {
            "create" => {
                let origin = arrow_binding_point(&spec["origin"]).0;
                gesture.press_arrow_tool(origin);
                let moves = moves("moves");
                for &(point, shift) in &moves {
                    gesture.drag(point, shift);
                }
                let up = spec
                    .get("up")
                    .map_or_else(|| *moves.last().expect("a move"), arrow_binding_point);
                gesture.finalize(up.0, up.1);
            }
            "dragPoint" => {
                gesture.arrow_id = spec["id"].as_str().expect("arrow id").to_owned();
                let index = spec["index"].as_u64().expect("point index") as usize;
                let arrow = gesture.arrow();
                let Element::Arrow(l) = &gesture.file.elements[arrow] else {
                    panic!("an arrow")
                };
                let at = [l.base.x + l.points[index][0], l.base.y + l.points[index][1]];
                let grab = spec.get("grab").map_or(at, |g| arrow_binding_point(g).0);
                let other = gesture.file.elements[arrow].binding_value(if index == 0 {
                    LinearEnd::End
                } else {
                    LinearEnd::Start
                });
                gesture.state = scene::binding::LinearDragState {
                    new_arrow: false,
                    origin: Some(grab),
                    other_end_started_inside: other
                        .is_some_and(|binding| binding["mode"].as_str() == Some("inside")),
                    pointer_offset: [grab[0] - at[0], grab[1] - at[1]],
                    ..Default::default()
                };
                gesture.index = index;
                let moves = moves("moves");
                for &(point, shift) in &moves {
                    gesture.drag(point, shift);
                }
                let up = spec
                    .get("up")
                    .map_or_else(|| *moves.last().expect("a move"), arrow_binding_point);
                gesture.finalize(up.0, up.1);
            }
            "clickCreate" => {
                let origin = arrow_binding_point(&spec["origin"]).0;
                gesture.press_arrow_tool(origin);
                // `lastCommittedPoint` (unset until a click commits the following point) and
                // whether it is still the arrow's last point object.
                let mut committed: Option<[f64; 2]> = None;
                let mut committed_is_last = false;
                let mut finished = false;
                for event in spec["events"].as_array().expect("events") {
                    assert!(!finished, "events after the arrow was finished");
                    let kind = event[0].as_str().expect("event kind");
                    let at = [num(&event[1]), num(&event[2])];
                    let arrow = gesture.arrow();
                    let Element::Arrow(l) = &gesture.file.elements[arrow] else {
                        panic!("an arrow")
                    };
                    let origin = [l.base.x, l.base.y];
                    let points = l.points.clone();
                    let local = [at[0] - origin[0], at[1] - origin[1]];
                    let distance =
                        |a: [f64; 2], b: [f64; 2]| rough::js::hypot(a[0] - b[0], a[1] - b[1]);
                    let last = *points.last().expect("points");
                    if kind == "move" {
                        if committed_is_last {
                            if distance(local, last) >= scene::collision::LINE_CONFIRM_THRESHOLD {
                                let mut next = points.clone();
                                next.push(local);
                                gesture.set_points(next);
                                committed_is_last = false;
                            }
                        } else if points.len() > 2
                            && committed.is_some_and(|c| {
                                distance(local, c) < scene::collision::LINE_CONFIRM_THRESHOLD
                            })
                        {
                            let mut next = points.clone();
                            next.pop();
                            let new_last = *next.last().expect("points");
                            gesture.set_points(next);
                            committed = Some(new_last);
                            committed_is_last = true;
                        } else {
                            gesture.index = points.len() - 1;
                            gesture.drag(at, false);
                            committed_is_last = false;
                        }
                    } else {
                        let finishes = scene::binding::multi_point_click_finishes(
                            &mut gesture.geometry,
                            &gesture.file.elements,
                            arrow,
                            at,
                            gesture.zoom,
                            gesture.enabled,
                            committed,
                            &gesture.state,
                        );
                        if finishes {
                            gesture.index = points.len() - 1;
                            gesture.finalize(at, false);
                            finished = true;
                        } else {
                            committed = Some(last);
                            committed_is_last = true;
                        }
                    }
                }
                assert!(finished, "the click-create gesture never finished");
                // `handlePointerMove` can skip its state update, so the generator reports no
                // suggestion for these gestures.
                gesture.suggested = None;
            }
            other => panic!("unknown gesture {other}"),
        }
        let suggested = gesture.suggested.clone();
        arrow_binding_output(&gesture, suggested.as_deref())
    });
}
