//! One `napkin apply` batch (AI spec §4): parsing, the `add` skeleton conversion, `update` and
//! `delete`, applied all-or-nothing.

// `pub(crate)`, not private: the editor's own text tool (`editor::text`) reuses
// `add::bind_label` to create a container label, the same primitive the AI batch interface
// uses for a skeleton's `label`.
pub(crate) mod add;
mod validate;

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value, json};

use crate::binding::fixed_point_for;
use crate::bound_text::{bound_text_position, handle_bind_text_resize, redraw_text_bounding_box};
use crate::edit::delete_selection;
use crate::editor::ItemStyle;
use crate::element::{Element, LinearEnd};
use crate::env::Env;
use crate::file::SceneFile;
use crate::fractional_index::sync_moved_indices;
use crate::geometry::size_from_points;
use crate::new_element::bump_version;
use crate::selection::Selection;
use crate::text::{TextMeasure, measure_text, normalize_text};

pub use add::{Added, LabelSpec, add_elements};

/// One op's validation failure, positioned within a batch for the client to report.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OpError {
    /// Position of the offending op in `ops`; `None` for an error about the batch as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<usize>,
    /// JSON path inside that op, e.g. `"label.fontSize"` or `"points[2]"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

/// The result of a batch that applied cleanly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchReport {
    /// `add` aliases -> generated ids.
    pub created: BTreeMap<String, String>,
    /// Ids of the elements the `add` ops created, in op order (labels not included).
    pub added: Vec<String>,
    /// Ids named by `update` ops, first mention order, deduplicated.
    pub updated: Vec<String>,
    /// Ids named by `delete` ops, deduplicated.
    pub deleted: Vec<String>,
}

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

/// `batch` as `{"ops": [...]}` with a non-empty array; `None` for anything else (not an
/// object, no `ops`, `ops` not an array, or an empty one), which is a single whole-batch
/// error rather than a per-op one.
fn validate_ops(batch: &Value) -> Option<&Vec<Value>> {
    let ops = batch.as_object()?.get("ops")?.as_array()?;
    if ops.is_empty() { None } else { Some(ops) }
}

/// One op's `"op"` field, checked against the three kinds this batch understands.
fn op_kind(value: &Value) -> Result<&'static str, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "must be an object".to_owned())?;
    match obj.get("op").and_then(Value::as_str) {
        Some("add") => Ok("add"),
        Some("update") => Ok("update"),
        Some("delete") => Ok("delete"),
        Some(other) => Err(format!("unknown op \"{other}\"")),
        None => Err("\"op\" must be one of: add, update, delete".to_owned()),
    }
}

/// Applies `batch` (`{"ops": [...]}`) to a copy of `file`: every `add` first (one
/// `add_elements` call), then `update` and `delete` in op order. Returns the new scene only when
/// every op succeeded; otherwise every error found, and `file` is untouched by construction.
pub fn apply_batch(
    file: &SceneFile,
    batch: &Value,
    style: &ItemStyle,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<(SceneFile, BatchReport), Vec<OpError>> {
    let Some(ops) = validate_ops(batch) else {
        return Err(vec![OpError {
            op: None,
            field: None,
            message: "batch must be {\"ops\": [...]} with at least one op".to_owned(),
        }]);
    };

    let mut errors: Vec<OpError> = Vec::new();
    let mut adds: Vec<(usize, &Value)> = Vec::new();
    let mut rest: Vec<(usize, &'static str, &Value)> = Vec::new();
    for (pos, op_value) in ops.iter().enumerate() {
        match op_kind(op_value) {
            Ok("add") => adds.push((pos, op_value)),
            Ok(kind) => rest.push((pos, kind, op_value)),
            Err(message) => errors.push(op_err(pos, Some("op"), message)),
        }
    }

    let mut next = file.clone();
    let mut report = BatchReport::default();

    match add_elements(&mut next, &adds, style, measure, env) {
        Ok(added) => {
            report.created = added.aliases;
            report.added = added.ids;
        }
        Err(add_errors) => errors.extend(add_errors),
    }

    for (pos, kind, value) in rest {
        let result = match kind {
            "update" => apply_update_op(
                &mut next,
                pos,
                value,
                &report.created,
                measure,
                env,
                &mut report.updated,
            ),
            "delete" => apply_delete_op(
                &mut next,
                pos,
                value,
                &report.created,
                env,
                &mut report.deleted,
            ),
            _ => unreachable!("op_kind only ever classifies add, update or delete"),
        };
        if let Err(e) = result {
            errors.push(e);
        }
    }

    if !errors.is_empty() {
        errors.sort_by_key(|e| e.op);
        return Err(errors);
    }
    Ok((next, report))
}

/// `id` resolved against `created` (batch-local `add` aliases) first, then against `next`'s
/// live elements: the element's position, plus its real (non-alias) id.
fn resolve_target(
    id: &str,
    created: &BTreeMap<String, String>,
    next: &SceneFile,
) -> Result<(usize, String), String> {
    let real_id = created.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let index = next
        .elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(real_id.as_str()))
        .ok_or_else(|| format!("no element \"{id}\""))?;
    Ok((index, real_id))
}

/// Position of a live (non-deleted) element by id, if any.
fn find_alive_by_id(elements: &[Element], id: &str) -> Option<usize> {
    elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(id))
}

fn binding_field_name(end: LinearEnd) -> &'static str {
    match end {
        LinearEnd::Start => "startBinding",
        LinearEnd::End => "endBinding",
    }
}

/// The live text bound to `container_index`, if it has one (`boundElements`'s `"text"`
/// entry, resolved to a non-deleted element).
fn live_label_index(next: &SceneFile, container_index: usize) -> Option<usize> {
    let (text_id, _) = next.elements[container_index]
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")?;
    let text_id = text_id.to_owned();
    find_alive_by_id(&next.elements, &text_id)
}

/// Recomputes and sets `label`'s `text`/`originalText` (normalized first) and remeasures its
/// width/height from its own font; a no-op for anything but a text element. Position is left
/// untouched (the caller repositions separately when the container calls for it).
fn update_label_text_fields(label: &mut Element, text_value: &str, measure: &mut dyn TextMeasure) {
    let Element::Text(t) = label else { return };
    let normalized = normalize_text(text_value);
    let line_height = t
        .line_height
        .unwrap_or_else(|| crate::text::line_height(t.font_family));
    let [width, height] = measure_text(
        &normalized,
        t.font_family,
        t.font_size,
        line_height,
        measure,
    );
    t.text = normalized.clone();
    t.original_text = Some(normalized);
    t.base.width = width;
    t.base.height = height;
}

/// Repositions `label_index`'s text with [`bound_text_position`] against `container_index`'s
/// current state. A no-op when `container_index` is not a rectangle/diamond/ellipse container.
fn reposition_label(next: &mut SceneFile, container_index: usize, label_index: usize) {
    let container = next.elements[container_index].clone();
    if let Element::Text(t) = &next.elements[label_index]
        && let Some([x, y]) = bound_text_position(&container, t)
        && let Element::Text(t) = &mut next.elements[label_index]
    {
        t.base.x = x;
        t.base.y = y;
    }
}

/// Bumps `element` unless it already carries a version newer than `version_before`: the
/// bound-text primitives bump what they change themselves, so an element they touched must
/// not be bumped a second time for the same `update`.
fn bump_if_not_bumped(element: &mut Element, version_before: f64, env: &mut impl Env) {
    if element.version() == version_before {
        bump_version(element, env);
    }
}

/// Recomputes `fixedPoint` for the arrow at `arrow_index`'s binding to `target_index` at `end`
/// (`fixed_point_for`), keeping the binding's `elementId` and `mode` (defaulting to `"orbit"`
/// when absent); bumps the arrow if this changed anything. A no-op when the geometry cannot be
/// read (an untyped or unplaceable arrow or target).
fn refresh_fixed_point(
    next: &mut SceneFile,
    arrow_index: usize,
    end: LinearEnd,
    target_index: usize,
    env: &mut impl Env,
) {
    let target = next.elements[target_index].clone();
    let Some(fixed_point) = fixed_point_for(&next.elements[arrow_index], end, &target) else {
        return;
    };
    let target_id = target.id().expect("bindable target has an id").to_owned();
    let mode = next.elements[arrow_index]
        .to_value()
        .get(binding_field_name(end))
        .and_then(|b| b.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("orbit")
        .to_owned();

    let before = next.elements[arrow_index].clone();
    next.elements[arrow_index].set_binding(
        end,
        json!({"elementId": target_id, "mode": mode, "fixedPoint": fixed_point}),
    );
    if next.elements[arrow_index] != before {
        bump_version(&mut next.elements[arrow_index], env);
    }
}

/// For every live arrow in `container_index`'s `boundElements`, refreshes whichever end(s) are
/// bound to it: a shape's move or resize keeps its bound arrows' `fixedPoint`s current, even
/// though napkin does not make the arrow itself follow the shape.
fn refresh_bound_arrow_fixed_points(
    next: &mut SceneFile,
    container_index: usize,
    env: &mut impl Env,
) {
    let container_id = next.elements[container_index]
        .id()
        .expect("has an id")
        .to_owned();
    let arrow_ids: Vec<String> = next.elements[container_index]
        .bound_elements()
        .into_iter()
        .filter(|&(_, kind)| kind == "arrow")
        .map(|(id, _)| id.to_owned())
        .collect();
    for arrow_id in arrow_ids {
        let Some(arrow_index) = find_alive_by_id(&next.elements, &arrow_id) else {
            continue;
        };
        for end in [LinearEnd::Start, LinearEnd::End] {
            if next.elements[arrow_index].binding_target(end) == Some(container_id.as_str()) {
                refresh_fixed_point(next, arrow_index, end, container_index, env);
            }
        }
    }
}

/// Translates the arrow at `arrow_index`'s own bound label (if it is alive) by `(dx, dy)`,
/// matching the arrow's move (M4a's drag behavior, not a recomputed position: napkin stores an
/// arrow label's position rather than deriving it from the arrow's path).
fn translate_arrow_label(
    next: &mut SceneFile,
    arrow_index: usize,
    dx: f64,
    dy: f64,
    env: &mut impl Env,
) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let Some((label_id, _)) = next.elements[arrow_index]
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")
    else {
        return;
    };
    let label_id = label_id.to_owned();
    let Some(label_index) = find_alive_by_id(&next.elements, &label_id) else {
        return;
    };
    let Some(placement) = next.elements[label_index].placement() else {
        return;
    };
    let before = next.elements[label_index].clone();
    next.elements[label_index].set_position(placement.x + dx, placement.y + dy);
    if next.elements[label_index] != before {
        bump_version(&mut next.elements[label_index], env);
    }
}

/// The style keys every typed element accepts in `update`'s `set`, parsed once and applied to
/// an `ElementBase` (or a no-op on `Raw`, which never reaches this).
#[derive(Clone, Debug, Default, PartialEq)]
struct StyleSet {
    stroke_color: Option<String>,
    background_color: Option<String>,
    fill_style: Option<String>,
    stroke_width: Option<f64>,
    stroke_style: Option<String>,
    roughness: Option<f64>,
    opacity: Option<f64>,
}

impl StyleSet {
    fn parse(&mut self, pos: usize, key: &str, v: &Value) -> Result<(), OpError> {
        let field = format!("set.{key}");
        match key {
            "strokeColor" => {
                self.stroke_color =
                    Some(validate::string(v).map_err(|m| op_err(pos, Some(&field), m))?);
            }
            "backgroundColor" => {
                self.background_color =
                    Some(validate::string(v).map_err(|m| op_err(pos, Some(&field), m))?);
            }
            "fillStyle" => {
                self.fill_style = Some(
                    validate::one_of(v, validate::FILL_STYLES)
                        .map_err(|m| op_err(pos, Some(&field), m))?,
                );
            }
            "strokeWidth" => {
                self.stroke_width =
                    Some(validate::finite(v).map_err(|m| op_err(pos, Some(&field), m))?);
            }
            "strokeStyle" => {
                self.stroke_style = Some(
                    validate::one_of(v, validate::STROKE_STYLES)
                        .map_err(|m| op_err(pos, Some(&field), m))?,
                );
            }
            "roughness" => {
                self.roughness =
                    Some(validate::roughness(v).map_err(|m| op_err(pos, Some(&field), m))?);
            }
            "opacity" => {
                self.opacity =
                    Some(validate::opacity(v).map_err(|m| op_err(pos, Some(&field), m))?);
            }
            _ => unreachable!("only called for validate::STYLE_KEYS"),
        }
        Ok(())
    }

    fn apply_to(&self, element: &mut Element) {
        let Some(base) = element.base_mut() else {
            return;
        };
        if let Some(v) = &self.stroke_color {
            base.stroke_color = v.clone();
        }
        if let Some(v) = &self.background_color {
            base.background_color = v.clone();
        }
        if let Some(v) = &self.fill_style {
            base.fill_style = v.clone();
        }
        if let Some(v) = self.stroke_width {
            base.stroke_width = v;
        }
        if let Some(v) = &self.stroke_style {
            base.stroke_style = v.clone();
        }
        if let Some(v) = self.roughness {
            base.roughness = v;
        }
        if let Some(v) = self.opacity {
            base.opacity = v;
        }
    }
}

/// Dispatches one `update` op's already-resolved `set` to the handler for `next.elements[index]`'s
/// kind (the table in AI spec §4.3), which also refreshes bound arrows' `fixedPoint`s when a
/// move or resize changes their geometry.
fn apply_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<(), OpError> {
    match &next.elements[index] {
        Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_) => {
            apply_generic_update(next, pos, index, set, measure, env)
        }
        Element::Text(t) if t.container_id.value().is_some() => {
            apply_bound_text_update(next, pos, index, set, measure, env)
        }
        Element::Text(_) => apply_text_update(next, pos, index, set, measure, env),
        Element::Line(_) | Element::Arrow(_) => apply_linear_update(next, pos, index, set, env),
        Element::Freedraw(_) => apply_freedraw_update(next, pos, index, set, env),
        Element::Image(_) => apply_image_update(next, pos, index, set, env),
        Element::Raw(_) => apply_raw_update(next, pos, index, set, env),
    }
}

/// `x`, `y`, `width` (>0), `height` (>0), `text` (the bound label) and `STYLE_KEYS` for a
/// rectangle, diamond or ellipse.
#[allow(clippy::too_many_arguments)]
fn apply_generic_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let mut new_x = None;
    let mut new_y = None;
    let mut new_width = None;
    let mut new_height = None;
    let mut new_text: Option<String> = None;
    let mut style = StyleSet::default();

    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            "width" => {
                new_width =
                    Some(validate::positive(v).map_err(|m| op_err(pos, Some("set.width"), m))?);
            }
            "height" => {
                new_height =
                    Some(validate::positive(v).map_err(|m| op_err(pos, Some("set.height"), m))?);
            }
            "text" => {
                let s = validate::string(v).map_err(|m| op_err(pos, Some("set.text"), m))?;
                if s.is_empty() {
                    return Err(op_err(pos, Some("set.text"), "text must not be empty"));
                }
                new_text = Some(s);
            }
            key if validate::STYLE_KEYS.contains(&key) => style.parse(pos, key, v)?,
            other => {
                return Err(op_err(pos, Some(&format!("set.{other}")), "unknown field"));
            }
        }
    }

    let container_before = next.elements[index].clone();
    let stroke_color_changed = style.stroke_color.is_some();
    if let Some(base) = next.elements[index].base_mut() {
        if let Some(x) = new_x {
            base.x = x;
        }
        if let Some(y) = new_y {
            base.y = y;
        }
        if let Some(w) = new_width {
            base.width = w;
        }
        if let Some(h) = new_height {
            base.height = h;
        }
    }
    style.apply_to(&mut next.elements[index]);
    let size_changed = match (
        next.elements[index].placement(),
        container_before.placement(),
    ) {
        (Some(after), Some(before)) => after.width != before.width || after.height != before.height,
        _ => false,
    };
    let moved = next.elements[index].placement() != container_before.placement();

    match (live_label_index(next, index), &new_text) {
        (Some(label_index), _) => {
            let label_before = next.elements[label_index].clone();
            let label_version_before = label_before.version();
            if let Some(text_value) = &new_text {
                update_label_text_fields(&mut next.elements[label_index], text_value, measure);
            }
            if stroke_color_changed {
                let color = next.elements[index]
                    .base()
                    .expect("generic container has a base")
                    .stroke_color
                    .clone();
                if let Some(base) = next.elements[label_index].base_mut() {
                    base.stroke_color = color;
                }
            }
            if size_changed {
                handle_bind_text_resize(next, index, None, false, false, false, measure, env);
            } else if new_text.is_some() {
                redraw_text_bounding_box(next, label_index, Some(index), measure, env);
            } else if moved {
                reposition_label(next, index, label_index);
            }
            if next.elements[label_index] != label_before {
                bump_if_not_bumped(&mut next.elements[label_index], label_version_before, env);
            }
        }
        (None, Some(text_value)) => {
            let label = LabelSpec {
                text: text_value.clone(),
                font_size: None,
                font_family: None,
                text_align: None,
                vertical_align: None,
                stroke_color: None,
                opacity: None,
            };
            let label_index = add::bind_label(next, index, &label, measure, env);
            let label_id = next.elements[label_index]
                .id()
                .expect("label has an id")
                .to_owned();
            sync_moved_indices(&mut next.elements, &HashSet::from([label_id]), env);
        }
        (None, None) => {}
    }

    if next.elements[index].placement() != container_before.placement() {
        refresh_bound_arrow_fixed_points(next, index, env);
    }

    if next.elements[index] != container_before {
        bump_if_not_bumped(&mut next.elements[index], container_before.version(), env);
    }

    Ok(())
}

/// `text`, `STYLE_KEYS` for a text element bound to a container; `x`/`y` error.
fn apply_bound_text_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let mut new_text: Option<String> = None;
    let mut style = StyleSet::default();
    for (key, v) in set {
        match key.as_str() {
            "x" | "y" => {
                return Err(op_err(
                    pos,
                    Some(&format!("set.{key}")),
                    "a label follows its container; move the container instead",
                ));
            }
            "text" => {
                let s = validate::string(v).map_err(|m| op_err(pos, Some("set.text"), m))?;
                if s.is_empty() {
                    return Err(op_err(pos, Some("set.text"), "text must not be empty"));
                }
                new_text = Some(s);
            }
            key if validate::STYLE_KEYS.contains(&key) => style.parse(pos, key, v)?,
            other => {
                return Err(op_err(pos, Some(&format!("set.{other}")), "unknown field"));
            }
        }
    }

    let container_id = match &next.elements[index] {
        Element::Text(t) => t.container_id.value().cloned(),
        _ => None,
    };

    let before = next.elements[index].clone();
    let container_before = container_id
        .as_deref()
        .and_then(|cid| find_alive_by_id(&next.elements, cid))
        .map(|position| (position, next.elements[position].clone()));
    if let Some(text_value) = &new_text {
        update_label_text_fields(&mut next.elements[index], text_value, measure);
    }
    style.apply_to(&mut next.elements[index]);

    if new_text.is_some()
        && let Some((container_index, _)) = &container_before
    {
        redraw_text_bounding_box(next, index, Some(*container_index), measure, env);
    }

    if next.elements[index] != before {
        bump_if_not_bumped(&mut next.elements[index], before.version(), env);
    }
    if let Some((container_index, container_before)) = &container_before
        && next.elements[*container_index] != *container_before
    {
        refresh_bound_arrow_fixed_points(next, *container_index, env);
    }
    Ok(())
}

/// `x`, `y`, `text`, `STYLE_KEYS` for a text element with no container.
fn apply_text_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let mut new_x = None;
    let mut new_y = None;
    let mut new_text: Option<String> = None;
    let mut style = StyleSet::default();
    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            "text" => {
                let s = validate::string(v).map_err(|m| op_err(pos, Some("set.text"), m))?;
                if s.is_empty() {
                    return Err(op_err(pos, Some("set.text"), "text must not be empty"));
                }
                new_text = Some(s);
            }
            key if validate::STYLE_KEYS.contains(&key) => style.parse(pos, key, v)?,
            other => {
                return Err(op_err(pos, Some(&format!("set.{other}")), "unknown field"));
            }
        }
    }

    let before = next.elements[index].clone();
    if let Some(base) = next.elements[index].base_mut() {
        if let Some(x) = new_x {
            base.x = x;
        }
        if let Some(y) = new_y {
            base.y = y;
        }
    }
    style.apply_to(&mut next.elements[index]);
    if let Some(text_value) = &new_text {
        // A fixed-width text keeps its width and rewraps at it (`redrawTextBoundingBox`).
        let fixed_width = match &next.elements[index] {
            Element::Text(t) if t.auto_resize == Some(false) => Some(t.base.width),
            _ => None,
        };
        if fixed_width.is_none()
            && let Element::Text(t) = &mut next.elements[index]
        {
            let line_height = t
                .line_height
                .unwrap_or_else(|| crate::text::line_height(t.font_family));
            let size = measure_text(
                &normalize_text(text_value),
                t.font_family,
                t.font_size,
                line_height,
                measure,
            );
            let [x, y] = crate::text::adjusted_origin(t, size, measure);
            t.base.x = x;
            t.base.y = y;
        }
        update_label_text_fields(&mut next.elements[index], text_value, measure);
        if let Some(width) = fixed_width {
            if let Element::Text(t) = &mut next.elements[index] {
                t.base.width = width;
            }
            redraw_text_bounding_box(next, index, None, measure, env);
        }
    }
    if next.elements[index] != before {
        bump_if_not_bumped(&mut next.elements[index], before.version(), env);
    }
    Ok(())
}

/// `x`, `y`, `points` (>=2), `STYLE_KEYS` for a line or arrow; `text` errors. An arrow also
/// translates its own bound label and refreshes its bound ends' `fixedPoint`s when its
/// position or points change.
fn apply_linear_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let is_arrow = matches!(next.elements[index], Element::Arrow(_));
    let mut new_x = None;
    let mut new_y = None;
    let mut new_points: Option<Vec<[f64; 2]>> = None;
    let mut style = StyleSet::default();
    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            "points" => {
                new_points = Some(
                    validate::points(v, 2)
                        .map_err(|(i, m)| op_err(pos, Some(&field_path("set.points", i)), m))?,
                );
            }
            "text" => {
                return Err(op_err(
                    pos,
                    Some("set.text"),
                    "napkin does not create labels on lines or arrows",
                ));
            }
            key if validate::STYLE_KEYS.contains(&key) => style.parse(pos, key, v)?,
            other => {
                return Err(op_err(pos, Some(&format!("set.{other}")), "unknown field"));
            }
        }
    }

    let before = next.elements[index].clone();
    let (old_x, old_y) = {
        let p = before.placement().expect("line/arrow has a placement");
        (p.x, p.y)
    };
    let base_x = new_x.unwrap_or(old_x);
    let base_y = new_y.unwrap_or(old_y);

    if let Some(points) = new_points {
        let (origin, normalized) = add::normalize_points([base_x, base_y], &points);
        let [width, height] = size_from_points(&normalized);
        if let Element::Line(l) | Element::Arrow(l) = &mut next.elements[index] {
            l.base.x = origin[0];
            l.base.y = origin[1];
            l.base.width = width;
            l.base.height = height;
            l.points = normalized;
        }
    } else if (new_x.is_some() || new_y.is_some())
        && let Some(base) = next.elements[index].base_mut()
    {
        base.x = base_x;
        base.y = base_y;
    }
    style.apply_to(&mut next.elements[index]);

    if is_arrow {
        let placement = next.elements[index]
            .placement()
            .expect("arrow has a placement");
        let (dx, dy) = (placement.x - old_x, placement.y - old_y);
        let geometry_changed = match (&before, &next.elements[index]) {
            (Element::Arrow(a), Element::Arrow(b)) => {
                a.points != b.points || dx != 0.0 || dy != 0.0
            }
            _ => false,
        };
        if geometry_changed {
            translate_arrow_label(next, index, dx, dy, env);
            for end in [LinearEnd::Start, LinearEnd::End] {
                if let Some(target_id) = next.elements[index].binding_target(end) {
                    let target_id = target_id.to_owned();
                    if let Some(target_index) = find_alive_by_id(&next.elements, &target_id) {
                        refresh_fixed_point(next, index, end, target_index, env);
                    }
                }
            }
        }
    }

    if next.elements[index] != before {
        bump_version(&mut next.elements[index], env);
    }
    Ok(())
}

/// `x`, `y`, `STYLE_KEYS` for a freedraw element.
fn apply_freedraw_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let mut new_x = None;
    let mut new_y = None;
    let mut style = StyleSet::default();
    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            key if validate::STYLE_KEYS.contains(&key) => style.parse(pos, key, v)?,
            other => {
                return Err(op_err(pos, Some(&format!("set.{other}")), "unknown field"));
            }
        }
    }
    let before = next.elements[index].clone();
    if let Some(base) = next.elements[index].base_mut() {
        if let Some(x) = new_x {
            base.x = x;
        }
        if let Some(y) = new_y {
            base.y = y;
        }
    }
    style.apply_to(&mut next.elements[index]);
    if next.elements[index] != before {
        bump_version(&mut next.elements[index], env);
    }
    Ok(())
}

/// `x`, `y`, `width` (>0), `height` (>0) and `opacity` for an image; anything else errors. `width` and
/// `height` are set as given, without keeping the aspect ratio.
fn apply_image_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let id = next.elements[index].id().unwrap_or_default().to_owned();
    let mut new_x = None;
    let mut new_y = None;
    let mut new_width = None;
    let mut new_height = None;
    let mut new_opacity = None;
    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            "width" => {
                new_width =
                    Some(validate::positive(v).map_err(|m| op_err(pos, Some("set.width"), m))?);
            }
            "height" => {
                new_height =
                    Some(validate::positive(v).map_err(|m| op_err(pos, Some("set.height"), m))?);
            }
            "opacity" => {
                new_opacity =
                    Some(validate::opacity(v).map_err(|m| op_err(pos, Some("set.opacity"), m))?);
            }
            other => {
                return Err(op_err(
                    pos,
                    Some(&format!("set.{other}")),
                    format!("{id} is an image napkin can only move, resize and fade"),
                ));
            }
        }
    }
    let before = next.elements[index].clone();
    if let Some(base) = next.elements[index].base_mut() {
        if let Some(x) = new_x {
            base.x = x;
        }
        if let Some(y) = new_y {
            base.y = y;
        }
        if let Some(w) = new_width {
            base.width = w;
        }
        if let Some(h) = new_height {
            base.height = h;
        }
        if let Some(o) = new_opacity {
            base.opacity = o;
        }
    }
    if next.elements[index].placement() != before.placement() {
        refresh_bound_arrow_fixed_points(next, index, env);
    }
    if next.elements[index] != before {
        bump_if_not_bumped(&mut next.elements[index], before.version(), env);
    }
    Ok(())
}

/// `x`, `y` for a `Raw` element (spec §5.2); anything else errors.
fn apply_raw_update(
    next: &mut SceneFile,
    pos: usize,
    index: usize,
    set: &Map<String, Value>,
    env: &mut impl Env,
) -> Result<(), OpError> {
    let id = next.elements[index].id().unwrap_or_default().to_owned();
    let kind = next.elements[index].kind().to_owned();
    let mut new_x = None;
    let mut new_y = None;
    for (key, v) in set {
        match key.as_str() {
            "x" => new_x = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.x"), m))?),
            "y" => new_y = Some(validate::finite(v).map_err(|m| op_err(pos, Some("set.y"), m))?),
            other => {
                return Err(op_err(
                    pos,
                    Some(&format!("set.{other}")),
                    format!("{id} is a {kind} napkin can only move"),
                ));
            }
        }
    }
    let before = next.elements[index].clone();
    if new_x.is_some() || new_y.is_some() {
        let placement = before.placement().ok_or_else(|| {
            op_err(
                pos,
                Some("id"),
                format!("{id} has no position napkin can change"),
            )
        })?;
        next.elements[index]
            .set_position(new_x.unwrap_or(placement.x), new_y.unwrap_or(placement.y));
    }
    if next.elements[index] != before {
        bump_version(&mut next.elements[index], env);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_update_op(
    next: &mut SceneFile,
    pos: usize,
    value: &Value,
    created: &BTreeMap<String, String>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
    updated: &mut Vec<String>,
) -> Result<(), OpError> {
    let obj = value
        .as_object()
        .expect("classified as an object by op_kind");
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| op_err(pos, Some("id"), "must be a string"))?;
    let set = obj
        .get("set")
        .and_then(Value::as_object)
        .ok_or_else(|| op_err(pos, Some("set"), "must be an object"))?;

    let (index, real_id) =
        resolve_target(id, created, next).map_err(|m| op_err(pos, Some("id"), m))?;

    apply_update(next, pos, index, set, measure, env)?;

    if !updated.contains(&real_id) {
        updated.push(real_id);
    }
    Ok(())
}

/// `{"op": "delete", "ids": [...]}`: `ids` must be a non-empty array of ids that each resolve
/// (batch alias or live element); one `edit::delete_selection` call for the whole op.
fn apply_delete_op(
    next: &mut SceneFile,
    pos: usize,
    value: &Value,
    created: &BTreeMap<String, String>,
    env: &mut impl Env,
    deleted: &mut Vec<String>,
) -> Result<(), OpError> {
    let obj = value
        .as_object()
        .expect("classified as an object by op_kind");
    let ids_arr = obj
        .get("ids")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| op_err(pos, Some("ids"), "must be a non-empty array of ids"))?;

    let mut real_ids: Vec<String> = Vec::with_capacity(ids_arr.len());
    for (i, id_value) in ids_arr.iter().enumerate() {
        let field = field_path("ids", Some(i));
        let id_str = id_value
            .as_str()
            .ok_or_else(|| op_err(pos, Some(&field), "must be a string"))?;
        let (_, real_id) =
            resolve_target(id_str, created, next).map_err(|m| op_err(pos, Some(&field), m))?;
        real_ids.push(real_id);
    }

    let selection = Selection::from_ids(real_ids.iter().cloned());
    delete_selection(next, &selection, env);
    for id in real_ids {
        if !deleted.contains(&id) {
            deleted.push(id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample::{self, CharWidthMeasure};

    fn scene() -> SceneFile {
        sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 60.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::text("t", [26.0, 17.5, 48.0, 25.0], "box", Some("r")),
                // `bindTextToContainer`'s own default alignment for a container label: the
                // shared `sample::text` fixture defaults to left/top (a plain, unbound text
                // element's default), which does not match this fixture's centered position.
                json!({"textAlign": "center", "verticalAlign": "middle"}),
            ),
            sample::with(
                sample::linear("arrow", "a", [105.0, 30.0], &[[0.0, 0.0], [90.0, 0.0]]),
                json!({"startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [1.05, 0.5001]}}),
            ),
            // A real, already-valid `index` (later than the other elements' shared "a0",
            // itself already an invalid run of duplicates): without one, adding any element
            // triggers `sync_invalid_indices`'s full-array fallback, which would reassign
            // `img`'s index too and break the "untouched elements" assertion below.
            json!({"id": "img", "type": "image", "x": 300, "y": 0, "width": 50, "height": 50,
                   "isDeleted": false, "version": 1, "versionNonce": 1, "index": "a1"}),
        ])
    }

    fn apply(file: &SceneFile, batch: Value) -> Result<(SceneFile, BatchReport), Vec<OpError>> {
        apply_batch(
            file,
            &batch,
            &ItemStyle::default(),
            &mut CharWidthMeasure,
            &mut TestEnv(0),
        )
    }

    fn get<'a>(file: &'a SceneFile, id: &str) -> &'a Element {
        file.elements.iter().find(|e| e.id() == Some(id)).expect(id)
    }

    /// Same increment-by-37 fixed generator `edit.rs`'s tests use.
    struct TestEnv(u8);

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            for b in bytes {
                self.0 = self.0.wrapping_add(37);
                *b = self.0;
            }
        }

        fn now_ms(&mut self) -> f64 {
            42.0
        }
    }

    #[test]
    fn a_failing_op_rejects_the_whole_batch() {
        let file = scene();
        let errors = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "r", "set": {"x": 10}},
                {"op": "update", "id": "gone", "set": {"x": 1}},
                {"op": "update", "id": "img", "set": {"strokeColor": "#e03131"}},
                {"op": "delete", "ids": []},
                {"op": "paint"},
            ]}),
        )
        .unwrap_err();
        let positions: Vec<Option<usize>> = errors.iter().map(|e| e.op).collect();
        assert_eq!(positions, vec![Some(1), Some(2), Some(3), Some(4)]);
        assert!(
            apply(&file, json!({"nope": []})).unwrap_err()[0]
                .op
                .is_none()
        );
        assert!(
            apply(&file, json!({"ops": []})).is_err(),
            "an empty batch is an error"
        );
    }

    #[test]
    fn a_typed_image_takes_position_size_and_opacity_but_no_other_style() {
        let image = sample::with(
            sample::generic("rectangle", "pic", [0.0, 0.0, 200.0, 100.0]),
            json!({"type": "image", "strokeColor": "transparent", "status": "saved",
                   "fileId": "f1", "scale": [1, 1], "crop": null}),
        );
        let file = sample::file(vec![image]);
        assert!(matches!(get(&file, "pic"), Element::Image(_)));

        let (next, report) = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "pic", "set": {"x": 10, "y": 20, "width": 50, "height": 25}},
            ]}),
        )
        .unwrap();
        assert_eq!(report.updated, vec!["pic"]);
        let p = get(&next, "pic").placement().unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (10.0, 20.0, 50.0, 25.0));
        assert_eq!(
            get(&next, "pic").version(),
            get(&file, "pic").version() + 1.0
        );

        let (faded, _) = apply(
            &file,
            json!({"ops": [{"op": "update", "id": "pic", "set": {"opacity": 40}}]}),
        )
        .unwrap();
        assert_eq!(get(&faded, "pic").to_value()["opacity"], json!(40.0));

        let errors = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "pic", "set": {"strokeColor": "#e03131"}},
                {"op": "update", "id": "pic", "set": {"width": 0}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn an_unknown_update_field_is_reported_as_set_dot_key() {
        let errors = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "r", "set": {"nope": 1}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors[0].field.as_deref(), Some("set.nope"));
    }

    #[test]
    fn moving_a_container_recenters_its_label_and_refreshes_arrow_bindings() {
        let (next, report) = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "r", "set": {"x": 0, "y": 100, "width": 200}},
            ]}),
        )
        .unwrap();
        assert_eq!(report.updated, vec!["r"]);
        let label = get(&next, "t").placement().unwrap();
        // The label is remeasured on a resize: "box" is 36 wide (the fixture's 48 is stale),
        // centered on the wider rectangle's x = 100.
        assert_eq!((label.x, label.y), (82.0, 117.5));
        // The arrow stayed where it was; its start's fixedPoint now describes (105, 30)
        // relative to the moved, wider rectangle.
        let binding = get(&next, "a").to_value()["startBinding"].clone();
        assert_eq!(binding["fixedPoint"], json!([0.525, -1.1666666666666667]));
        assert_eq!(binding["mode"], json!("orbit"));
    }

    #[test]
    fn setting_text_to_empty_errors_for_a_bound_label_and_an_unbound_text() {
        let errors = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "t", "set": {"text": ""}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors[0].message, "text must not be empty");

        let file = sample::file(vec![sample::text(
            "free",
            [0.0, 0.0, 40.0, 25.0],
            "hello",
            None,
        )]);
        let errors = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "free", "set": {"text": ""}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors[0].message, "text must not be empty");
    }

    #[test]
    fn container_text_updates_or_creates_its_label() {
        let (next, _) = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "r", "set": {"text": "renamed"}},
            ]}),
        )
        .unwrap();
        let Element::Text(label) = get(&next, "t") else {
            panic!("text")
        };
        assert_eq!(label.text, "renamed");
        assert_eq!(label.base.width, 7.0 * 20.0 * 0.6);

        let (next, report) = apply(
            &scene(),
            json!({"ops": [
                {"op": "add", "type": "ellipse", "id": "e", "x": 0, "y": 200, "width": 120, "height": 60},
                {"op": "update", "id": "e", "set": {"text": "later"}},
            ]}),
        )
        .unwrap();
        let e = report.created["e"].clone();
        let bound = get(&next, &e).bound_elements();
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].1, "text");
    }

    #[test]
    fn a_label_created_via_update_gets_a_real_index_ordered_after_the_rest() {
        let (next, report) = apply(
            &scene(),
            json!({"ops": [
                {"op": "add", "type": "ellipse", "id": "e", "x": 0, "y": 200, "width": 120, "height": 60},
                {"op": "update", "id": "e", "set": {"text": "later"}},
            ]}),
        )
        .unwrap();
        let e = report.created["e"].clone();
        let (label_id, _) = get(&next, &e).bound_elements()[0];
        let label_id = label_id.to_owned();
        let label_index = get(&next, &label_id)
            .index()
            .expect("a label created via update must get a real index, not null");
        let max_other_index = next
            .elements
            .iter()
            .filter(|el| el.id() != Some(label_id.as_str()))
            .filter_map(Element::index)
            .max()
            .expect("the scene has other indexed elements");
        assert!(
            label_index > max_other_index,
            "label index {label_index:?} should sort after {max_other_index:?}"
        );
    }

    #[test]
    fn raw_elements_only_move_and_labels_do_not_move_alone() {
        let (next, _) = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "img", "set": {"x": 310, "y": 5}},
            ]}),
        )
        .unwrap();
        assert_eq!(get(&next, "img").placement().unwrap().x, 310.0);
        let errors = apply(
            &scene(),
            json!({"ops": [
                {"op": "update", "id": "t", "set": {"x": 3}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors[0].field.as_deref(), Some("set.x"));
    }

    #[test]
    fn moving_a_raw_element_without_a_numeric_placement_errors_instead_of_panicking() {
        let file = sample::file(vec![json!({"id": "w", "type": "weird", "x": "a"})]);
        let errors = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "w", "set": {"x": 5}},
            ]}),
        )
        .unwrap_err();
        assert_eq!(errors[0].message, "w has no position napkin can change");
    }

    #[test]
    fn delete_takes_the_label_and_unbinds_the_arrow() {
        let (next, report) = apply(
            &scene(),
            json!({"ops": [
                {"op": "delete", "ids": ["r"]},
            ]}),
        )
        .unwrap();
        assert_eq!(report.deleted, vec!["r"]);
        assert!(get(&next, "r").is_deleted() && get(&next, "t").is_deleted());
        assert_eq!(get(&next, "a").binding_target(LinearEnd::Start), None);
    }

    #[test]
    fn adds_run_before_updates_and_every_change_bumps_once() {
        let file = scene();
        let (next, report) = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "b", "set": {"strokeColor": "#e03131"}},
                {"op": "add", "type": "rectangle", "id": "b", "x": 0, "y": 300, "width": 50, "height": 50},
            ]}),
        )
        .unwrap();
        let id = &report.created["b"];
        assert_eq!(report.added, vec![id.clone()]);
        assert_eq!(get(&next, id).to_value()["strokeColor"], json!("#e03131"));
        // Untouched elements keep their exact JSON, version included.
        assert_eq!(get(&next, "img"), get(&file, "img"));
    }

    #[test]
    fn setting_a_field_to_its_current_value_does_not_bump() {
        let file = scene();
        let (next, _) = apply(
            &file,
            json!({"ops": [
                {"op": "update", "id": "r", "set": {"x": 0}},
            ]}),
        )
        .unwrap();
        assert_eq!(get(&next, "r"), get(&file, "r"));
    }
}
