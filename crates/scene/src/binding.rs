//! Arrow binding, ported at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` from
//! `packages/element/src/binding.ts`: creation (`bindBindingElement` non-elbow branch,
//! `applyBinding`, `calculateFixedPointForNonElbowArrowBinding`, `getBindingGap`,
//! `normalizeFixedPoint`), the arrows-follow-their-targets half (`updateBoundElements`,
//! `updateBoundPoint`, `extractBinding`, `getGlobalFixedPointForBindableElement`,
//! `boundElementsVisitor`/`bindableElementsVisitor`), and the strategy for dragging an
//! endpoint (`getBindingStrategyForDraggingBindingElementEndpoints_simple`,
//! `bindOrUnbindBindingElement`, `maxBindingDistance_simple`); from
//! `packages/element/src/linearElementEditor.ts`: `createPointAt`,
//! `getPointAtIndexGlobalCoordinates`, `pointDraggingUpdates` and the binding bookkeeping of
//! `handlePointDragging`/`handlePointerMove`; from `packages/element/src/utils.ts`:
//! `projectFixedPointOntoDiagonal`, `getSnapOutlineMidPoint`,
//! `getDiagonalsForBindableElement`; from `packages/excalidraw/data/restore.ts`:
//! `repairBinding`; and `packages/element/src/bounds.ts`'s `elementCenterPoint`. Only arrows
//! with the current binding format (`mode` and `fixedPoint`) follow; an elbow arrow never
//! binds or follows. The `COMPLEX_BINDINGS` paths, the Alt key, grid snapping and elbow arrows
//! are not ported.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::collision;
use crate::edit::move_points;
use crate::element::{Element, LinearEnd};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{GeometryCache, element_absolute_coords, element_bounds, rotate_point};
use crate::new_element::bump_version;
use crate::text::TextMeasure;

/// `BASE_BINDING_GAP`.
pub const BASE_BINDING_GAP: f64 = 5.0;
/// `MIN_BINDABLE_SIZE`.
const MIN_BINDABLE_SIZE: f64 = 1.0;
/// `FIXED_POINT_BOUND`.
const FIXED_POINT_BOUND: f64 = 10.0;

/// `normalizeFixedPoint`: a non-finite point becomes the "just off center" sentinel; otherwise
/// each ratio is clamped to `±FIXED_POINT_BOUND`, and a clamped ratio within `EPSILON` of `0.5`
/// (in either coordinate) is nudged to `0.5001` to keep an arrow's heading from flipping on
/// floating-point noise.
pub fn normalize_fixed_point(point: [f64; 2]) -> [f64; 2] {
    if !point[0].is_finite() || !point[1].is_finite() {
        return [0.5001, 0.5001];
    }
    const EPSILON: f64 = 0.0001;
    let clamped = point.map(|ratio| ratio.clamp(-FIXED_POINT_BOUND, FIXED_POINT_BOUND));
    if clamped.iter().any(|ratio| (ratio - 0.5).abs() < EPSILON) {
        clamped.map(|ratio| {
            if (ratio - 0.5).abs() < EPSILON {
                0.5001
            } else {
                ratio
            }
        })
    } else {
        clamped
    }
}

/// `getPointAtIndexGlobalCoordinates(arrow, 0 | -1)`: `None` for anything but a line or arrow,
/// or one with no points at the requested end.
pub fn linear_end_point(element: &Element, end: LinearEnd) -> Option<[f64; 2]> {
    let (Element::Line(l) | Element::Arrow(l)) = element else {
        return None;
    };
    let point = match end {
        LinearEnd::Start => l.points.first(),
        LinearEnd::End => l.points.last(),
    }?;
    let (_, center) = element_absolute_coords(element)?;
    Some(rotate_point(
        [l.base.x + point[0], l.base.y + point[1]],
        center,
        l.base.angle,
    ))
}

/// `calculateFixedPointForNonElbowArrowBinding`: `focus` (or, without one, `arrow`'s `end`
/// point) expressed as a ratio of `target`'s unrotated bounds (`elementCenterPoint`/
/// `getCenterForBounds` is `target`'s axis-aligned [`element_bounds`] center, not its
/// unrotated placement center). `None` when either element's geometry cannot be read.
pub fn fixed_point_for(
    arrow: &Element,
    end: LinearEnd,
    target: &Element,
    focus: Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    let edge = match focus {
        Some(focus) => focus,
        None => linear_end_point(arrow, end)?,
    };
    let placement = target.placement()?;
    let [x1, y1, x2, y2] = element_bounds(target)?;
    let center = [x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0];
    let unrotated = collision::rotate_unless_zero(edge, center, -placement.angle);
    if placement.width < MIN_BINDABLE_SIZE || placement.height < MIN_BINDABLE_SIZE {
        return Some(normalize_fixed_point([0.5, 0.5]));
    }
    // `getBindingGap` for a non-elbow arrow.
    let gap = BASE_BINDING_GAP + collision::stroke_width(target) / 2.0;
    Some(normalize_fixed_point([
        (unrotated[0] - placement.x) / placement.width.max(gap),
        (unrotated[1] - placement.y) / placement.height.max(gap),
    ]))
}

/// `bindBindingElement(arrow, target, mode, end, scene, focus)` for a non-elbow arrow
/// (`applyBinding`): sets the arrow's binding to `{elementId, mode, fixedPoint}` and, unless
/// the target already lists the arrow, appends it to the target's `boundElements`; each
/// element that changed gets a version bump (`scene.mutateElement` in `applyBinding`). `focus`
/// is the scene point the binding anchors to; without one the arrow's own `end` point is used.
/// A no-op when `fixed_point_for` cannot compute a fixed point (an untyped or unplaceable
/// arrow or target).
pub fn bind_arrow(
    file: &mut SceneFile,
    arrow: usize,
    end: LinearEnd,
    target: usize,
    mode: &str,
    focus: Option<[f64; 2]>,
    env: &mut impl Env,
) {
    let Some(fixed_point) =
        fixed_point_for(&file.elements[arrow], end, &file.elements[target], focus)
    else {
        return;
    };
    let target_id = file.elements[target]
        .id()
        .expect("bindable target has an id")
        .to_owned();
    let arrow_id = file.elements[arrow]
        .id()
        .expect("arrow has an id")
        .to_owned();

    let before = file.elements[arrow].clone();
    file.elements[arrow].set_binding(
        end,
        json!({"elementId": target_id, "mode": mode, "fixedPoint": fixed_point}),
    );
    if file.elements[arrow] != before {
        bump_version(&mut file.elements[arrow], env);
    }

    let before = file.elements[target].clone();
    file.elements[target].add_bound_element(&arrow_id, "arrow");
    if file.elements[target] != before {
        bump_version(&mut file.elements[target], env);
    }
}

/// `BASE_ARROW_MIN_LENGTH`.
const BASE_ARROW_MIN_LENGTH: f64 = 10.0;

/// A `{elementId, mode, fixedPoint}` binding.
struct FixedBinding {
    element_id: String,
    mode: String,
    fixed_point: [f64; 2],
}

enum BindingState {
    Unbound,
    /// A binding without `mode`/`fixedPoint` (the `focus`/`gap` format).
    Legacy,
    Fixed(FixedBinding),
}

fn read_binding(arrow: &Element, end: LinearEnd) -> BindingState {
    let Some(value) = arrow.binding_value(end) else {
        return BindingState::Unbound;
    };
    let fixed_point = value
        .get("fixedPoint")
        .and_then(Value::as_array)
        .and_then(|a| match a.as_slice() {
            [x, y] => Some([x.as_f64()?, y.as_f64()?]),
            _ => None,
        });
    match (
        value.get("elementId").and_then(Value::as_str),
        value.get("mode").and_then(Value::as_str),
        fixed_point,
    ) {
        (Some(id), Some(mode), Some(fixed_point)) => BindingState::Fixed(FixedBinding {
            element_id: id.to_owned(),
            mode: mode.to_owned(),
            fixed_point,
        }),
        _ => BindingState::Legacy,
    }
}

fn opposite(end: LinearEnd) -> LinearEnd {
    match end {
        LinearEnd::Start => LinearEnd::End,
        LinearEnd::End => LinearEnd::Start,
    }
}

/// `elementsMap.get(id)` over `getNonDeletedElementsMap`.
fn live_element<'a>(elements: &'a [Element], id: &str) -> Option<&'a Element> {
    elements
        .iter()
        .find(|element| !element.is_deleted() && element.id() == Some(id))
}

/// `getBindingGap` for a non-elbow arrow.
fn binding_gap(target: &Element) -> f64 {
    BASE_BINDING_GAP + collision::stroke_width(target) / 2.0
}

/// `getGlobalFixedPointForBindableElement`: where `fixed_point` (a ratio of `target`'s
/// unrotated frame) lies in the scene, rotated about `target`'s bounds center. `None` when
/// `target`'s geometry cannot be read.
pub fn global_fixed_point(fixed_point: [f64; 2], target: &Element) -> Option<[f64; 2]> {
    let [fx, fy] = normalize_fixed_point(fixed_point);
    let placement = target.placement()?;
    let [x1, y1, x2, y2] = element_bounds(target)?;
    let center = [x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0];
    Some(rotate_point(
        [
            placement.x + placement.width * fx,
            placement.y + placement.height * fy,
        ],
        center,
        placement.angle,
    ))
}

/// `LinearElementEditor.createPointAt` without grid snapping: the local point of `arrow` that
/// sits at scene point `global`. `None` for an arrow whose geometry cannot be read (no points).
fn create_point_at(
    geometry: &mut GeometryCache,
    arrow: &Element,
    global: [f64; 2],
) -> Option<[f64; 2]> {
    let (_, center) = geometry.absolute_coords(arrow)?;
    let placement = arrow.placement()?;
    let [x, y] = collision::rotate_unless_zero(global, center, -placement.angle);
    Some([x - placement.x, y - placement.y])
}

/// `LinearElementEditor.getPointGlobalCoordinates`: the scene position of `arrow`'s local
/// `point`. `None` when the arrow's geometry cannot be read.
fn local_to_global(
    geometry: &mut GeometryCache,
    arrow: &Element,
    point: [f64; 2],
) -> Option<[f64; 2]> {
    let (_, center) = geometry.absolute_coords(arrow)?;
    let placement = arrow.placement()?;
    Some(collision::rotate_unless_zero(
        [placement.x + point[0], placement.y + point[1]],
        center,
        placement.angle,
    ))
}

/// `LinearElementEditor.getPointAtIndexGlobalCoordinates` for a typed arrow; a negative index
/// counts from the end. An index with no point gives the arrow's own origin, as JS does.
/// `None` for anything but an arrow with points.
fn point_at_index_global(
    geometry: &mut GeometryCache,
    arrow: &Element,
    index: isize,
) -> Option<[f64; 2]> {
    let Element::Arrow(l) = arrow else {
        return None;
    };
    let index = if index < 0 {
        l.points.len() as isize + index
    } else {
        index
    };
    let local = usize::try_from(index)
        .ok()
        .and_then(|i| l.points.get(i))
        .copied()
        .unwrap_or([0.0, 0.0]);
    local_to_global(geometry, arrow, local)
}

/// The point of `points` nearest `to` (the first of equals), i.e. JS's
/// `.sort((a, b) => pointDistanceSq(a, to) - pointDistanceSq(b, to))[0]`.
fn nearest(points: Vec<[f64; 2]>, to: [f64; 2]) -> Option<[f64; 2]> {
    let distance_sq = |p: &[f64; 2]| (p[0] - to[0]).powi(2) + (p[1] - to[1]).powi(2);
    let mut best: Option<[f64; 2]> = None;
    for point in points {
        if best.is_none_or(|b| distance_sq(&point) < distance_sq(&b)) {
            best = Some(point);
        }
    }
    best
}

/// JS `value || fallback` for a number: 0 and NaN fall through.
fn or_fallback(value: f64, fallback: f64) -> f64 {
    if value == 0.0 || value.is_nan() {
        fallback
    } else {
        value
    }
}

/// `updateBoundPoint`: the new local position of `arrow`'s `end` bound to `target`, or `None`
/// when JS returns null (no binding in the current format, a >2-point arrow whose binding
/// names another element than `target`, or a fresh arrow whose last point is still [0, 0]).
/// Reads `elements` for the other end's target.
pub fn update_bound_point(
    geometry: &mut GeometryCache,
    elements: &[Element],
    arrow: &Element,
    end: LinearEnd,
    target: &Element,
    dragging: bool,
) -> Option<[f64; 2]> {
    let Element::Arrow(l) = arrow else {
        return None;
    };
    let BindingState::Fixed(binding) = read_binding(arrow, end) else {
        return None;
    };
    // An arrow with no points has nothing to move.
    if l.points.is_empty() {
        return None;
    }
    if (target.id() != Some(binding.element_id.as_str()) && l.points.len() > 2)
        || l.points.last() == Some(&[0.0, 0.0])
    {
        return None;
    }

    let focus = global_fixed_point(binding.fixed_point, target)?;
    // 0. An inside binding needs no calculation and ignores the other end.
    if binding.mode == "inside" {
        return create_point_at(geometry, arrow, focus);
    }

    let other_end = opposite(end);
    let (other_bindable, other_focus) = match read_binding(arrow, other_end) {
        BindingState::Fixed(other) => {
            let element = live_element(elements, &other.element_id);
            let focus = element.and_then(|e| global_fixed_point(other.fixed_point, e));
            (element, focus)
        }
        _ => (None, None),
    };
    let other_arrow_point = point_at_index_global(
        geometry,
        arrow,
        match end {
            LinearEnd::Start => 1,
            LinearEnd::End => -2,
        },
    )?;
    let other_focus_or_arrow_point = if l.points.len() == 2 {
        other_focus.unwrap_or(other_arrow_point)
    } else {
        other_arrow_point
    };
    let intersector = [focus, other_focus_or_arrow_point];
    let other_outline = other_bindable.and_then(|other| {
        nearest(
            collision::intersect_element_with_line_segment(
                geometry,
                other,
                intersector,
                binding_gap(other),
            ),
            focus,
        )
    });
    let outline = nearest(
        collision::intersect_element_with_line_segment(
            geometry,
            target,
            intersector,
            binding_gap(target),
        ),
        other_focus_or_arrow_point,
    );
    // `startArrowhead !== null`: an absent key counts as an arrowhead.
    let start_has_arrowhead = !matches!(l.start_arrowhead, crate::json::Slot::Null);
    let end_has_arrowhead = !matches!(l.end_arrowhead, crate::json::Slot::Null);
    let resolved_target = if (!start_has_arrowhead && !end_has_arrowhead)
        || (end == LinearEnd::Start && start_has_arrowhead)
        || (end == LinearEnd::End && end_has_arrowhead)
    {
        focus
    } else {
        outline.unwrap_or(focus)
    };

    let area = |element: &Element| element.placement().map_or(0.0, |p| p.width * p.height);

    // 1. The outline point inside the other shape would invert the arrow: use the target.
    if let (Some(other), Some(outline), false) = (other_bindable, outline, dragging)
        && area(other) < area(target) * 2.0
        && collision::hit_element_itself_with(geometry, other, outline, binding_gap(other), true)
    {
        return create_point_at(geometry, arrow, resolved_target);
    }

    let other_target_point = if other_bindable.is_some() {
        other_outline.or(other_focus).unwrap_or(other_arrow_point)
    } else {
        other_arrow_point
    };
    let reference = outline.unwrap_or(focus);
    let arrow_too_short = rough::js::hypot(
        other_target_point[0] - reference[0],
        other_target_point[1] - reference[1],
    ) <= BASE_ARROW_MIN_LENGTH;

    // 2. An unconnected other end: a short arrow takes the focus point, else the outline.
    if other_bindable.is_none() {
        let point = if arrow_too_short {
            focus
        } else {
            outline.unwrap_or(focus)
        };
        return create_point_at(geometry, arrow, point);
    }

    // 3. Too short while connected on both ends.
    if arrow_too_short {
        return create_point_at(
            geometry,
            arrow,
            [
                or_fallback(resolved_target[0], focus[0]),
                or_fallback(resolved_target[1], focus[1]),
            ],
        );
    }

    // 4. The general case: snap to the outline.
    let snapped = outline.unwrap_or(focus);
    create_point_at(
        geometry,
        arrow,
        [
            or_fallback(snapped[0], focus[0]),
            or_fallback(snapped[1], focus[1]),
        ],
    )
}

/// `updateBoundElements(changed, scene, {simultaneouslyUpdated})`: moves the bound ends of every
/// live, typed, non-elbow arrow in `changed`'s `boundElements` whose id is not in
/// `simultaneous`, then `sync_arrow_label`s it. Bindings without `mode` (legacy) are skipped.
/// Returns whether anything changed.
pub fn update_bound_elements(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    changed: usize,
    simultaneous: &HashSet<String>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool {
    if !collision::is_bindable_including_locked(&file.elements[changed]) {
        return false;
    }
    let Some(changed_id) = file.elements[changed].id().map(str::to_owned) else {
        return false;
    };
    let bound_ids: Vec<String> = file.elements[changed]
        .bound_elements()
        .into_iter()
        .map(|(id, _)| id.to_owned())
        .collect();

    let mut any_changed = false;
    for arrow_id in bound_ids {
        let Some(arrow) = file
            .elements
            .iter()
            .position(|e| !e.is_deleted() && e.id() == Some(arrow_id.as_str()))
        else {
            continue;
        };
        let Element::Arrow(l) = &file.elements[arrow] else {
            continue;
        };
        if l.elbowed == Some(true) || simultaneous.contains(&arrow_id) {
            continue;
        }
        let (start, end) = (
            read_binding(&file.elements[arrow], LinearEnd::Start),
            read_binding(&file.elements[arrow], LinearEnd::End),
        );
        if matches!(start, BindingState::Legacy) || matches!(end, BindingState::Legacy) {
            continue;
        }
        let ids = [&start, &end].map(|state| match state {
            BindingState::Fixed(b) => Some(b.element_id.as_str()),
            _ => None,
        });
        // `doesNeedUpdate`: the arrow's `boundElements` entry may be stale.
        if !ids.contains(&Some(changed_id.as_str())) {
            continue;
        }

        let last = l.points.len().saturating_sub(1);
        let mut updates: Vec<(usize, [f64; 2])> = Vec::new();
        for (binding_end, index) in [(LinearEnd::Start, 0), (LinearEnd::End, last)] {
            let Some(target_id) = ids[usize::from(binding_end == LinearEnd::End)] else {
                continue;
            };
            let Some(target) = live_element(&file.elements, target_id) else {
                continue;
            };
            if !collision::is_bindable_including_locked(target) {
                continue;
            }
            if let Some(point) = update_bound_point(
                geometry,
                &file.elements,
                &file.elements[arrow],
                binding_end,
                target,
                false,
            ) {
                updates.push((index, point));
            }
        }

        let same_target_both_ends = ids[0].is_some()
            && ids[0] == ids[1]
            && live_element(&file.elements, ids[0].unwrap_or_default()).is_some();
        any_changed |= move_points(
            &mut file.elements[arrow],
            &updates,
            same_target_both_ends,
            env,
        );
        any_changed |= crate::bound_text::sync_arrow_label(geometry, file, arrow, measure, env);
    }
    any_changed
}

// ---------------------------------------------------------------------------
// Binding while dragging arrow endpoints
// ---------------------------------------------------------------------------

/// The parts of `LinearElementEditor.initialState` (and `pointerOffset`) the binding strategy
/// reads, carried by the editor for one gesture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearDragState {
    /// `appState.newElement` is this arrow (creation, including click-to-add-point).
    pub new_arrow: bool,
    /// `initialState.origin`: the scene point the gesture started at.
    pub origin: Option<[f64; 2]>,
    /// `initialState.altFocusPoint`.
    pub alt_focus_point: Option<[f64; 2]>,
    /// `initialState.arrowOtherEndpointInitialBinding?.mode === "inside"`.
    pub other_end_started_inside: bool,
    /// `pointerOffset`: how far the pointer sat from the grabbed point when the gesture began.
    pub pointer_offset: [f64; 2],
}

/// `appState.suggestedBinding`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuggestedBinding {
    pub target: usize,
    pub mid_point: Option<[f64; 2]>,
}

/// Per-event inputs shared by the drag and finalize calls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BindingInput {
    /// Scene pointer.
    pub pointer: [f64; 2],
    pub zoom: f64,
    /// Shift (`shouldRotateWithDiscreteAngle`) and a single point dragged.
    pub angle_locked: bool,
    /// `isBindingEnabled`: false while Ctrl is held.
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindMode {
    Orbit,
    Inside,
}

impl BindMode {
    fn as_str(self) -> &'static str {
        match self {
            BindMode::Orbit => "orbit",
            BindMode::Inside => "inside",
        }
    }
}

/// One end's `BindingStrategy`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Strategy {
    /// `{mode: undefined}`: keep the existing binding.
    Keep,
    /// `{mode: null}`: break it.
    Break,
    /// Bind to `target` (a position in the element list) anchored at the scene point `focus`.
    Bind {
        mode: BindMode,
        target: usize,
        focus: [f64; 2],
    },
}

/// `maxBindingDistance_simple`.
pub fn max_binding_distance(zoom: f64) -> f64 {
    const BASE_BINDING_DISTANCE: f64 = 15.0;
    let zoom_value = if zoom != 0.0 && zoom < 1.0 { zoom } else { 1.0 };
    (BASE_BINDING_DISTANCE / (zoom_value * 1.5))
        .clamp(BASE_BINDING_DISTANCE, BASE_BINDING_DISTANCE * 2.0)
}

/// The center `elementCenterPoint` gives a bindable element: the center of its rotated bounds.
fn element_center(geometry: &mut GeometryCache, element: &Element) -> Option<[f64; 2]> {
    let [x1, y1, x2, y2] = geometry.bounds(element)?;
    Some([x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0])
}

/// The position of the live element with `id` that can be a binding target.
fn live_bindable_position(elements: &[Element], id: &str) -> Option<usize> {
    elements.iter().position(|element| {
        !element.is_deleted()
            && element.id() == Some(id)
            && collision::is_bindable_including_locked(element)
    })
}

/// `getSnapOutlineMidPoint`: the side midpoint of `target` within the binding distance of
/// `point` (and outside the shape), if any.
pub fn snap_outline_mid_point(
    geometry: &mut GeometryCache,
    point: [f64; 2],
    target: &Element,
    zoom: f64,
) -> Option<[f64; 2]> {
    let placement = target.placement()?;
    let center = element_center(geometry, target)?;
    let rotate = |p: [f64; 2]| collision::rotate_unless_zero(p, center, placement.angle);
    let (x, y, w, h) = (placement.x, placement.y, placement.width, placement.height);
    let side_midpoints: Vec<[f64; 2]> = if let Element::Diamond(diamond) = target {
        let (_, corners) = collision::deconstruct_diamond(placement, &diamond.base.roundness);
        corners
            .iter()
            .map(|&curve| rotate(crate::curve::bezier_equation(curve, 0.5)))
            .collect()
    } else {
        vec![
            rotate([x + w, y + h / 2.0]),
            rotate([x + w / 2.0, y + h]),
            rotate([x, y + h / 2.0]),
            rotate([x + w / 2.0, y]),
        ]
    };
    let limit = max_binding_distance(zoom) + collision::stroke_width(target) / 2.0;
    side_midpoints.into_iter().find(|midpoint| {
        rough::js::hypot(point[0] - midpoint[0], point[1] - midpoint[1]) <= limit
            && !collision::hit_element_itself_with(geometry, target, point, 0.0, true)
    })
}

type Segment2 = [[f64; 2]; 2];

/// `getDiagonalsForBindableElement`: the two lines through `target` that
/// `projectFixedPointOntoDiagonal` aims at (corner to corner for rectangular shapes, the
/// center lines for the others), a rectangle's shrunk by 15 at each end.
fn diagonals_for(geometry: &mut GeometryCache, target: &Element) -> Option<[Segment2; 2]> {
    let placement = target.placement()?;
    let center = element_center(geometry, target)?;
    let offset_px = if matches!(target, Element::Rectangle(_)) {
        15.0
    } else {
        0.0
    };
    let shrink = |segment: Segment2| -> Segment2 {
        let v = [segment[1][0] - segment[0][0], segment[1][1] - segment[0][1]];
        let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let unit = if magnitude == 0.0 {
            [0.0, 0.0]
        } else {
            [v[0] / magnitude, v[1] / magnitude]
        };
        let offset = [unit[0] * offset_px, unit[1] * offset_px];
        [
            [segment[0][0] + offset[0], segment[0][1] + offset[1]],
            [segment[1][0] - offset[0], segment[1][1] - offset[1]],
        ]
    };
    let rotate = |p: [f64; 2]| collision::rotate_unless_zero(p, center, placement.angle);
    let (x, y, w, h) = (placement.x, placement.y, placement.width, placement.height);
    let rectangular = matches!(
        target,
        Element::Rectangle(_) | Element::Image(_) | Element::Text(_)
    );
    let (one, two) = if rectangular {
        (
            [rotate([x, y]), rotate([x + w, y + h])],
            [rotate([x + w, y]), rotate([x, y + h])],
        )
    } else {
        (
            [rotate([x + w / 2.0, y]), rotate([x + w / 2.0, y + h])],
            [rotate([x, y + h / 2.0]), rotate([x + w, y + h / 2.0])],
        )
    };
    Some([shrink(one), shrink(two)])
}

/// `projectFixedPointOntoDiagonal`: where the line from `arrow`'s neighbouring point (or the
/// other end's focus point, for a two-point arrow) through `point` crosses one of `target`'s
/// diagonals, when that is inside `target`; the side midpoint near `point` wins when
/// `midpoint_snapping`. `None` for a point-sized arrow or no usable crossing.
#[expect(
    clippy::too_many_arguments,
    reason = "the JS function's own parameters plus the geometry cache and the element list"
)]
pub fn project_fixed_point_onto_diagonal(
    geometry: &mut GeometryCache,
    elements: &[Element],
    arrow: &Element,
    point: [f64; 2],
    target: &Element,
    end: LinearEnd,
    zoom: f64,
    midpoint_snapping: bool,
) -> Option<[f64; 2]> {
    let Element::Arrow(l) = arrow else {
        return None;
    };
    if l.points.len() < 2 || (l.base.width < 3.0 && l.base.height < 3.0) {
        return None;
    }
    if midpoint_snapping
        && let Some(midpoint) = snap_outline_mid_point(geometry, point, target, zoom)
    {
        return Some(midpoint);
    }

    let [diagonal_one, diagonal_two] = diagonals_for(geometry, target)?;
    // The neighbouring point stays put while `end` moves; a two-point arrow takes the other
    // end's focus point instead, which the other end's own binding fixes.
    let mut a = point_at_index_global(
        geometry,
        arrow,
        match end {
            LinearEnd::Start => 1,
            LinearEnd::End => l.points.len() as isize - 2,
        },
    )?;
    if l.points.len() == 2
        && let BindingState::Fixed(other) = read_binding(arrow, opposite(end))
        && let Some(other_bindable) = live_element(elements, &other.element_id)
        && let Some(other_focus) = global_fixed_point(other.fixed_point, other_bindable)
    {
        a = other_focus;
    }

    let distance = |p: [f64; 2], q: [f64; 2]| rough::js::hypot(q[0] - p[0], q[1] - p[1]);
    let scale = 2.0 * distance(a, point)
        + distance(diagonal_one[0], diagonal_one[1])
            .max(distance(diagonal_two[0], diagonal_two[1]));
    let b = [
        a[0] + (point[0] - a[0]) * scale,
        a[1] + (point[1] - a[1]) * scale,
    ];
    let intersector = [b, a];
    let p1 = collision::line_segment_intersection_points(diagonal_one, intersector);
    let p2 = collision::line_segment_intersection_points(diagonal_two, intersector);
    let projection = match (p1, p2) {
        (Some(first), Some(second)) => {
            if distance(a, first) < distance(a, second) {
                Some(first)
            } else {
                Some(second)
            }
        }
        (first, second) => first.or(second),
    };
    projection.filter(|&p| collision::is_point_in_element(geometry, target, p))
}

/// A line or arrow's points (empty for anything else).
fn points_of(element: &Element) -> &[[f64; 2]] {
    match element {
        Element::Line(l) | Element::Arrow(l) => &l.points,
        _ => &[],
    }
}

/// `arrow` with `points` swapped in, for the simulated "next arrow" the strategy code asks
/// `updateBoundPoint` about. Version fields stay as they were, so a geometry cache must not be
/// shared with the real arrow.
fn with_points(arrow: &Element, points: Vec<[f64; 2]>) -> Element {
    let mut next = arrow.clone();
    if let Element::Line(l) | Element::Arrow(l) = &mut next {
        l.points = points;
    }
    next
}

/// `getBindingStrategyForDraggingBindingElementEndpoints_simple` for `arrow` while the points
/// in `dragged` (index, local position) move: the strategies for the (start, end) ends.
/// Anything but a typed, non-elbow arrow with two points keeps its bindings.
fn binding_strategy(
    geometry: &mut GeometryCache,
    elements: &[Element],
    arrow: &Element,
    dragged: &[(usize, [f64; 2])],
    input: BindingInput,
    state: &LinearDragState,
    new_arrow: bool,
) -> (Strategy, Strategy) {
    const KEEP: (Strategy, Strategy) = (Strategy::Keep, Strategy::Keep);
    let Element::Arrow(l) = arrow else {
        return KEEP;
    };
    if l.points.len() < 2 {
        return KEEP;
    }
    let last = l.points.len() - 1;
    let start_dragged = dragged.iter().any(|&(index, _)| index == 0);
    let end_dragged = dragged.iter().any(|&(index, _)| index == last);
    if !start_dragged && !end_dragged {
        return KEEP;
    }
    if start_dragged && end_dragged {
        return (Strategy::Break, Strategy::Break);
    }
    if !input.enabled {
        let pick = |is_dragged| {
            if is_dragged {
                Strategy::Break
            } else {
                Strategy::Keep
            }
        };
        return (pick(start_dragged), pick(end_dragged));
    }
    if l.elbowed == Some(true) {
        return KEEP;
    }

    let (dragged_end, other_end) = if start_dragged {
        (LinearEnd::Start, LinearEnd::End)
    } else {
        (LinearEnd::End, LinearEnd::Start)
    };
    let dragged_index = if start_dragged { 0 } else { last };
    let Some(&(_, local)) = dragged.iter().find(|&&(index, _)| index == dragged_index) else {
        return KEEP;
    };
    let Some(global_point) = local_to_global(geometry, arrow, local) else {
        return KEEP;
    };

    let probe = if input.angle_locked {
        input.pointer
    } else {
        global_point
    };
    let hit = collision::hovered_element_for_binding(
        geometry,
        elements,
        probe,
        max_binding_distance(input.zoom),
    );
    let point_in_element =
        hit.is_some_and(|hit| collision::is_point_in_element(geometry, &elements[hit], probe));

    let other_binding = match read_binding(arrow, other_end) {
        BindingState::Fixed(binding) => Some(binding),
        _ => None,
    };
    let other_bindable = other_binding
        .as_ref()
        .and_then(|binding| live_bindable_position(elements, &binding.element_id));
    let other_focus = other_binding
        .as_ref()
        .zip(other_bindable)
        .and_then(|(binding, at)| global_fixed_point(binding.fixed_point, &elements[at]));

    // Outside-outside (or inside) binding to the same element on both ends.
    if let (Some(other), Some(hit)) = (&other_binding, hit)
        && elements[hit].id() == Some(other.element_id.as_str())
        && (!new_arrow || state.origin.is_some())
    {
        let other_fixed = other_bindable.zip(other_focus).map(|(_, focus)| focus);
        let end_point = |index: isize, geometry: &mut GeometryCache| {
            point_at_index_global(geometry, arrow, index)
        };
        let start_focus = if start_dragged {
            Some(global_point)
        } else if new_arrow {
            state.origin
        } else {
            other_fixed.or_else(|| end_point(0, geometry))
        };
        let end_focus = if end_dragged {
            Some(global_point)
        } else {
            other_fixed.or_else(|| end_point(-1, geometry))
        };
        let (Some(start_focus), Some(end_focus)) = (start_focus, end_focus) else {
            return KEEP;
        };
        let inside = |focus| Strategy::Bind {
            mode: BindMode::Inside,
            target: hit,
            focus,
        };
        return (inside(start_focus), inside(end_focus));
    }

    let current = match hit {
        None => Strategy::Break,
        Some(hit) if point_in_element => Strategy::Bind {
            mode: BindMode::Inside,
            target: hit,
            focus: global_point,
        },
        Some(hit) => Strategy::Bind {
            mode: BindMode::Orbit,
            target: hit,
            focus: project_fixed_point_onto_diagonal(
                geometry,
                elements,
                arrow,
                global_point,
                &elements[hit],
                dragged_end,
                input.zoom,
                !input.angle_locked,
            )
            .unwrap_or(global_point),
        },
    };

    let other_endpoint = point_at_index_global(geometry, arrow, if start_dragged { -1 } else { 0 });
    let point_is_close_to_other_element = match (other_focus, other_bindable) {
        (Some(_), Some(at)) => collision::hit_element_itself_with(
            geometry,
            &elements[at],
            global_point,
            max_binding_distance(input.zoom),
            true,
        ),
        _ => false,
    };
    let other_focus_is_in_element = match (other_focus, other_bindable) {
        (Some(focus), Some(at)) => {
            collision::hit_element_itself_with(geometry, &elements[at], focus, 0.0, true)
        }
        _ => false,
    };
    let other_never_override = if new_arrow {
        // `initialState.arrowStartIsInside` is only ever set by the Alt and delayed-bind-mode
        // paths, which napkin does not have.
        false
    } else {
        other_binding
            .as_ref()
            .is_some_and(|binding| binding.mode == "inside")
            && state.other_end_started_inside
    };

    let mut other = Strategy::Keep;
    if !other_never_override {
        let other_inside = other_binding
            .as_ref()
            .is_some_and(|binding| binding.mode == "inside");
        if let Some(at) = other_bindable {
            if other_inside && !state.other_end_started_inside && !new_arrow {
                if let Some(focus) = other_focus.or(other_endpoint) {
                    other = Strategy::Bind {
                        mode: BindMode::Orbit,
                        target: at,
                        focus,
                    };
                }
            } else if !other_focus_is_in_element
                && !point_is_close_to_other_element
                && let Some(focus) = state.alt_focus_point
            {
                other = Strategy::Bind {
                    mode: BindMode::Orbit,
                    target: at,
                    focus,
                };
            } else if input.angle_locked
                && let Some(endpoint) = other_endpoint
            {
                other = Strategy::Bind {
                    mode: BindMode::Orbit,
                    target: at,
                    focus: project_fixed_point_onto_diagonal(
                        geometry,
                        elements,
                        arrow,
                        endpoint,
                        &elements[at],
                        other_end,
                        input.zoom,
                        true,
                    )
                    .unwrap_or(endpoint),
                };
            }
        }
    }

    if start_dragged {
        (current, other)
    } else {
        (other, current)
    }
}

/// A new `{elementId, mode, fixedPoint}` binding to `target`, with the fixed point of
/// `focus` on `target`.
fn new_binding(
    arrow: &Element,
    end: LinearEnd,
    mode: BindMode,
    target: &Element,
    focus: [f64; 2],
) -> Option<Value> {
    let fixed_point = fixed_point_for(arrow, end, target, Some(focus))?;
    Some(json!({"elementId": target.id()?, "mode": mode.as_str(), "fixedPoint": fixed_point}))
}

/// After an end of the arrow at `arrow` stopped binding `old_target` (cleared or rebound),
/// removes the arrow from that element's `boundElements`, unless an end of the arrow still binds
/// it. Excalidraw leaves the stale record behind until the shape moves; napkin keeps both sides
/// of a binding consistent (spec §5.8). Bumps the element only if it changed.
fn release_old_target(
    file: &mut SceneFile,
    arrow: usize,
    old_target: Option<&str>,
    env: &mut impl Env,
) {
    let Some(old_target) = old_target else {
        return;
    };
    let still_bound = [LinearEnd::Start, LinearEnd::End]
        .into_iter()
        .any(|end| file.elements[arrow].binding_target(end) == Some(old_target));
    let Some(arrow_id) = file.elements[arrow].id().map(str::to_owned) else {
        return;
    };
    if still_bound {
        return;
    }
    let Some(target) = file
        .elements
        .iter()
        .position(|e| e.id() == Some(old_target))
    else {
        return;
    };
    let before = file.elements[target].clone();
    file.elements[target].remove_bound_element(&arrow_id);
    if file.elements[target] != before {
        bump_version(&mut file.elements[target], env);
    }
}

/// `pointDraggingUpdates` + `movePoints` + the `altFocusPoint`/`suggestedBinding` bookkeeping
/// of `handlePointDragging`/`handlePointerMove`: moves `indices` of the arrow at `arrow` by
/// `delta` (local units, already shift-locked by the caller), rebinding or unbinding the
/// dragged end (and dropping the arrow from the previous target's `boundElements`, which
/// Excalidraw leaves stale), and moving bound non-dragged ends with `update_bound_point`. A line just moves its points.
/// When one end is dragged the arrow moves above the shape under it, so its position in
/// `file.elements` can change: look it up by id afterwards. Arrow labels are the caller's to
/// resync (`bound_text::sync_arrow_label`).
#[expect(
    clippy::too_many_arguments,
    reason = "one gesture step: the scene, the arrow, the dragged points, and three kinds of input"
)]
pub fn drag_points(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    indices: &[usize],
    delta: [f64; 2],
    input: BindingInput,
    state: &mut LinearDragState,
    env: &mut impl Env,
) -> Option<SuggestedBinding> {
    let is_arrow = match &file.elements[arrow] {
        Element::Arrow(l) if l.elbowed != Some(true) => true,
        Element::Line(_) => false,
        _ => return None,
    };
    let points = points_of(&file.elements[arrow]).to_vec();
    if points.len() < 2 {
        return None;
    }
    let last = points.len() - 1;
    let mut selected: Vec<usize> = Vec::new();
    for &index in indices {
        if index <= last && !selected.contains(&index) {
            selected.push(index);
        }
    }
    let naive: Vec<(usize, [f64; 2])> = selected
        .iter()
        .map(|&index| {
            (
                index,
                [points[index][0] + delta[0], points[index][1] + delta[1]],
            )
        })
        .collect();
    if !is_arrow {
        move_points(&mut file.elements[arrow], &naive, false, env);
        return None;
    }

    let start_dragged = selected.contains(&0);
    let end_dragged = selected.contains(&last);
    let arrow_before = file.elements[arrow].clone();
    let (start, end) = binding_strategy(
        geometry,
        &file.elements,
        &arrow_before,
        &naive,
        input,
        state,
        state.new_arrow,
    );

    // `None`: leave the binding alone; `Some(None)`: set it to null.
    let mut binding_updates: [Option<Option<Value>>; 2] = [None, None];
    let mut update_suggestion: Option<SuggestedBinding> = None;
    let mut positions = naive.clone();
    // The simulated arrow's geometry is not the cached real arrow's.
    let mut scratch = GeometryCache::default();

    if !start_dragged && !end_dragged {
        // Neither end moves, but bound ends follow the points that do.
        let next = with_points(
            &arrow_before,
            points
                .iter()
                .enumerate()
                .map(|(index, &p)| {
                    naive
                        .iter()
                        .find(|&&(i, _)| i == index)
                        .map_or(p, |&(_, moved)| moved)
                })
                .collect(),
        );
        for (end, index) in [(LinearEnd::Start, 0), (LinearEnd::End, last)] {
            let Some(target) = arrow_before
                .binding_target(end)
                .and_then(|id| live_bindable_position(&file.elements, id))
            else {
                continue;
            };
            if let Some(point) = update_bound_point(
                &mut scratch,
                &file.elements,
                &next,
                end,
                &file.elements[target],
                false,
            ) {
                positions.retain(|&(i, _)| i != index);
                positions.push((index, point));
            }
        }
    } else if start_dragged != end_dragged {
        let grabbed = [
            input.pointer[0] - state.pointer_offset[0],
            input.pointer[1] - state.pointer_offset[1],
        ];
        for (which, strategy, is_dragged, slot) in [
            (LinearEnd::Start, start, start_dragged, 0),
            (LinearEnd::End, end, end_dragged, 1),
        ] {
            match strategy {
                Strategy::Keep => {}
                Strategy::Break => binding_updates[slot] = Some(None),
                Strategy::Bind {
                    mode,
                    target,
                    focus,
                } => {
                    let Some(binding) =
                        new_binding(&arrow_before, which, mode, &file.elements[target], focus)
                    else {
                        continue;
                    };
                    binding_updates[slot] = Some(Some(binding));
                    if is_dragged {
                        update_suggestion = Some(SuggestedBinding {
                            target,
                            mid_point: snap_outline_mid_point(
                                geometry,
                                grabbed,
                                &file.elements[target],
                                input.zoom,
                            ),
                        });
                    }
                }
            }
        }

        // The arrow as it will be, so `update_bound_point` sees the dragged end and the new
        // bindings; the non-dragged bound end follows.
        let mut next_points = points.clone();
        if start_dragged {
            next_points[0] = [points[0][0] + delta[0], points[0][1] + delta[1]];
        }
        if end_dragged {
            next_points[last] = [points[last][0] + delta[0], points[last][1] + delta[1]];
        }
        let mut next = with_points(&arrow_before, next_points);
        for (which, update) in [LinearEnd::Start, LinearEnd::End]
            .into_iter()
            .zip(&binding_updates)
        {
            match update {
                Some(Some(binding)) => next.set_binding(which, binding.clone()),
                Some(None) => next.clear_binding(which),
                None => {}
            }
        }
        let bindable = |next: &Element, which: LinearEnd| {
            next.binding_target(which)
                .and_then(|id| live_bindable_position(&file.elements, id))
        };
        let end_bindable = bindable(&next, LinearEnd::End);
        let end_local = match end_bindable {
            Some(target) => update_bound_point(
                &mut scratch,
                &file.elements,
                &next,
                LinearEnd::End,
                &file.elements[target],
                end_dragged,
            ),
            None => None,
        }
        .unwrap_or(points_of(&next)[last]);
        if let Element::Arrow(l) = &mut next {
            l.points[last] = end_local;
        }
        let start_bindable = bindable(&next, LinearEnd::Start);
        let start_local = match start_bindable {
            Some(target) => update_bound_point(
                &mut scratch,
                &file.elements,
                &next,
                LinearEnd::Start,
                &file.elements[target],
                start_dragged,
            ),
            None => None,
        }
        .unwrap_or(points_of(&next)[0]);
        let start_changed = rough::js::hypot(
            start_local[0] - points_of(&next)[0][0],
            start_local[1] - points_of(&next)[0][1],
        ) != 0.0;

        let mut moved = selected.clone();
        if start_bindable.is_some() && start_changed && !moved.contains(&0) {
            moved.push(0);
        }
        if end_bindable.is_some() && !moved.contains(&last) {
            moved.push(last);
        }
        positions = moved
            .into_iter()
            .map(|index| {
                let point = if index == 0 {
                    start_local
                } else if index == last {
                    end_local
                } else {
                    naive
                        .iter()
                        .find(|&&(i, _)| i == index)
                        .map_or(points[index], |&(_, p)| p)
                };
                (index, point)
            })
            .collect();
    }

    for (which, update) in [LinearEnd::Start, LinearEnd::End]
        .into_iter()
        .zip(&binding_updates)
    {
        let before = file.elements[arrow].clone();
        let old_target = before.binding_target(which).map(str::to_owned);
        match update {
            Some(Some(binding)) => file.elements[arrow].set_binding(which, binding.clone()),
            Some(None) => file.elements[arrow].clear_binding(which),
            None => {}
        }
        if file.elements[arrow] != before {
            bump_version(&mut file.elements[arrow], env);
        }
        if update.is_some() {
            release_old_target(file, arrow, old_target.as_deref(), env);
        }
    }
    move_points(&mut file.elements[arrow], &positions, false, env);

    let suggestion_id =
        update_suggestion.and_then(|s| file.elements[s.target].id().map(str::to_owned));

    if start_dragged != end_dragged {
        let grabbed_local = points_of(&file.elements[arrow])[if start_dragged { 0 } else { last }];
        if let Some(global) = local_to_global(geometry, &file.elements[arrow], grabbed_local) {
            crate::zindex::move_arrow_above_bindable(geometry, file, arrow, global, env);
        }
    }

    // Raising the arrow shifted the positions of the shapes it passed.
    let update_suggestion = update_suggestion.zip(suggestion_id).and_then(|(s, id)| {
        let target = file
            .elements
            .iter()
            .position(|e| e.id() == Some(id.as_str()))?;
        Some(SuggestedBinding { target, ..s })
    });
    let suggestion = if input.enabled && (start_dragged || end_dragged) {
        update_suggestion
    } else {
        None
    };

    // The first time the dragged end leaves the shape the other end is bound to, remember the
    // diagonal point the other end should keep aiming at.
    let moved_arrow_at = file
        .elements
        .iter()
        .position(|e| e.id() == arrow_before.id())
        .unwrap_or(arrow);
    let after = &file.elements[moved_arrow_at];
    let bound_target = |which: LinearEnd| {
        after
            .binding_target(which)
            .and_then(|id| live_bindable_position(&file.elements, id))
    };
    let suggested_target = update_suggestion.map(|s| s.target);
    let alt_target = match (bound_target(LinearEnd::Start), bound_target(LinearEnd::End)) {
        (Some(start_target), _) if end_dragged && suggested_target != Some(start_target) => {
            Some(start_target)
        }
        (_, Some(end_target)) if start_dragged && suggested_target != Some(end_target) => {
            Some(end_target)
        }
        _ => None,
    };
    if state.alt_focus_point.is_none()
        && let Some(target) = alt_target
        && let Some(placement) = after.placement()
    {
        state.alt_focus_point = project_fixed_point_onto_diagonal(
            geometry,
            &file.elements,
            after,
            [placement.x, placement.y],
            &file.elements[target],
            LinearEnd::Start,
            input.zoom,
            !input.angle_locked,
        );
    }

    suggestion
}

/// `bindOrUnbindBindingElement` with the dragged points `dragged` (index, local point): binds,
/// rebinds or unbinds each end the strategy names, keeping both sides of every binding the
/// strategy touches in step, then snaps ends whose strategy carried a focus point. `initial`
/// is the creation-time call on pointer-down (`initialBinding: true`).
#[expect(
    clippy::too_many_arguments,
    reason = "one gesture step: the scene, the arrow, the dragged points, and three kinds of input"
)]
pub fn finalize_binding(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    arrow: usize,
    dragged: &[(usize, [f64; 2])],
    input: BindingInput,
    state: &LinearDragState,
    initial: bool,
    env: &mut impl Env,
) {
    let new_arrow = state.new_arrow || initial;
    let (start, end) = binding_strategy(
        geometry,
        &file.elements,
        &file.elements[arrow],
        dragged,
        input,
        state,
        new_arrow,
    );

    for (which, strategy) in [(LinearEnd::Start, start), (LinearEnd::End, end)] {
        match strategy {
            Strategy::Keep => {}
            Strategy::Break => {
                let ids: Vec<String> = file
                    .elements
                    .iter()
                    .map(|e| e.id().unwrap_or_default().to_owned())
                    .collect();
                let id_to_pos: HashMap<&str, usize> = file
                    .elements
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.is_deleted())
                    .map(|(i, _)| (ids[i].as_str(), i))
                    .collect();
                crate::edit::unbind_arrow_end(file, &id_to_pos, arrow, which, env);
            }
            Strategy::Bind {
                mode,
                target,
                focus,
            } => {
                bind_arrow(file, arrow, which, target, mode.as_str(), Some(focus), env);
            }
        }
    }

    // A strategy with a focus point overrides the arrow's own point with the bound position.
    let points = points_of(&file.elements[arrow]).to_vec();
    let mut updates: Vec<(usize, [f64; 2])> = Vec::new();
    for (which, strategy, index) in [
        (LinearEnd::Start, start, 0),
        (LinearEnd::End, end, points.len().saturating_sub(1)),
    ] {
        let Strategy::Bind { target, .. } = strategy else {
            continue;
        };
        let Some(&current) = points.get(index) else {
            continue;
        };
        let point = update_bound_point(
            geometry,
            &file.elements,
            &file.elements[arrow],
            which,
            &file.elements[target],
            false,
        )
        .unwrap_or(current);
        updates.push((index, point));
    }
    if !updates.is_empty() {
        move_points(&mut file.elements[arrow], &updates, false, env);
    }
}

/// The arrow tool's hover suggestion (App.tsx's `isBindingElementType` hover branch).
pub fn hover_suggestion(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    zoom: f64,
) -> Option<SuggestedBinding> {
    let target = collision::hovered_element_for_binding(
        geometry,
        elements,
        point,
        max_binding_distance(zoom),
    )?;
    Some(SuggestedBinding {
        target,
        mid_point: snap_outline_mid_point(geometry, point, &elements[target], zoom),
    })
}

/// App.tsx's multi-point pointer-down decision for an arrow being created by clicking points:
/// whether a click at `click` (scene) ends the arrow now. It does when the last point lands
/// in the orbit zone of a shape other than the one the arrow starts at, when both ends bind
/// outside the same shape, or when the click is within `LINE_CONFIRM_THRESHOLD` of
/// `last_committed` (the last point the user confirmed, local). `state` is the gesture's
/// state from creation.
#[expect(
    clippy::too_many_arguments,
    reason = "one pointer-down: the scene, the arrow, the click, and the gesture's state"
)]
pub fn multi_point_click_finishes(
    geometry: &mut GeometryCache,
    elements: &[Element],
    arrow: usize,
    click: [f64; 2],
    zoom: f64,
    enabled: bool,
    last_committed: Option<[f64; 2]>,
    state: &LinearDragState,
) -> bool {
    let Element::Arrow(l) = &elements[arrow] else {
        return false;
    };
    let Some(&last_point) = l.points.last() else {
        return false;
    };
    if l.elbowed == Some(true) {
        return l.points.len() > 1;
    }
    let (start, end) = if enabled {
        binding_strategy(
            geometry,
            elements,
            &elements[arrow],
            &[(l.points.len() - 1, last_point)],
            BindingInput {
                pointer: click,
                zoom,
                angle_locked: false,
                enabled,
            },
            state,
            true,
        )
    } else {
        (Strategy::Keep, Strategy::Keep)
    };

    let end_outside_same_element = matches!(
        (start, end),
        (Strategy::Bind { target: a, .. }, Strategy::Bind { target: b, focus, .. })
            if a == b && !collision::is_point_in_element(geometry, &elements[b], focus)
    );
    let bound_outside_from_elsewhere = matches!(
        end,
        Strategy::Bind { mode: BindMode::Orbit, target, .. }
            if elements[arrow].binding_target(LinearEnd::Start) != elements[target].id()
    );
    let in_commit_zone = last_committed.is_some_and(|committed| {
        rough::js::hypot(
            click[0] - l.base.x - committed[0],
            click[1] - l.base.y - committed[1],
        ) < collision::LINE_CONFIRM_THRESHOLD
    });
    bound_outside_from_elsewhere
        || end_outside_same_element
        || (l.points.len() > 1 && in_commit_zone)
}

/// `repairBinding` for every typed non-elbow arrow, run by `edit::repair_on_load`: a binding
/// with `mode` gets its `fixedPoint` normalized; a legacy one (no `mode`) whose target exists
/// becomes `{elementId, mode, fixedPoint}`; one that cannot be repaired, or whose target is
/// missing, becomes `null`.
pub fn repair_bindings(geometry: &mut GeometryCache, file: &mut SceneFile, env: &mut impl Env) {
    for position in 0..file.elements.len() {
        let Element::Arrow(l) = &file.elements[position] else {
            continue;
        };
        if l.elbowed == Some(true) {
            continue;
        }
        // Both ends repair against the arrow as loaded, so compute before writing either.
        let repaired: Vec<(LinearEnd, Value, Option<Value>)> = [LinearEnd::Start, LinearEnd::End]
            .into_iter()
            .filter_map(|end| {
                let existing = file.elements[position].binding_value(end)?.clone();
                let repaired = repair_binding(geometry, file, position, end, &existing);
                (repaired.as_ref() != Some(&existing)).then_some((end, existing, repaired))
            })
            .collect();
        if repaired.is_empty() {
            continue;
        }
        let mut dropped: Vec<String> = Vec::new();
        for (end, existing, repaired) in repaired {
            match repaired {
                Some(binding) => file.elements[position].set_binding(end, binding),
                None => {
                    file.elements[position].clear_binding(end);
                    dropped.extend(
                        existing
                            .get("elementId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    );
                }
            }
        }
        bump_version(&mut file.elements[position], env);
        for target_id in dropped {
            // A Raw element cannot be edited, so its record stays; so does the record of a
            // target the arrow still binds with its other end.
            let still_bound = [LinearEnd::Start, LinearEnd::End]
                .into_iter()
                .any(|end| file.elements[position].binding_target(end) == Some(target_id.as_str()));
            let Some(target) = file
                .elements
                .iter()
                .position(|e| e.id() == Some(target_id.as_str()))
            else {
                continue;
            };
            if still_bound
                || matches!(file.elements[target], Element::Raw(_))
                || collision::is_bindable_including_locked(&file.elements[target])
            {
                continue;
            }
            let arrow_id = file.elements[position].id().unwrap_or_default().to_owned();
            let before = file.elements[target].clone();
            file.elements[target].remove_bound_element(&arrow_id);
            if file.elements[target] != before {
                bump_version(&mut file.elements[target], env);
            }
        }
    }
}

/// `repairBinding` for one end of the arrow at `position`.
fn repair_binding(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    position: usize,
    end: LinearEnd,
    binding: &Value,
) -> Option<Value> {
    let element_id = binding
        .get("elementId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let normalized = |binding: &Value| {
        let point = binding
            .get("fixedPoint")
            .and_then(Value::as_array)
            .and_then(|a| match a.as_slice() {
                [x, y] => Some([x.as_f64()?, y.as_f64()?]),
                _ => None,
            })
            .unwrap_or([f64::NAN, f64::NAN]);
        normalize_fixed_point(point)
    };

    // Binding schema v2.
    if let Some(mode) = binding
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| !mode.is_empty())
    {
        if !file.elements.iter().any(|e| e.id() == Some(element_id)) {
            return None;
        }
        return Some(json!({
            "elementId": element_id,
            "mode": mode,
            "fixedPoint": normalized(binding),
        }));
    }

    // Binding schema v1: recompute the fixed point from where the end sits now.
    let target = file
        .elements
        .iter()
        .position(|e| e.id() == Some(element_id))
        .filter(|&at| collision::is_bindable_including_locked(&file.elements[at]))?;
    let arrow = &file.elements[position];
    let end_index = match end {
        LinearEnd::Start => 0,
        LinearEnd::End => -1,
    };
    let at = point_at_index_global(geometry, arrow, end_index)?;
    let target_element = &file.elements[target];
    let mode = if collision::is_point_in_element(geometry, target_element, at) {
        BindMode::Inside
    } else {
        BindMode::Orbit
    };

    // `safeElement`: both bindings in the new format, carrying this end's mode.
    let mut safe = arrow.clone();
    for which in [LinearEnd::Start, LinearEnd::End] {
        match arrow.binding_value(which) {
            Some(b) => match b
                .get("elementId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            {
                Some(id) => safe.set_binding(
                    which,
                    json!({"elementId": id, "mode": mode.as_str(), "fixedPoint": normalized(b)}),
                ),
                None => safe.clear_binding(which),
            },
            None => safe.clear_binding(which),
        }
    }
    let focus = match mode {
        BindMode::Inside => at,
        BindMode::Orbit => project_fixed_point_onto_diagonal(
            geometry,
            &file.elements,
            &safe,
            at,
            target_element,
            end,
            1.0,
            true,
        )
        .unwrap_or(at),
    };
    new_binding(&safe, end, mode, target_element, focus)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::f64::consts::FRAC_PI_2;

    use serde_json::{Value, json};

    use super::*;
    use crate::geometry::GeometryCache;
    use crate::sample::{self, CharWidthMeasure};

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(0);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    #[test]
    fn normalize_fixed_point_handles_nan_bounds_and_near_half() {
        assert_eq!(normalize_fixed_point([f64::NAN, 0.2]), [0.5001, 0.5001]);
        assert_eq!(
            normalize_fixed_point([f64::INFINITY, 0.2]),
            [0.5001, 0.5001]
        );
        assert_eq!(normalize_fixed_point([20.0, -20.0]), [10.0, -10.0]);
        assert_eq!(normalize_fixed_point([0.50005, 0.2]), [0.5001, 0.2]);
        assert_eq!(normalize_fixed_point([0.3, 0.49995]), [0.3, 0.5001]);
        assert_eq!(normalize_fixed_point([0.1, 0.9]), [0.1, 0.9]);
    }

    #[test]
    fn fixed_point_for_a_rotated_target_uses_its_unrotated_frame() {
        // A 100x100 target centered at (50, 50), rotated 90°: the arrow's end point sits at
        // its own (100, 50) in scene coordinates, which is the target's own (unrotated) top
        // edge midpoint, i.e. ratio (0.5, 0).
        let arrow = Element::from_value(sample::linear(
            "arrow",
            "a",
            [0.0, 0.0],
            &[[0.0, 0.0], [100.0, 50.0]],
        ));
        let target = Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"angle": FRAC_PI_2, "strokeWidth": 0}),
        ));
        let [x, y] = fixed_point_for(&arrow, LinearEnd::End, &target, None).expect("fixed point");
        // The raw ratio is exactly (0.5, 0); `normalize_fixed_point` nudges the 0.5 away.
        assert!((x - 0.5001).abs() < 1e-9, "{x}");
        assert!(y.abs() < 1e-9, "{y}");
    }

    #[test]
    fn tiny_target_binds_to_its_center() {
        let arrow = Element::from_value(sample::linear(
            "arrow",
            "a",
            [0.0, 0.0],
            &[[0.0, 0.0], [10.0, 10.0]],
        ));
        let target = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 0.5, 20.0]));
        assert_eq!(
            fixed_point_for(&arrow, LinearEnd::End, &target, None),
            Some([0.5001, 0.5001])
        );
    }

    #[test]
    fn binding_both_ends_to_the_same_target_adds_it_once() {
        let mut file = sample::file(vec![
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 0.0]]),
            sample::generic("rectangle", "r", [100.0, -25.0, 50.0, 50.0]),
        ]);
        bind_arrow(
            &mut file,
            0,
            LinearEnd::Start,
            1,
            "orbit",
            None,
            &mut FixedEnv,
        );
        bind_arrow(
            &mut file,
            0,
            LinearEnd::End,
            1,
            "orbit",
            None,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1].bound_elements(), vec![("a", "arrow")]);
        assert!(file.elements[0].binding_target(LinearEnd::Start).is_some());
        assert!(file.elements[0].binding_target(LinearEnd::End).is_some());
    }

    /// `r`: 100x100 at (0, 0), strokeWidth 2 (gap 6). Arrow `a` starts bound to `r`'s center
    /// and, with `both_ends_on_r`, ends bound to its top edge.
    fn follow_scene(arrow_points: &[[f64; 2]], start: [f64; 2], both_ends_on_r: bool) -> SceneFile {
        let end_binding = if both_ends_on_r {
            json!({"elementId": "r", "mode": "orbit", "fixedPoint": [0.5001, 0.0]})
        } else {
            Value::Null
        };
        sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
                json!({"strokeWidth": 2, "boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", start, arrow_points),
                json!({
                    "startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [0.5001, 0.5001]},
                    "endBinding": end_binding,
                }),
            ),
        ])
    }

    #[test]
    fn moving_the_target_drags_the_bound_end_onto_its_outline() {
        let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
        file.elements[0].set_position(50.0, 0.0);
        let changed = update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert!(changed);
        let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
        // r now spans x 50..150; the start sits one gap (6) outside its right edge, on the line
        // toward the free end at (300, 50).
        assert!(
            (start[0] - 156.0).abs() < 0.05 && (start[1] - 50.0).abs() < 0.05,
            "{start:?}"
        );
        let end = linear_end_point(&file.elements[1], LinearEnd::End).unwrap();
        assert_eq!(end, [300.0, 50.0]);
    }

    #[test]
    fn an_arrow_moved_together_with_its_target_is_left_alone() {
        let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
        file.elements[0].set_position(50.0, 0.0);
        let before = file.elements[1].clone();
        let simultaneous = HashSet::from(["r".to_owned(), "a".to_owned()]);
        update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &simultaneous,
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1], before);
    }

    #[test]
    fn both_ends_on_one_target_follow_without_collapsing() {
        let mut file = follow_scene(&[[0.0, 0.0], [0.0, -56.0]], [50.0, 50.0], true);
        file.elements[0].set_position(200.0, 0.0);
        update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
        let end = linear_end_point(&file.elements[1], LinearEnd::End).unwrap();
        assert!(start.iter().chain(end.iter()).all(|v| v.is_finite()));
        assert!(start[0] > 190.0 && end[0] > 190.0, "{start:?} {end:?}");
    }

    #[test]
    fn a_degenerate_target_keeps_points_finite() {
        let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
        if let Element::Rectangle(r) = &mut file.elements[0] {
            r.base.width = 0.0;
            r.base.height = 0.0;
        }
        update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        let start = linear_end_point(&file.elements[1], LinearEnd::Start).unwrap();
        assert!(start[0].is_finite() && start[1].is_finite(), "{start:?}");
    }

    #[test]
    fn an_arrow_with_a_legacy_binding_is_left_alone() {
        let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
        file.elements[1].set_binding(
            LinearEnd::Start,
            json!({"elementId": "r", "focus": 0.0, "gap": 4.0}),
        );
        file.elements[0].set_position(50.0, 0.0);
        let before = file.elements[1].clone();
        let changed = update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert!(!changed);
        assert_eq!(file.elements[1], before);
    }

    fn drop_input(pointer: [f64; 2]) -> BindingInput {
        BindingInput {
            pointer,
            zoom: 1.0,
            angle_locked: false,
            enabled: true,
        }
    }

    #[test]
    fn dropping_a_new_arrow_end_near_a_shape_binds_it_on_both_sides() {
        // r: 100x100 at (200, 0). A new arrow from (0, 50) dragged to (195, 50), 5 units left
        // of r.
        let mut file = sample::file(vec![
            sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
            sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [0.0, 0.0]]),
        ]);
        let mut geometry = GeometryCache::default();
        let mut state = LinearDragState {
            new_arrow: true,
            origin: Some([0.0, 50.0]),
            ..Default::default()
        };
        let input = drop_input([195.0, 50.0]);
        let suggested = drag_points(
            &mut geometry,
            &mut file,
            1,
            &[1],
            [195.0, 0.0],
            input,
            &mut state,
            &mut FixedEnv,
        );
        assert_eq!(suggested.map(|s| s.target), Some(0));
        finalize_binding(
            &mut geometry,
            &mut file,
            1,
            &[(1, [195.0, 0.0])],
            input,
            &state,
            false,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1].binding_target(LinearEnd::End), Some("r"));
        assert_eq!(file.elements[0].bound_elements(), vec![("a", "arrow")]);
    }

    #[test]
    fn ctrl_held_drops_the_binding_of_the_dragged_end() {
        let mut file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [-0.06, 0.5001]}}),
            ),
        ]);
        let mut geometry = GeometryCache::default();
        let mut state = LinearDragState::default();
        let input = BindingInput {
            enabled: false,
            ..drop_input([190.0, 50.0])
        };
        drag_points(
            &mut geometry,
            &mut file,
            1,
            &[1],
            [-4.0, 0.0],
            input,
            &mut state,
            &mut FixedEnv,
        );
        finalize_binding(
            &mut geometry,
            &mut file,
            1,
            &[(1, [190.0, 0.0])],
            input,
            &state,
            false,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1].binding_target(LinearEnd::End), None);
        assert!(file.elements[0].bound_elements().is_empty());
    }

    #[test]
    fn rebinding_an_end_moves_the_record_from_the_old_shape_to_the_new_one() {
        let mut file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "a_shape", [200.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "w", "type": "arrow"}]}),
            ),
            sample::generic("rectangle", "b_shape", [200.0, 300.0, 100.0, 100.0]),
            sample::with(
                sample::linear("arrow", "w", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "a_shape", "mode": "orbit", "fixedPoint": [-0.06, 0.5001]}}),
            ),
        ]);
        let mut geometry = GeometryCache::default();
        let mut state = LinearDragState::default();
        // End point at (194, 50) -> (250, 294): 6 above b_shape's top edge.
        let input = drop_input([250.0, 294.0]);
        drag_points(
            &mut geometry,
            &mut file,
            2,
            &[1],
            [56.0, 244.0],
            input,
            &mut state,
            &mut FixedEnv,
        );
        finalize_binding(
            &mut geometry,
            &mut file,
            2,
            &[(1, [250.0, 244.0])],
            input,
            &state,
            false,
            &mut FixedEnv,
        );
        let arrow = file
            .elements
            .iter()
            .position(|e| e.id() == Some("w"))
            .unwrap();
        assert_eq!(
            file.elements[arrow].binding_target(LinearEnd::End),
            Some("b_shape")
        );
        assert!(file.elements[0].bound_elements().is_empty());
        assert_eq!(file.elements[1].bound_elements(), vec![("w", "arrow")]);
    }

    #[test]
    fn update_bound_point_on_an_arrow_without_points_is_none() {
        let target =
            Element::from_value(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]));
        for mode in ["orbit", "inside"] {
            let arrow = Element::from_value(sample::with(
                sample::linear("arrow", "a", [0.0, 0.0], &[]),
                json!({"endBinding": {"elementId": "r", "mode": mode, "fixedPoint": [1.06, 0.5001]}}),
            ));
            for end in [LinearEnd::Start, LinearEnd::End] {
                assert_eq!(
                    update_bound_point(
                        &mut GeometryCache::default(),
                        &[],
                        &arrow,
                        end,
                        &target,
                        false
                    ),
                    None,
                    "{mode} {end:?}"
                );
            }
        }
    }

    #[test]
    fn repair_drops_the_record_of_a_non_bindable_target_it_unbinds() {
        let mut file = sample::file(vec![
            sample::with(
                sample::text("t", [200.0, 0.0, 40.0, 20.0], "hi", Some("c")),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::generic("rectangle", "c", [100.0, 0.0, 200.0, 100.0]),
            sample::with(
                sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "t", "focus": 0, "gap": 6}}),
            ),
        ]);
        repair_bindings(&mut GeometryCache::default(), &mut file, &mut FixedEnv);
        assert_eq!(file.elements[2].to_value()["endBinding"], Value::Null);
        assert!(file.elements[0].bound_elements().is_empty());
    }

    #[test]
    fn unbinding_a_still_bound_end_removes_it_from_the_shape() {
        // Both ends dragged: the drag leaves the bindings alone and the gesture's end breaks
        // both through `unbindBindingElement`.
        let mut file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [-0.06, 0.5001]}}),
            ),
        ]);
        finalize_binding(
            &mut GeometryCache::default(),
            &mut file,
            1,
            &[(0, [0.0, 0.0]), (1, [194.0, 0.0])],
            drop_input([100.0, 50.0]),
            &LinearDragState::default(),
            false,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1].binding_target(LinearEnd::End), None);
        assert!(file.elements[0].bound_elements().is_empty());
    }

    #[test]
    fn a_legacy_binding_is_converted_on_load() {
        let mut file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "r", "focus": 0, "gap": 6}}),
            ),
        ]);
        repair_bindings(&mut GeometryCache::default(), &mut file, &mut FixedEnv);
        let binding = file.elements[1].to_value()["endBinding"].clone();
        assert_eq!(binding["elementId"], "r");
        assert_eq!(binding["mode"], "orbit");
        assert!(binding["fixedPoint"][0].as_f64().unwrap().is_finite());
        assert!(binding.get("focus").is_none());
    }

    #[test]
    fn repair_normalizes_current_bindings_and_nulls_unrepairable_ones() {
        let mut file = sample::file(vec![
            sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
            sample::with(
                sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({
                    "startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [0.50001, 99]},
                    "endBinding": {"elementId": "gone", "focus": 0, "gap": 6},
                }),
            ),
            sample::with(
                sample::linear("arrow", "b", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"endBinding": {"elementId": "missing", "mode": "orbit", "fixedPoint": [0.1, 0.2]}}),
            ),
        ]);
        repair_bindings(&mut GeometryCache::default(), &mut file, &mut FixedEnv);
        let a = file.elements[1].to_value();
        assert_eq!(a["startBinding"]["fixedPoint"], json!([0.5001, 10.0]));
        assert_eq!(a["endBinding"], Value::Null);
        assert_eq!(file.elements[2].to_value()["endBinding"], Value::Null);
    }

    #[test]
    fn repair_leaves_raw_and_elbow_arrows_alone() {
        let mut raw = sample::linear("arrow", "raw", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]);
        raw["endBinding"] = json!({"elementId": "r", "focus": 0, "gap": 6});
        raw["points"] = json!("not points");
        let mut file = sample::file(vec![
            sample::generic("rectangle", "r", [200.0, 0.0, 100.0, 100.0]),
            raw,
            sample::with(
                sample::linear("arrow", "elbow", [0.0, 50.0], &[[0.0, 0.0], [194.0, 0.0]]),
                json!({"elbowed": true, "endBinding": {"elementId": "r", "focus": 0, "gap": 6}}),
            ),
        ]);
        assert!(matches!(file.elements[1], Element::Raw(_)));
        let before = file.elements.clone();
        repair_bindings(&mut GeometryCache::default(), &mut file, &mut FixedEnv);
        assert_eq!(file.elements, before);
    }

    #[test]
    fn a_typed_arrow_without_points_never_panics() {
        let mut file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 0.0], &[]),
                json!({"startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [1.06, 0.5001]}}),
            ),
        ]);
        file.elements[0].set_position(50.0, 0.0);
        let before = file.elements[1].clone();
        let mut geometry = GeometryCache::default();
        update_bound_elements(
            &mut geometry,
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1], before);

        let mut state = LinearDragState::default();
        let input = drop_input([10.0, 10.0]);
        assert_eq!(
            drag_points(
                &mut geometry,
                &mut file,
                1,
                &[0],
                [5.0, 5.0],
                input,
                &mut state,
                &mut FixedEnv,
            ),
            None
        );
        finalize_binding(
            &mut geometry,
            &mut file,
            1,
            &[(0, [5.0, 5.0])],
            input,
            &state,
            true,
            &mut FixedEnv,
        );
        repair_bindings(&mut geometry, &mut file, &mut FixedEnv);
        assert_eq!(file.elements[1], before);
    }

    #[test]
    fn a_one_point_arrow_is_not_bound_or_moved() {
        let mut file = sample::file(vec![
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            sample::linear("arrow", "a", [50.0, 50.0], &[[0.0, 0.0]]),
        ]);
        let before = file.elements.clone();
        let mut state = LinearDragState::default();
        let input = drop_input([50.0, 50.0]);
        let mut geometry = GeometryCache::default();
        drag_points(
            &mut geometry,
            &mut file,
            1,
            &[0],
            [1.0, 1.0],
            input,
            &mut state,
            &mut FixedEnv,
        );
        finalize_binding(
            &mut geometry,
            &mut file,
            1,
            &[(0, [0.0, 0.0])],
            input,
            &state,
            true,
            &mut FixedEnv,
        );
        assert_eq!(file.elements, before);
    }

    #[test]
    fn binding_to_a_zero_size_shape_keeps_everything_finite() {
        let mut file = sample::file(vec![
            sample::generic("rectangle", "r", [200.0, 50.0, 0.0, 0.0]),
            sample::linear("arrow", "a", [0.0, 50.0], &[[0.0, 0.0], [0.0, 0.0]]),
        ]);
        let mut geometry = GeometryCache::default();
        let mut state = LinearDragState {
            new_arrow: true,
            origin: Some([0.0, 50.0]),
            ..Default::default()
        };
        let input = drop_input([198.0, 50.0]);
        drag_points(
            &mut geometry,
            &mut file,
            1,
            &[1],
            [198.0, 0.0],
            input,
            &mut state,
            &mut FixedEnv,
        );
        finalize_binding(
            &mut geometry,
            &mut file,
            1,
            &[(1, [198.0, 0.0])],
            input,
            &state,
            false,
            &mut FixedEnv,
        );
        let value = file.elements[1].to_value();
        assert!(
            value["endBinding"]["fixedPoint"]
                .as_array()
                .expect("bound")
                .iter()
                .all(|n| n.as_f64().is_some_and(f64::is_finite))
        );
        let end = linear_end_point(&file.elements[1], LinearEnd::End).unwrap();
        assert!(end.iter().all(|v| v.is_finite()), "{end:?}");
    }

    #[test]
    fn hover_suggestion_names_the_shape_within_the_binding_distance() {
        let file = sample::file(vec![sample::generic(
            "rectangle",
            "r",
            [200.0, 0.0, 100.0, 100.0],
        )]);
        let mut geometry = GeometryCache::default();
        let near = hover_suggestion(&mut geometry, &file.elements, [190.0, 50.0], 1.0);
        assert_eq!(near.map(|s| s.target), Some(0));
        // At zoom 0.5 the binding distance grows from 15 to 20.
        assert_eq!(
            hover_suggestion(&mut geometry, &file.elements, [182.0, 50.0], 1.0),
            None
        );
        assert_eq!(
            hover_suggestion(&mut geometry, &file.elements, [182.0, 50.0], 0.5).map(|s| s.target),
            Some(0)
        );
    }

    #[test]
    fn an_elbow_arrow_is_left_alone() {
        let mut file = follow_scene(&[[0.0, 0.0], [194.0, 0.0]], [106.0, 50.0], false);
        if let Element::Arrow(a) = &mut file.elements[1] {
            a.elbowed = Some(true);
        }
        file.elements[0].set_position(50.0, 0.0);
        let before = file.elements[1].clone();
        update_bound_elements(
            &mut GeometryCache::default(),
            &mut file,
            0,
            &HashSet::new(),
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert_eq!(file.elements[1], before);
    }
}
