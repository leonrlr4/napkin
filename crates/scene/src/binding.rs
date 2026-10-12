//! Arrow binding creation, ported at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` from
//! `packages/element/src/binding.ts`'s `bindBindingElement` (non-elbow branch), `applyBinding`,
//! `calculateFixedPointForNonElbowArrowBinding` (no focus point), `getBindingGap` and
//! `normalizeFixedPoint`; `packages/element/src/linearElementEditor.ts`'s
//! `getPointAtIndexGlobalCoordinates`; and `packages/element/src/bounds.ts`'s
//! `elementCenterPoint`/`getCenterForBounds`; and the arrows-follow-their-targets half:
//! `updateBoundElements`, `updateBoundPoint`, `extractBinding`,
//! `getGlobalFixedPointForBindableElement`, `boundElementsVisitor`/`bindableElementsVisitor`
//! and `LinearElementEditor.createPointAt`. Only arrows with the current binding format
//! (`mode` and `fixedPoint`) follow; an elbow arrow never does.

use std::collections::HashSet;

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

/// `calculateFixedPointForNonElbowArrowBinding` without a focus point: `arrow`'s `end` point,
/// expressed as a ratio of `target`'s unrotated bounds (`elementCenterPoint`/
/// `getCenterForBounds` is `target`'s axis-aligned [`element_bounds`] center, not its
/// unrotated placement center). `None` when either element's geometry cannot be read.
pub fn fixed_point_for(arrow: &Element, end: LinearEnd, target: &Element) -> Option<[f64; 2]> {
    let edge = linear_end_point(arrow, end)?;
    let placement = target.placement()?;
    let [x1, y1, x2, y2] = element_bounds(target)?;
    let center = [x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0];
    let unrotated = rotate_point(edge, center, -placement.angle);
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

/// `bindBindingElement(arrow, target, "orbit", end)` for a non-elbow arrow (`applyBinding`):
/// sets the arrow's binding to `{elementId, mode: "orbit", fixedPoint}` and, unless the target
/// already lists the arrow, appends it to the target's `boundElements`; each element that
/// changed gets a version bump (`scene.mutateElement` in `applyBinding`). A no-op when
/// `fixed_point_for` cannot compute a fixed point (an untyped or unplaceable arrow or target).
pub fn bind_arrow(
    file: &mut SceneFile,
    arrow: usize,
    end: LinearEnd,
    target: usize,
    env: &mut impl Env,
) {
    let Some(fixed_point) = fixed_point_for(&file.elements[arrow], end, &file.elements[target])
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
        json!({"elementId": target_id, "mode": "orbit", "fixedPoint": fixed_point}),
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
/// sits at scene point `global`.
fn create_point_at(geometry: &mut GeometryCache, arrow: &Element, global: [f64; 2]) -> [f64; 2] {
    let (_, center) = geometry
        .absolute_coords(arrow)
        .expect("a typed arrow with points");
    let placement = arrow.placement().expect("a typed arrow");
    let [x, y] = rotate_point(global, center, -placement.angle);
    [x - placement.x, y - placement.y]
}

/// `LinearElementEditor.getPointAtIndexGlobalCoordinates` for a typed arrow; a negative index
/// counts from the end. An index with no point gives the arrow's own origin, as JS does.
fn point_at_index_global(geometry: &mut GeometryCache, arrow: &Element, index: isize) -> [f64; 2] {
    let Element::Arrow(l) = arrow else {
        unreachable!("callers pass a typed arrow")
    };
    let index = if index < 0 {
        l.points.len() as isize + index
    } else {
        index
    };
    let (_, center) = geometry
        .absolute_coords(arrow)
        .expect("a typed arrow with points");
    let local = usize::try_from(index)
        .ok()
        .and_then(|i| l.points.get(i))
        .copied()
        .unwrap_or([0.0, 0.0]);
    rotate_point(
        [l.base.x + local[0], l.base.y + local[1]],
        center,
        l.base.angle,
    )
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
    if (target.id() != Some(binding.element_id.as_str()) && l.points.len() > 2)
        || l.points.last() == Some(&[0.0, 0.0])
    {
        return None;
    }

    let focus = global_fixed_point(binding.fixed_point, target)?;
    // 0. An inside binding needs no calculation and ignores the other end.
    if binding.mode == "inside" {
        return Some(create_point_at(geometry, arrow, focus));
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
    );
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
        return Some(create_point_at(geometry, arrow, resolved_target));
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
        return Some(create_point_at(geometry, arrow, point));
    }

    // 3. Too short while connected on both ends.
    if arrow_too_short {
        return Some(create_point_at(
            geometry,
            arrow,
            [
                or_fallback(resolved_target[0], focus[0]),
                or_fallback(resolved_target[1], focus[1]),
            ],
        ));
    }

    // 4. The general case: snap to the outline.
    let snapped = outline.unwrap_or(focus);
    Some(create_point_at(
        geometry,
        arrow,
        [
            or_fallback(snapped[0], focus[0]),
            or_fallback(snapped[1], focus[1]),
        ],
    ))
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
        let [x, y] = fixed_point_for(&arrow, LinearEnd::End, &target).expect("fixed point");
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
            fixed_point_for(&arrow, LinearEnd::End, &target),
            Some([0.5001, 0.5001])
        );
    }

    #[test]
    fn binding_both_ends_to_the_same_target_adds_it_once() {
        let mut file = sample::file(vec![
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 0.0]]),
            sample::generic("rectangle", "r", [100.0, -25.0, 50.0, 50.0]),
        ]);
        bind_arrow(&mut file, 0, LinearEnd::Start, 1, &mut FixedEnv);
        bind_arrow(&mut file, 0, LinearEnd::End, 1, &mut FixedEnv);
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
