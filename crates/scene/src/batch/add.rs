//! `convertToExcalidrawElements` for napkin's subset of `ExcalidrawElementSkeleton`
//! (`packages/element/src/transform.ts` at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`): rectangles, diamonds, ellipses, text, lines,
//! arrows and freedraw, with labels (`bindTextToContainer`) and arrow bindings
//! (`bindLinearElementToElement`). Differences from the JS this ports are the AI interface
//! design's, not bugs: shapes must always give `width`/`height` (napkin never grows a
//! container to fit a label, see [`bind_label`]), line/arrow/freedraw `points` are normalized
//! so the first point is `[0, 0]` (napkin's editing code assumes that), and a line or
//! freedraw's stored `width`/`height` always comes from its points rather than being kept
//! separately.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Value};

use crate::binding::bind_arrow;
use crate::editor::ItemStyle;
use crate::element::{Element, LinearEnd};
use crate::env::Env;
use crate::file::SceneFile;
use crate::fractional_index::sync_moved_indices;
use crate::geometry::size_from_points;
use crate::json::Slot;
use crate::new_element::{
    ElementProps, GenericKind, TextProps, bump_version, new_arrow_element, new_freedraw_element,
    new_generic_element, new_line_element, new_text_element,
};
use crate::text::TextMeasure;
use crate::transform::{bound_text_max_size, bound_text_position};

use super::OpError;
use super::validate;

/// The default a line or arrow's `width`/`height` (and, when `points` is absent, its synthetic
/// two-point shape) take when the skeleton gives neither (`DEFAULT_LINEAR_ELEMENT_PROPS`).
const DEFAULT_LINEAR_WIDTH: f64 = 100.0;
const DEFAULT_LINEAR_HEIGHT: f64 = 0.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Added {
    /// Skeleton `id` -> generated id, for every skeleton that gave one.
    pub aliases: BTreeMap<String, String>,
    /// Generated ids of the skeletons, in input order (labels not included).
    pub ids: Vec<String>,
    /// Labels that do not fit their container.
    pub warnings: Vec<String>,
}

/// A skeleton's `label`, or `update`'s `text` on a container without one.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelSpec {
    pub text: String,
    pub font_size: Option<f64>,
    pub font_family: Option<f64>,
    pub text_align: Option<String>,
    pub vertical_align: Option<String>,
    /// Overrides `bind_label`'s default stroke color (the container's own): `None` for the AI
    /// batch interface, which always takes the container's color. The editor's own text tool
    /// sets this to `currentItemStrokeColor` (`startTextEditing`'s field assembly gives a
    /// freshly bound label the current item style's stroke color, not the container's, except
    /// for a sticky note's label, out of scope here).
    pub stroke_color: Option<String>,
    /// Overrides `bind_label`'s default opacity (`ElementProps::default()`, 100): `None` for
    /// the AI batch interface. The editor sets this to `currentItemOpacity`.
    pub opacity: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct StyleOverrides {
    stroke_color: Option<String>,
    background_color: Option<String>,
    fill_style: Option<String>,
    stroke_width: Option<f64>,
    stroke_style: Option<String>,
    roughness: Option<f64>,
    opacity: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct RawEnd {
    id: String,
}

#[derive(Clone, Debug, PartialEq)]
struct TextFields {
    text: String,
    font_size: Option<f64>,
    font_family: Option<f64>,
    text_align: Option<String>,
}

/// A skeleton op after field-level validation: every field this op's `kind` can use, `None`
/// for the ones it did not give. Fields other kinds use stay `None`.
#[derive(Clone, Debug, PartialEq)]
struct ParsedOp {
    kind: &'static str,
    id: Option<String>,
    x: f64,
    y: f64,
    group_ids: Vec<String>,
    style: StyleOverrides,
    width: Option<f64>,
    height: Option<f64>,
    label: Option<LabelSpec>,
    text: Option<TextFields>,
    points: Option<Vec<[f64; 2]>>,
    start: Option<RawEnd>,
    end: Option<RawEnd>,
    /// `None` when the op did not give this field at all, so the arrow falls back to
    /// `ItemStyle`'s default; `Some(None)` for an explicit JSON `null`, meaning no arrowhead
    /// (JS allows this to turn off the default); `Some(Some(kind))` for a named arrowhead.
    start_arrowhead: Option<Option<String>>,
    end_arrowhead: Option<Option<String>>,
}

const SUPPORTED_TYPES: &[&str] = &[
    "rectangle",
    "diamond",
    "ellipse",
    "text",
    "line",
    "arrow",
    "freedraw",
];
const LABEL_KEYS: &[&str] = &[
    "text",
    "fontSize",
    "fontFamily",
    "textAlign",
    "verticalAlign",
];

fn op_err(pos: usize, field: Option<&str>, message: impl Into<String>) -> OpError {
    OpError {
        op: Some(pos),
        field: field.map(str::to_owned),
        message: message.into(),
    }
}

fn field_path(name: &str, index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{name}[{i}]"),
        None => name.to_owned(),
    }
}

fn parse_style(pos: usize, obj: &Map<String, Value>) -> Result<StyleOverrides, OpError> {
    let mut style = StyleOverrides::default();
    if let Some(v) = obj.get("strokeColor") {
        style.stroke_color =
            Some(validate::string(v).map_err(|m| op_err(pos, Some("strokeColor"), m))?);
    }
    if let Some(v) = obj.get("backgroundColor") {
        style.background_color =
            Some(validate::string(v).map_err(|m| op_err(pos, Some("backgroundColor"), m))?);
    }
    if let Some(v) = obj.get("fillStyle") {
        style.fill_style = Some(
            validate::one_of(v, validate::FILL_STYLES)
                .map_err(|m| op_err(pos, Some("fillStyle"), m))?,
        );
    }
    if let Some(v) = obj.get("strokeWidth") {
        style.stroke_width =
            Some(validate::finite(v).map_err(|m| op_err(pos, Some("strokeWidth"), m))?);
    }
    if let Some(v) = obj.get("strokeStyle") {
        style.stroke_style = Some(
            validate::one_of(v, validate::STROKE_STYLES)
                .map_err(|m| op_err(pos, Some("strokeStyle"), m))?,
        );
    }
    if let Some(v) = obj.get("roughness") {
        style.roughness =
            Some(validate::roughness(v).map_err(|m| op_err(pos, Some("roughness"), m))?);
    }
    if let Some(v) = obj.get("opacity") {
        style.opacity = Some(validate::opacity(v).map_err(|m| op_err(pos, Some("opacity"), m))?);
    }
    Ok(style)
}

fn parse_label(pos: usize, value: &Value) -> Result<LabelSpec, OpError> {
    let obj = value
        .as_object()
        .ok_or_else(|| op_err(pos, Some("label"), "must be an object"))?;
    for key in obj.keys() {
        if !LABEL_KEYS.contains(&key.as_str()) {
            return Err(op_err(pos, Some(&format!("label.{key}")), "unknown field"));
        }
    }
    let text = obj
        .get("text")
        .ok_or_else(|| op_err(pos, Some("label.text"), "must be a non-empty string"))
        .and_then(|v| {
            validate::non_empty_string(v).map_err(|m| op_err(pos, Some("label.text"), m))
        })?;
    let font_size = match obj.get("fontSize") {
        Some(v) => {
            Some(validate::font_size(v).map_err(|m| op_err(pos, Some("label.fontSize"), m))?)
        }
        None => None,
    };
    let font_family = match obj.get("fontFamily") {
        Some(v) => {
            Some(validate::font_family(v).map_err(|m| op_err(pos, Some("label.fontFamily"), m))?)
        }
        None => None,
    };
    let text_align = match obj.get("textAlign") {
        Some(v) => Some(
            validate::one_of(v, validate::TEXT_ALIGNS)
                .map_err(|m| op_err(pos, Some("label.textAlign"), m))?,
        ),
        None => None,
    };
    let vertical_align = match obj.get("verticalAlign") {
        Some(v) => Some(
            validate::one_of(v, validate::VERTICAL_ALIGNS)
                .map_err(|m| op_err(pos, Some("label.verticalAlign"), m))?,
        ),
        None => None,
    };
    Ok(LabelSpec {
        text,
        font_size,
        font_family,
        text_align,
        vertical_align,
        stroke_color: None,
        opacity: None,
    })
}

fn parse_end(pos: usize, field: &str, value: &Value) -> Result<RawEnd, OpError> {
    let bad = || {
        op_err(
            pos,
            Some(field),
            "start/end can only name an existing element by id; add the shape first",
        )
    };
    let obj = value.as_object().ok_or_else(bad)?;
    if obj.len() != 1 {
        return Err(bad());
    }
    let id = obj.get("id").ok_or_else(bad)?;
    let id = validate::non_empty_string(id).map_err(|_| bad())?;
    Ok(RawEnd { id })
}

/// Field-level validation of one skeleton op (`pos` is its position in the batch, for error
/// reporting): shape and key-set checks that need no other op. Cross-op checks (duplicate
/// `id`s, `start`/`end` resolution) run separately once every op has parsed.
fn parse_op(pos: usize, value: &Value) -> Result<ParsedOp, OpError> {
    let obj = value
        .as_object()
        .ok_or_else(|| op_err(pos, None, "must be an object"))?;

    let kind = match obj.get("type").and_then(Value::as_str) {
        Some(k) if SUPPORTED_TYPES.contains(&k) => SUPPORTED_TYPES
            .iter()
            .find(|&&s| s == k)
            .expect("checked above"),
        Some(other) => {
            return Err(op_err(
                pos,
                Some("type"),
                format!(
                    "unsupported type \"{other}\"; napkin can add rectangle, diamond, ellipse, \
                     text, line, arrow and freedraw"
                ),
            ));
        }
        None => return Err(op_err(pos, Some("type"), "must be a string")),
    };

    let mut allowed: Vec<&str> = vec!["op", "type", "id", "x", "y", "groupIds"];
    allowed.extend_from_slice(validate::STYLE_KEYS);
    match *kind {
        "rectangle" | "diamond" | "ellipse" => allowed.extend(["width", "height", "label"]),
        "text" => allowed.extend(["text", "fontSize", "fontFamily", "textAlign"]),
        "line" => allowed.extend(["points", "width", "height"]),
        "arrow" => allowed.extend([
            "points",
            "width",
            "height",
            "start",
            "end",
            "startArrowhead",
            "endArrowhead",
        ]),
        "freedraw" => allowed.push("points"),
        _ => unreachable!("kind checked against SUPPORTED_TYPES above"),
    }
    for key in obj.keys() {
        if key == "label" && matches!(*kind, "line" | "arrow") {
            return Err(op_err(
                pos,
                Some("label"),
                "napkin does not create labels on lines or arrows",
            ));
        }
        if !allowed.contains(&key.as_str()) {
            return Err(op_err(pos, Some(key), "unknown field"));
        }
    }

    let id = match obj.get("id") {
        Some(v) => Some(validate::string(v).map_err(|m| op_err(pos, Some("id"), m))?),
        None => None,
    };
    let x = validate::finite(obj.get("x").unwrap_or(&Value::Null))
        .map_err(|m| op_err(pos, Some("x"), m))?;
    let y = validate::finite(obj.get("y").unwrap_or(&Value::Null))
        .map_err(|m| op_err(pos, Some("y"), m))?;
    let group_ids = match obj.get("groupIds") {
        Some(v) => validate::group_ids(v).map_err(|m| op_err(pos, Some("groupIds"), m))?,
        None => Vec::new(),
    };
    let style = parse_style(pos, obj)?;

    let mut parsed = ParsedOp {
        kind,
        id,
        x,
        y,
        group_ids,
        style,
        width: None,
        height: None,
        label: None,
        text: None,
        points: None,
        start: None,
        end: None,
        start_arrowhead: None,
        end_arrowhead: None,
    };

    match *kind {
        "rectangle" | "diamond" | "ellipse" => {
            parsed.width = Some(
                validate::positive(obj.get("width").unwrap_or(&Value::Null))
                    .map_err(|m| op_err(pos, Some("width"), m))?,
            );
            parsed.height = Some(
                validate::positive(obj.get("height").unwrap_or(&Value::Null))
                    .map_err(|m| op_err(pos, Some("height"), m))?,
            );
            if let Some(label) = obj.get("label") {
                parsed.label = Some(parse_label(pos, label)?);
            }
        }
        "text" => {
            let text = obj
                .get("text")
                .ok_or_else(|| op_err(pos, Some("text"), "must be a string"))
                .and_then(|v| validate::string(v).map_err(|m| op_err(pos, Some("text"), m)))?;
            let font_size = match obj.get("fontSize") {
                Some(v) => {
                    Some(validate::font_size(v).map_err(|m| op_err(pos, Some("fontSize"), m))?)
                }
                None => None,
            };
            let font_family = match obj.get("fontFamily") {
                Some(v) => {
                    Some(validate::font_family(v).map_err(|m| op_err(pos, Some("fontFamily"), m))?)
                }
                None => None,
            };
            let text_align = match obj.get("textAlign") {
                Some(v) => Some(
                    validate::one_of(v, validate::TEXT_ALIGNS)
                        .map_err(|m| op_err(pos, Some("textAlign"), m))?,
                ),
                None => None,
            };
            parsed.text = Some(TextFields {
                text,
                font_size,
                font_family,
                text_align,
            });
        }
        "line" => {
            if let Some(v) = obj.get("points") {
                parsed.points = Some(
                    validate::points(v, 2)
                        .map_err(|(i, m)| op_err(pos, Some(&field_path("points", i)), m))?,
                );
            }
            parsed.width = match obj.get("width") {
                Some(v) => Some(validate::finite(v).map_err(|m| op_err(pos, Some("width"), m))?),
                None => None,
            };
            parsed.height = match obj.get("height") {
                Some(v) => Some(validate::finite(v).map_err(|m| op_err(pos, Some("height"), m))?),
                None => None,
            };
        }
        "arrow" => {
            if let Some(v) = obj.get("points") {
                parsed.points = Some(
                    validate::points(v, 2)
                        .map_err(|(i, m)| op_err(pos, Some(&field_path("points", i)), m))?,
                );
            }
            parsed.width = match obj.get("width") {
                Some(v) => Some(validate::finite(v).map_err(|m| op_err(pos, Some("width"), m))?),
                None => None,
            };
            parsed.height = match obj.get("height") {
                Some(v) => Some(validate::finite(v).map_err(|m| op_err(pos, Some("height"), m))?),
                None => None,
            };
            if let Some(v) = obj.get("start") {
                parsed.start = Some(parse_end(pos, "start", v)?);
            }
            if let Some(v) = obj.get("end") {
                parsed.end = Some(parse_end(pos, "end", v)?);
            }
            if let Some(v) = obj.get("startArrowhead") {
                parsed.start_arrowhead = Some(if v.is_null() {
                    None
                } else {
                    Some(
                        validate::one_of(v, validate::ARROWHEADS)
                            .map_err(|m| op_err(pos, Some("startArrowhead"), m))?,
                    )
                });
            }
            if let Some(v) = obj.get("endArrowhead") {
                parsed.end_arrowhead = Some(if v.is_null() {
                    None
                } else {
                    Some(
                        validate::one_of(v, validate::ARROWHEADS)
                            .map_err(|m| op_err(pos, Some("endArrowhead"), m))?,
                    )
                });
            }
        }
        "freedraw" => {
            let v = obj
                .get("points")
                .ok_or_else(|| op_err(pos, Some("points"), "must be an array of [x, y] points"))?;
            parsed.points = Some(
                validate::points(v, 1)
                    .map_err(|(i, m)| op_err(pos, Some(&field_path("points", i)), m))?,
            );
        }
        _ => unreachable!("kind checked against SUPPORTED_TYPES above"),
    }

    Ok(parsed)
}

/// Whether `id` names a rectangle/diamond/ellipse this batch will create, or one already in
/// `file`; `Err` names the problem, for the caller to attach a position and field to.
fn validate_bind_target(
    id: &str,
    alias_to_index: &HashMap<String, usize>,
    parsed: &[Option<ParsedOp>],
    file: &SceneFile,
) -> Result<(), String> {
    if let Some(&i) = alias_to_index.get(id) {
        let kind = parsed[i]
            .as_ref()
            .expect("an alias is only recorded for an op that parsed")
            .kind;
        return if matches!(kind, "rectangle" | "diamond" | "ellipse") {
            Ok(())
        } else {
            Err("arrows can only bind to rectangles, diamonds and ellipses".to_owned())
        };
    }
    match file
        .elements
        .iter()
        .find(|e| !e.is_deleted() && e.id() == Some(id))
    {
        Some(Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)) => Ok(()),
        Some(_) => Err("arrows can only bind to rectangles, diamonds and ellipses".to_owned()),
        None => Err(format!("no element \"{id}\"")),
    }
}

/// [`validate_bind_target`]'s target, resolved to its actual position in `file.elements` once
/// every op has been created (`op_positions[i]` is where the batch's op `i` landed).
fn resolve_bind_target(
    id: &str,
    alias_to_index: &HashMap<String, usize>,
    op_positions: &[usize],
    file: &SceneFile,
) -> usize {
    if let Some(&i) = alias_to_index.get(id) {
        return op_positions[i];
    }
    file.elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(id))
        .expect("validate_bind_target already confirmed this id exists")
}

fn apply_overrides(props: &mut ElementProps, p: &ParsedOp) {
    if let Some(v) = &p.style.stroke_color {
        props.stroke_color = v.clone();
    }
    if let Some(v) = &p.style.background_color {
        props.background_color = v.clone();
    }
    if let Some(v) = &p.style.fill_style {
        props.fill_style = v.clone();
    }
    if let Some(v) = p.style.stroke_width {
        props.stroke_width = v;
    }
    if let Some(v) = &p.style.stroke_style {
        props.stroke_style = v.clone();
    }
    if let Some(v) = p.style.roughness {
        props.roughness = v;
    }
    if let Some(v) = p.style.opacity {
        props.opacity = v;
    }
    props.group_ids = p.group_ids.clone();
}

/// The points a line or arrow op ends up with: normalized `points` when given, or a synthetic
/// two-point line from `width`/`height` (present, including `0`, or `DEFAULT_LINEAR_WIDTH`/
/// `DEFAULT_LINEAR_HEIGHT` when neither is given) otherwise.
fn line_or_arrow_points(p: &ParsedOp) -> ([f64; 2], Vec<[f64; 2]>) {
    match &p.points {
        Some(points) => normalize_points([p.x, p.y], points),
        None => {
            let w = p.width.unwrap_or(DEFAULT_LINEAR_WIDTH);
            let h = p.height.unwrap_or(DEFAULT_LINEAR_HEIGHT);
            ([p.x, p.y], vec![[0.0, 0.0], [w, h]])
        }
    }
}

/// A line's points, plus its stored `width`/`height`. Unlike an arrow (which always stores
/// `size_from_points` of its final points, see [`create_element`]'s arrow branch),
/// `convertToExcalidrawElements` never derives a line's stored width/height from its points at
/// all: given `points` with no `width`/`height`, they stay at the `DEFAULT_LINEAR_WIDTH`/
/// `DEFAULT_LINEAR_HEIGHT` default, completely disconnected from the actual shape. napkin's own
/// selection and bounding box need accurate numbers, so it derives them from the points instead,
/// except in the one case that would throw away information `size_from_points`'s bounding box
/// cannot represent: a synthetic two-point line from a *given* negative `width` or `height`
/// keeps that literal value (sign included) rather than its absolute size.
fn line_points_and_size(p: &ParsedOp) -> ([f64; 2], Vec<[f64; 2]>, f64, f64) {
    match &p.points {
        Some(points) => {
            let (origin, points) = normalize_points([p.x, p.y], points);
            let [width, height] = size_from_points(&points);
            (origin, points, width, height)
        }
        None => {
            let w = p.width.unwrap_or(DEFAULT_LINEAR_WIDTH);
            let h = p.height.unwrap_or(DEFAULT_LINEAR_HEIGHT);
            ([p.x, p.y], vec![[0.0, 0.0], [w, h]], w, h)
        }
    }
}

fn create_element(
    p: &ParsedOp,
    style: &ItemStyle,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Element {
    match p.kind {
        "rectangle" | "diamond" | "ellipse" => {
            let kind = match p.kind {
                "rectangle" => GenericKind::Rectangle,
                "diamond" => GenericKind::Diamond,
                _ => GenericKind::Ellipse,
            };
            let width = p.width.expect("validated for a shape");
            let height = p.height.expect("validated for a shape");
            let mut props = style.props(
                [p.x, p.y],
                width,
                height,
                style.generic_roundness(kind),
                style.stroke_width.value(false),
            );
            apply_overrides(&mut props, p);
            new_generic_element(kind, props, env)
        }
        "text" => {
            let text = p.text.as_ref().expect("validated for a text op");
            let mut props =
                style.props([p.x, p.y], 0.0, 0.0, None, style.stroke_width.value(false));
            apply_overrides(&mut props, p);
            new_text_element(
                props,
                TextProps {
                    text: text.text.clone(),
                    font_size: text.font_size,
                    font_family: text.font_family,
                    text_align: text.text_align.clone(),
                    vertical_align: None,
                    container_id: None,
                    line_height: None,
                },
                measure,
                env,
            )
        }
        "line" => {
            let (origin, points, width, height) = line_points_and_size(p);
            let mut props = style.props(
                origin,
                width,
                height,
                style.line_roundness(),
                style.stroke_width.value(false),
            );
            apply_overrides(&mut props, p);
            new_line_element(props, points, env)
        }
        "arrow" => {
            let (origin, points) = line_or_arrow_points(p);
            let [width, height] = size_from_points(&points);
            let mut props = style.props(
                origin,
                width,
                height,
                style.arrow_roundness(),
                style.stroke_width.value(false),
            );
            apply_overrides(&mut props, p);
            let start_arrowhead = p
                .start_arrowhead
                .clone()
                .unwrap_or_else(|| style.start_arrowhead.clone());
            let end_arrowhead = p
                .end_arrowhead
                .clone()
                .unwrap_or_else(|| style.end_arrowhead.clone());
            new_arrow_element(props, points, start_arrowhead, end_arrowhead, env)
        }
        "freedraw" => {
            let (origin, points) = normalize_points(
                [p.x, p.y],
                p.points.as_deref().expect("validated for freedraw"),
            );
            let [width, height] = size_from_points(&points);
            let mut props =
                style.props(origin, width, height, None, style.stroke_width.value(true));
            apply_overrides(&mut props, p);
            new_freedraw_element(
                props,
                points,
                Vec::new(),
                true,
                Some(crate::element::StrokeOptions {
                    variability: Slot::Value(style.stroke_variability.clone()),
                    streamline: Slot::Value(0.5),
                    extra: Map::new(),
                }),
                env,
            )
        }
        _ => unreachable!("kind checked against SUPPORTED_TYPES in parse_op"),
    }
}

/// `bindLinearElementToElement`'s tail, run on every arrow regardless of whether it ended up
/// bound: shifts the first and last point by `0.5` away from each other along whichever axes
/// moved between them (so the bound edges do not sit exactly on the binding target's outline),
/// then re-normalizes so the first point is `[0, 0]` again. A no-op for anything but an arrow,
/// or an arrow with fewer than 2 points.
fn apply_binding_point_shift(element: &mut Element) {
    let Element::Arrow(l) = element else {
        return;
    };
    if l.points.len() < 2 {
        return;
    }
    let last = l.points.len() - 1;
    let mut shifted = l.points.clone();
    const DELTA: f64 = 0.5;
    if l.points[last][0] > l.points[last - 1][0] {
        shifted[0][0] = DELTA;
        shifted[last][0] -= DELTA;
    }
    if l.points[last][0] < l.points[last - 1][0] {
        shifted[0][0] = -DELTA;
        shifted[last][0] += DELTA;
    }
    if l.points[last][1] > l.points[last - 1][1] {
        shifted[0][1] = DELTA;
        shifted[last][1] -= DELTA;
    }
    if l.points[last][1] < l.points[last - 1][1] {
        shifted[0][1] = -DELTA;
        shifted[last][1] += DELTA;
    }
    let (origin, points) = normalize_points([l.base.x, l.base.y], &shifted);
    l.base.x = origin[0];
    l.base.y = origin[1];
    l.points = points;
}

/// `convertToExcalidrawElements` for napkin's subset, appended to `file`. `skeletons` pairs
/// each skeleton with its op position, for error positions. Validates everything first and
/// changes nothing on error.
pub fn add_elements(
    file: &mut SceneFile,
    skeletons: &[(usize, &Value)],
    style: &ItemStyle,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<Added, Vec<OpError>> {
    let mut errors: Vec<OpError> = Vec::new();
    let mut parsed: Vec<Option<ParsedOp>> = Vec::with_capacity(skeletons.len());
    for &(pos, value) in skeletons {
        match parse_op(pos, value) {
            Ok(p) => parsed.push(Some(p)),
            Err(e) => {
                errors.push(e);
                parsed.push(None);
            }
        }
    }

    let mut alias_to_index: HashMap<String, usize> = HashMap::new();
    for (i, p) in parsed.iter().enumerate() {
        let Some(p) = p else { continue };
        let Some(id) = &p.id else { continue };
        if alias_to_index.contains_key(id) {
            errors.push(op_err(
                skeletons[i].0,
                Some("id"),
                format!("duplicate id \"{id}\""),
            ));
            continue;
        }
        alias_to_index.insert(id.clone(), i);
    }

    for (i, p) in parsed.iter().enumerate() {
        let Some(p) = p else { continue };
        if p.kind != "arrow" {
            continue;
        }
        if let Some(start) = &p.start
            && let Err(message) = validate_bind_target(&start.id, &alias_to_index, &parsed, file)
        {
            errors.push(op_err(skeletons[i].0, Some("start"), message));
        }
        if let Some(end) = &p.end
            && let Err(message) = validate_bind_target(&end.id, &alias_to_index, &parsed, file)
        {
            errors.push(op_err(skeletons[i].0, Some("end"), message));
        }
    }

    if !errors.is_empty() {
        errors.sort_by_key(|e| e.op);
        return Err(errors);
    }

    let mut op_positions: Vec<usize> = Vec::with_capacity(parsed.len());
    let mut ids: Vec<String> = Vec::with_capacity(parsed.len());
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    let mut moved: HashSet<String> = HashSet::new();

    for p in &parsed {
        let p = p.as_ref().expect("validated above");
        let element = create_element(p, style, measure, env);
        let id = element
            .id()
            .expect("a freshly created element has an id")
            .to_owned();
        moved.insert(id.clone());
        if let Some(alias) = &p.id {
            aliases.insert(alias.clone(), id.clone());
        }
        ids.push(id);
        file.elements.push(element);
        op_positions.push(file.elements.len() - 1);
    }

    let mut warnings: Vec<String> = Vec::new();
    for (i, p) in parsed.iter().enumerate() {
        let p = p.as_ref().expect("validated above");
        let position = op_positions[i];
        if let Some(label) = &p.label {
            let name = p.id.as_deref().unwrap_or(ids[i].as_str());
            let label_position =
                bind_label(file, position, label, name, measure, env, &mut warnings);
            moved.insert(
                file.elements[label_position]
                    .id()
                    .expect("label has an id")
                    .to_owned(),
            );
        }
        if p.kind == "arrow" {
            if let Some(start) = &p.start {
                let target = resolve_bind_target(&start.id, &alias_to_index, &op_positions, file);
                bind_arrow(file, position, LinearEnd::Start, target, env);
            }
            if let Some(end) = &p.end {
                let target = resolve_bind_target(&end.id, &alias_to_index, &op_positions, file);
                bind_arrow(file, position, LinearEnd::End, target, env);
            }
            apply_binding_point_shift(&mut file.elements[position]);
        }
    }

    sync_moved_indices(&mut file.elements, &moved, env);

    Ok(Added {
        aliases,
        ids,
        warnings,
    })
}

/// `bindTextToContainer` + `redrawTextBoundingBox` without wrapping or growing the container:
/// appends a label to the container at `container`, returns its position, and pushes a warning
/// when it does not fit (`name` is how the warning refers to the container). The label's
/// stroke color and opacity default to the container's own color and `ElementProps::default()`'s
/// opacity (the AI batch interface's behavior); `label.stroke_color`/`label.opacity` override
/// either, for the editor's own text tool. Does not bump `container`'s own version even though
/// it gains a `boundElements` entry: the AI batch interface's `add_elements` always calls this
/// on a container it just created in the same batch (version 1 throughout, matching the JS
/// baseline's `Object.assign`), and `batch::apply_update`'s own container-vs-`container_before`
/// diff already bumps it for a pre-existing one; a caller binding to a pre-existing container
/// outside those two paths (the editor's `text::create_new_label`) must bump it itself.
pub(crate) fn bind_label(
    file: &mut SceneFile,
    container: usize,
    label: &LabelSpec,
    name: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
    warnings: &mut Vec<String>,
) -> usize {
    let container_before = file.elements[container].clone();
    let stroke_color = label.stroke_color.clone().unwrap_or_else(|| {
        container_before
            .base()
            .expect("bind_label's container is always a typed shape")
            .stroke_color
            .clone()
    });
    let angle = container_before.placement().map_or(0.0, |p| p.angle);
    let container_id = container_before
        .id()
        .expect("bind_label's container has an id")
        .to_owned();

    let props = ElementProps {
        stroke_color,
        opacity: label
            .opacity
            .unwrap_or_else(|| ElementProps::default().opacity),
        ..ElementProps::default()
    };
    let mut text_element = new_text_element(
        props,
        TextProps {
            text: label.text.clone(),
            font_size: label.font_size,
            font_family: label.font_family,
            text_align: Some(
                label
                    .text_align
                    .clone()
                    .unwrap_or_else(|| "center".to_owned()),
            ),
            vertical_align: Some(
                label
                    .vertical_align
                    .clone()
                    .unwrap_or_else(|| "middle".to_owned()),
            ),
            container_id: Some(container_id),
            line_height: None,
        },
        measure,
        env,
    );
    if let Element::Text(t) = &mut text_element {
        t.base.angle = angle;
    }

    file.elements.push(text_element);
    let label_position = file.elements.len() - 1;
    let label_id = file.elements[label_position]
        .id()
        .expect("a freshly created text element has an id")
        .to_owned();

    file.elements[container].add_bound_element(&label_id, "text");

    let container_after = file.elements[container].clone();
    let new_position = match &file.elements[label_position] {
        Element::Text(t) => bound_text_position(&container_after, t),
        _ => None,
    };
    if let Some([x, y]) = new_position
        && let Element::Text(t) = &mut file.elements[label_position]
        && (t.base.x != x || t.base.y != y)
    {
        t.base.x = x;
        t.base.y = y;
        bump_version(&mut file.elements[label_position], env);
    }

    warn_if_label_overflows(
        &container_after,
        &file.elements[label_position],
        name,
        warnings,
    );

    label_position
}

/// Pushes a warning to `warnings` when `label`'s width or height exceeds what `container`
/// fits (`name` is how the warning refers to `container`); a no-op when it fits, when
/// `label` is not a text element, or for a container type [`bound_text_max_size`] has no
/// limit for.
pub(crate) fn warn_if_label_overflows(
    container: &Element,
    label: &Element,
    name: &str,
    warnings: &mut Vec<String>,
) {
    let Element::Text(t) = label else { return };
    let Some([max_w, max_h]) = bound_text_max_size(container) else {
        return;
    };
    if t.base.width > max_w || t.base.height > max_h {
        let kind = container.kind();
        let fmt = |v: f64| format!("{v:.0}");
        warnings.push(format!(
            "{name}: label needs {}x{} but the {kind} fits {}x{}; make the {kind} larger or \
             add line breaks",
            fmt(t.base.width),
            fmt(t.base.height),
            fmt(max_w),
            fmt(max_h)
        ));
    }
}

/// First point moved to `[0, 0]`, the offset added to the origin (`getNormalizedPoints`).
pub(crate) fn normalize_points(origin: [f64; 2], points: &[[f64; 2]]) -> ([f64; 2], Vec<[f64; 2]>) {
    match points.first() {
        Some(&[ox, oy]) => {
            let new_origin = [origin[0] + ox, origin[1] + oy];
            let new_points = points.iter().map(|p| [p[0] - ox, p[1] - oy]).collect();
            (new_origin, new_points)
        }
        None => (origin, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::editor::ItemStyle;
    use crate::env::Env;
    use crate::sample::{self, CharWidthMeasure};

    /// A counter written into every random byte, so each created element gets a distinct id
    /// (unlike a byte-constant `Env`, which would hand every element in a batch the same id).
    struct FixedEnv {
        next: u8,
    }

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(self.next);
            self.next = self.next.wrapping_add(1);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    fn env() -> FixedEnv {
        FixedEnv { next: 0 }
    }

    fn pairs(ops: &[Value]) -> Vec<(usize, &Value)> {
        ops.iter().enumerate().collect()
    }

    #[test]
    fn rejects_the_whole_list_and_leaves_the_file_alone() {
        let mut file = sample::file(vec![sample::generic(
            "rectangle",
            "keep",
            [0.0, 0.0, 10.0, 10.0],
        )]);
        let before = file.clone();
        let ops = [
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}),
            json!({"type": "image", "x": 0, "y": 0}),
            json!({"type": "arrow", "x": 0, "y": 0, "label": {"text": "no"}}),
            json!({"type": "arrow", "x": 0, "y": 0, "end": {"type": "rectangle"}}),
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 0, "height": 10}),
            json!({"type": "arrow", "x": 0, "y": 0, "start": {"id": "missing"}}),
        ];
        let ops = pairs(&ops);
        let errors = add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .unwrap_err();
        let positions: Vec<Option<usize>> = errors.iter().map(|e| e.op).collect();
        assert_eq!(positions, vec![Some(1), Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(errors[3].field.as_deref(), Some("width"));
        assert_eq!(file, before);
    }

    #[test]
    fn font_size_must_be_positive_and_not_absurdly_large() {
        let mut file = SceneFile::new();
        let ops = [
            json!({"type": "text", "x": 0, "y": 0, "text": "a", "fontSize": 0}),
            json!({"type": "text", "x": 0, "y": 0, "text": "a", "fontSize": -20}),
            json!({"type": "text", "x": 0, "y": 0, "text": "a", "fontSize": 1001}),
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
                   "label": {"text": "a", "fontSize": 0}}),
            json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
                   "label": {"text": "a", "fontSize": 1001}}),
        ];
        let ops = pairs(&ops);
        let errors = add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .unwrap_err();
        assert_eq!(errors.len(), 5);
        assert_eq!(errors[0].field.as_deref(), Some("fontSize"));
        assert_eq!(errors[3].field.as_deref(), Some("label.fontSize"));

        let ops = [json!({"type": "text", "x": 0, "y": 0, "text": "a", "fontSize": 1000})];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("1000 is still within the cap");
    }

    #[test]
    fn aliases_and_ids_track_batch_order() {
        let mut file = SceneFile::new();
        let ops = [
            json!({"type": "rectangle", "id": "r", "x": 0, "y": 0, "width": 10, "height": 10}),
            json!({"type": "ellipse", "x": 20, "y": 0, "width": 10, "height": 10}),
        ];
        let ops = pairs(&ops);
        let added = add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        assert_eq!(added.ids.len(), 2);
        assert_eq!(added.aliases.get("r"), Some(&added.ids[0]));
        assert_eq!(added.warnings, Vec::<String>::new());
    }

    #[test]
    fn arrow_binds_to_a_shape_defined_later_in_the_batch() {
        let mut file = SceneFile::new();
        let ops = [
            json!({"type": "arrow", "x": 60, "y": 5, "points": [[0, 0], [40, 0]], "end": {"id": "r"}}),
            json!({"type": "rectangle", "id": "r", "x": 100, "y": -25, "width": 50, "height": 50}),
        ];
        let ops = pairs(&ops);
        let added = add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("forward reference resolves");
        let rect_id = &added.ids[1];
        let rect = file
            .elements
            .iter()
            .find(|e| e.id() == Some(rect_id.as_str()))
            .expect("rectangle exists");
        assert_eq!(
            rect.bound_elements(),
            vec![(added.ids[0].as_str(), "arrow")]
        );
    }

    #[test]
    fn binding_to_an_existing_element_bumps_its_version() {
        let mut file = sample::file(vec![sample::generic(
            "rectangle",
            "r1",
            [200.0, 0.0, 50.0, 50.0],
        )]);
        let version_before = file.elements[0].version();
        let ops = [json!({
            "type": "arrow", "x": 100, "y": 25, "points": [[0, 0], [90, 0]], "end": {"id": "r1"}
        })];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        let rect = file
            .elements
            .iter()
            .find(|e| e.id() == Some("r1"))
            .expect("rectangle exists");
        assert!(rect.version() > version_before);
        assert_eq!(rect.bound_elements().len(), 1);
    }

    #[test]
    fn binding_to_a_raw_or_text_element_errors() {
        let mut file = sample::file(vec![
            json!({"id": "img", "type": "image", "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0}),
            sample::text("t1", [0.0, 0.0, 10.0, 10.0], "hi", None),
        ]);
        for target in ["img", "t1"] {
            let ops = [json!({
                "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]],
                "end": {"id": target}
            })];
            let ops = pairs(&ops);
            let errors = add_elements(
                &mut file,
                &ops,
                &ItemStyle::default(),
                &mut CharWidthMeasure,
                &mut env(),
            )
            .unwrap_err();
            assert_eq!(errors.len(), 1, "{target}: {errors:?}");
            assert!(
                errors[0]
                    .message
                    .contains("rectangles, diamonds and ellipses"),
                "{target}: {errors:?}"
            );
        }
    }

    #[test]
    fn arrow_without_points_uses_given_width_and_height_including_zero() {
        let mut file = SceneFile::new();
        let ops = [json!({"type": "arrow", "x": 0, "y": 0, "width": 0, "height": 80})];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        let Element::Arrow(l) = &file.elements[0] else {
            panic!("arrow")
        };
        // The synthetic two-point line runs straight down: [[0,0],[0,80]], before the 0.5
        // binding-point shift moves the endpoints along the axis that changed.
        assert_eq!(l.points[0][0], l.points[1][0]);
        assert_ne!(l.points[0][1], l.points[1][1]);
    }

    #[test]
    fn arrowheads_default_when_omitted_and_turn_off_on_explicit_null() {
        let mut file = SceneFile::new();
        let ops = [
            // Omitted: falls back to `ItemStyle`'s default (start none, end arrow).
            json!({"type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]]}),
            // Explicit null: no arrowhead at that end, overriding the style default.
            json!({"type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]],
                   "startArrowhead": null, "endArrowhead": null}),
            // Explicit kind.
            json!({"type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]],
                   "startArrowhead": "triangle", "endArrowhead": "bar"}),
        ];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");

        let heads: Vec<(Option<&str>, Option<&str>)> = file
            .elements
            .iter()
            .map(|e| match e {
                Element::Arrow(a) => (
                    a.start_arrowhead.value().map(String::as_str),
                    a.end_arrowhead.value().map(String::as_str),
                ),
                _ => panic!("arrow"),
            })
            .collect();
        assert_eq!(
            heads,
            vec![
                (None, Some("arrow")),
                (None, None),
                (Some("triangle"), Some("bar")),
            ]
        );
    }

    #[test]
    fn line_points_are_normalized_to_start_at_the_origin() {
        let mut file = SceneFile::new();
        let ops = [json!({
            "type": "line", "x": 5, "y": 5, "points": [[10, 10], [30, 10]]
        })];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        let Element::Line(l) = &file.elements[0] else {
            panic!("line")
        };
        assert_eq!(l.points, vec![[0.0, 0.0], [20.0, 0.0]]);
        assert_eq!((l.base.x, l.base.y), (15.0, 15.0));
    }

    #[test]
    fn label_that_does_not_fit_warns_without_growing_the_container() {
        let mut file = SceneFile::new();
        let ops = [json!({
            "type": "rectangle", "id": "r", "x": 0, "y": 0, "width": 10, "height": 10,
            "label": {"text": "way too long for this box"}
        })];
        let ops = pairs(&ops);
        let added = add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        assert_eq!(added.warnings.len(), 1, "{:?}", added.warnings);
        assert!(added.warnings[0].starts_with('r'), "{:?}", added.warnings);
        let rect = &file.elements[0];
        assert_eq!(rect.placement().unwrap().width, 10.0);
        assert_eq!(rect.placement().unwrap().height, 10.0);
    }

    #[test]
    fn default_style_rectangle_roundness_is_adaptive() {
        let mut file = SceneFile::new();
        let ops = [json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10})];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        assert_eq!(
            file.elements[0].to_value()["roundness"],
            json!({"type": 3.0})
        );
    }

    #[test]
    fn new_elements_get_index_after_existing_ones() {
        let mut file = sample::file(vec![sample::generic(
            "rectangle",
            "old",
            [0.0, 0.0, 10.0, 10.0],
        )]);
        let old_index = file.elements[0].index().unwrap().to_owned();
        let ops = [json!({"type": "rectangle", "x": 20, "y": 0, "width": 10, "height": 10})];
        let ops = pairs(&ops);
        add_elements(
            &mut file,
            &ops,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut env(),
        )
        .expect("valid batch");
        let new_index = file.elements[1].index().unwrap();
        assert!(new_index > old_index.as_str(), "{new_index} vs {old_index}");
    }
}
