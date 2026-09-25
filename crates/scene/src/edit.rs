//! Dragging, deletion, point dragging and load-time repairs, ported at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419` from `packages/element/src/dragElements.ts`'s
//! `dragSelectedElements` and `updateElementCoords`; `packages/excalidraw/components/App.tsx`'s
//! Shift drag-axis lock (around line 11049) and `insertNewElements`;
//! `packages/excalidraw/actions/actionDeleteSelected.tsx`'s `deleteSelectedElements`;
//! `packages/element/src/binding.ts`'s `fixBindingsAfterDeletion`, `BoundElement.unbindAffected`
//! and `BindableElement.unbindAffected`; `packages/element/src/linearElementEditor.ts`'s
//! `movePoints`, `_updatePoints` and `_getShiftLockedDelta`;
//! `packages/element/src/sizeHelpers.ts`'s `getLockedLinearCursorAlignSize`;
//! `packages/element/src/Scene.ts`'s `insertElementsAtIndex`; and the duplicate-id and
//! `syncInvalidIndices` part of `packages/excalidraw/data/restore.ts`'s `restoreElements`.
//!
//! A dragged element's bound text moves with it whether or not the element is an arrow: napkin
//! stores an arrow label's position rather than recomputing it from the arrow's path at render
//! time (M3), so leaving it behind during a drag would strand it. Dragging never rebinds an
//! arrow's endpoints (only deleting and recreating a binding does that, M5); it does unbind
//! them, the same way `dragSelectedElements` does. Deletion's elbow-arrow-specific early
//! unbinding in `deleteSelectedElements` is not ported: `fixBindingsAfterDeletion`, applied
//! afterwards regardless, already clears every binding that pointed at a deleted element, and
//! napkin never creates elbow arrows (M4a).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::element::{Element, LinearEnd};
use crate::env::{Env, random_id};
use crate::file::SceneFile;
use crate::fractional_index;
use crate::geometry::{element_absolute_coords, rotate_point, size_from_points};
use crate::new_element::bump_version;
use crate::selection::Selection;

/// `SHIFT_LOCKING_ANGLE` (`packages/common/src/constants.ts`).
const SHIFT_LOCKING_ANGLE: f64 = std::f64::consts::PI / 12.0;

/// `DRAGGING_THRESHOLD` (`packages/common/src/constants.ts`). Compared against a drag offset
/// that is already in scene coordinates (as `dragSelectedElements` compares it), not screen
/// pixels.
const DRAGGING_THRESHOLD: f64 = 10.0;

/// The Shift drag-axis lock App.tsx applies to `dragOffset` before calling
/// `dragSelectedElements`: the axis that moved less is zeroed, so the drag runs along the
/// other axis only. Neither axis is zeroed when the two are equal.
pub fn lock_drag_axis(offset: [f64; 2]) -> [f64; 2] {
    let [x, y] = offset;
    let distance_x = x.abs();
    let distance_y = y.abs();
    if distance_x < distance_y {
        [0.0, y]
    } else if distance_x > distance_y {
        [x, 0.0]
    } else {
        [x, y]
    }
}

/// `dragSelectedElements`'s `elementsToUpdate`, as positions in `file`, ascending: the
/// selected, non-deleted elements; elements whose `frameId` names a selected frame; and, for
/// every one of those, the text bound to it (a container's label or an arrow's, see the module
/// doc comment).
pub fn drag_targets(file: &SceneFile, selection: &Selection) -> Vec<usize> {
    let id_to_pos: HashMap<&str, usize> = file
        .elements
        .iter()
        .enumerate()
        .filter(|(_, element)| !element.is_deleted())
        .filter_map(|(index, element)| element.id().map(|id| (id, index)))
        .collect();

    let mut targets: BTreeSet<usize> = selection.positions(file).into_iter().collect();

    let frame_ids: HashSet<&str> = targets
        .iter()
        .filter(|&&pos| file.elements[pos].kind() == "frame")
        .filter_map(|&pos| file.elements[pos].id())
        .collect();
    if !frame_ids.is_empty() {
        for (pos, element) in file.elements.iter().enumerate() {
            if !element.is_deleted()
                && element
                    .frame_id()
                    .is_some_and(|fid| frame_ids.contains(fid))
            {
                targets.insert(pos);
            }
        }
    }

    let bound_text: Vec<usize> = targets
        .iter()
        .filter_map(|&pos| {
            let (text_id, _) = file.elements[pos]
                .bound_elements()
                .into_iter()
                .find(|&(_, kind)| kind == "text")?;
            id_to_pos.get(text_id).copied()
        })
        .collect();
    targets.extend(bound_text);

    targets.into_iter().collect()
}

/// `updateElementCoords`: sets every target's position to its `start` position plus `offset`.
/// A `Raw` target whose `x`/`y` are not both numbers does not move and is not touched.
///
/// A bound arrow that is the only *primary* target does not move at all — and so keeps both
/// its bindings, and so does its label, since it moves with the arrow (see below) — until
/// `offset` clears [`DRAGGING_THRESHOLD`], the same guard `dragSelectedElements` applies before
/// it will unbind a lone dragged arrow by accident. Once an arrow does move, whichever of its
/// `startBinding`/`endBinding` names an element that is not itself a primary target is cleared,
/// the way `unbindBindingElement` clears it: the arrow's own binding field is set to `null`,
/// and the arrow is dropped from that element's `boundElements` (unless the arrow's other end
/// is bound to the very same element, in which case that `boundElements` record still covers
/// the remaining end and stays). "Primary" mirrors `dragSelectedElements`'s
/// `elementsToUpdate`, which never includes a bound text: it is `targets` minus any text
/// element whose container is itself in `targets` (a container's label, or an arrow's — see
/// the module doc comment for why napkin puts arrow labels in `targets` at all, unlike
/// Excalidraw). Every call recomputes from `start` and `targets`, so calling this repeatedly
/// with the same arguments is idempotent, not cumulative.
pub fn apply_drag(
    file: &mut SceneFile,
    start: &SceneFile,
    targets: &[usize],
    offset: [f64; 2],
    env: &mut impl Env,
) {
    let target_positions: HashSet<usize> = targets.iter().copied().collect();
    let id_to_pos: HashMap<&str, usize> = start
        .elements
        .iter()
        .enumerate()
        .filter_map(|(pos, element)| element.id().map(|id| (id, pos)))
        .collect();
    let container_pos = |pos: usize| {
        start.elements[pos]
            .container_id()
            .and_then(|cid| id_to_pos.get(cid).copied())
    };
    let is_bound_text_target =
        |pos: usize| container_pos(pos).is_some_and(|cpos| target_positions.contains(&cpos));
    let primary_targets: HashSet<usize> = target_positions
        .iter()
        .copied()
        .filter(|&pos| !is_bound_text_target(pos))
        .collect();
    let past_threshold = offset[0].abs().max(offset[1].abs()) > DRAGGING_THRESHOLD;

    // Arrows the lone-target threshold guard holds still; a label whose container is one of
    // these is held still along with it, so it does not drift away from a stationary arrow.
    // Bindings are read from `file`, not `start`: a prior call in the same drag gesture (from
    // the same `start`, a larger offset that cleared the threshold) may already have unbound
    // this arrow in `file`, and once unbound it is no longer held still, exactly as JS's guard
    // reads the live element's `startBinding`/`endBinding`, not the drag's original snapshot.
    let suppressed_arrows: HashSet<usize> = targets
        .iter()
        .copied()
        .filter(|&pos| {
            start.elements[pos].kind() == "arrow"
                && primary_targets.len() <= 1
                && !past_threshold
                && (file.elements[pos]
                    .binding_target(LinearEnd::Start)
                    .is_some()
                    || file.elements[pos].binding_target(LinearEnd::End).is_some())
        })
        .collect();

    for &pos in targets {
        if suppressed_arrows.contains(&pos) {
            continue;
        }
        if container_pos(pos).is_some_and(|cpos| suppressed_arrows.contains(&cpos)) {
            continue;
        }

        let Some(placement) = start.elements[pos].placement() else {
            continue;
        };
        let before = file.elements[pos].clone();
        file.elements[pos].set_position(placement.x + offset[0], placement.y + offset[1]);
        if file.elements[pos] != before {
            bump_version(&mut file.elements[pos], env);
        }

        if start.elements[pos].kind() != "arrow" {
            continue;
        }
        for end in [LinearEnd::Start, LinearEnd::End] {
            let Some(target_id) = start.elements[pos].binding_target(end) else {
                continue;
            };
            let bound_to_a_target = id_to_pos
                .get(target_id)
                .is_some_and(|target_pos| primary_targets.contains(target_pos));
            if !bound_to_a_target {
                unbind_arrow_end(file, &id_to_pos, pos, end, env);
            }
        }
    }
}

/// `unbindBindingElement`: clears the arrow at `pos`'s binding for `end` and, unless the
/// arrow's other end is bound to the same element (that `boundElements` record then still
/// covers the remaining end), removes the arrow from that element's `boundElements`.
fn unbind_arrow_end(
    file: &mut SceneFile,
    id_to_pos: &HashMap<&str, usize>,
    pos: usize,
    end: LinearEnd,
    env: &mut impl Env,
) {
    let Some(target_id) = file.elements[pos].binding_target(end).map(str::to_owned) else {
        return;
    };
    let opposite = match end {
        LinearEnd::Start => LinearEnd::End,
        LinearEnd::End => LinearEnd::Start,
    };
    let opposite_target = file.elements[pos].binding_target(opposite);
    let shares_target = opposite_target == Some(target_id.as_str());

    // `Raw`'s restricted mutation surface (spec §5.2) has no binding fields, so `clear_binding`
    // below is a no-op for a Raw arrow. Removing it from the target's `boundElements` anyway
    // would leave the binding one-sided: the target no longer lists the arrow, but the arrow's
    // own JSON still names the target.
    let can_clear = !matches!(file.elements[pos], Element::Raw(_));

    if can_clear
        && !shares_target
        && let Some(&target_pos) = id_to_pos.get(target_id.as_str())
    {
        let arrow_id = file.elements[pos].id().expect("looked up by id").to_owned();
        let before = file.elements[target_pos].clone();
        file.elements[target_pos].remove_bound_element(&arrow_id);
        if file.elements[target_pos] != before {
            bump_version(&mut file.elements[target_pos], env);
        }
    }

    let before = file.elements[pos].clone();
    file.elements[pos].clear_binding(end);
    if file.elements[pos] != before {
        bump_version(&mut file.elements[pos], env);
    }
}

/// `deleteSelectedElements` plus `fixBindingsAfterDeletion`. Marks every selected element, and
/// every text whose container is selected, deleted. A selected frame's children are never
/// deleted: their `frameId` is cleared instead, and they (a bound text's container, in its
/// place) become the returned selection. `fixBindingsAfterDeletion`'s two directions then run
/// once for each element this call deleted, skipping a binding partner that is already deleted.
pub fn delete_selection(
    file: &mut SceneFile,
    selection: &Selection,
    env: &mut impl Env,
) -> Selection {
    // Ids and positions never change below (deletion only flips `isDeleted` or clears
    // `frameId`), so this snapshot stays valid for the whole function.
    let id_to_pos: HashMap<String, usize> = file
        .elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| element.id().map(|id| (id.to_owned(), index)))
        .collect();

    let selected_ids: BTreeSet<String> = file
        .elements
        .iter()
        .filter(|element| !element.is_deleted())
        .filter_map(Element::id)
        .filter(|id| selection.contains(id))
        .map(str::to_owned)
        .collect();

    let frames_to_delete: BTreeSet<String> = selected_ids
        .iter()
        .filter(|id| {
            id_to_pos
                .get(id.as_str())
                .is_some_and(|&pos| file.elements[pos].kind() == "frame")
        })
        .cloned()
        .collect();

    // Children of a deleted frame survive: `frameId` is cleared and they (or, for a bound
    // text, its container) join the next selection instead.
    let mut next_selection = Selection::new();
    let mut frame_child_ids: BTreeSet<String> = BTreeSet::new();
    for element in &file.elements {
        if element.is_deleted() {
            continue;
        }
        let Some(frame_id) = element.frame_id() else {
            continue;
        };
        if !frames_to_delete.contains(frame_id) {
            continue;
        }
        let Some(id) = element.id() else { continue };
        frame_child_ids.insert(id.to_owned());
        match element.container_id() {
            Some(container_id) => next_selection.insert(container_id.to_owned()),
            None => next_selection.insert(id.to_owned()),
        };
    }

    let delete_ids: BTreeSet<String> = file
        .elements
        .iter()
        .filter(|element| !element.is_deleted())
        .filter_map(|element| {
            let id = element.id()?;
            if frame_child_ids.contains(id) {
                return None;
            }
            let container_selected = element
                .container_id()
                .is_some_and(|container_id| selected_ids.contains(container_id));
            (selected_ids.contains(id) || container_selected).then(|| id.to_owned())
        })
        .collect();

    for pos in 0..file.elements.len() {
        let Some(id) = file.elements[pos].id().map(str::to_owned) else {
            continue;
        };
        if frame_child_ids.contains(&id) {
            let before = file.elements[pos].clone();
            file.elements[pos].clear_frame_id();
            if file.elements[pos] != before {
                bump_version(&mut file.elements[pos], env);
            }
        } else if delete_ids.contains(&id) {
            file.elements[pos].set_deleted(true);
            bump_version(&mut file.elements[pos], env);
        }
    }

    for id in &delete_ids {
        let Some(&pos) = id_to_pos.get(id) else {
            continue;
        };
        unbind_targets(file, &id_to_pos, pos, env);
        unbind_bound_elements(file, &id_to_pos, pos, env);
    }

    next_selection
}

/// `BoundElement.unbindAffected`: removes the element at `pos`'s id from `boundElements` of
/// each non-deleted element it is itself bound to (its container, and the elements its start
/// and end bindings name). `frameId` is not one of these targets: a frame never lists its
/// children in its own `boundElements`.
fn unbind_targets(
    file: &mut SceneFile,
    id_to_pos: &HashMap<String, usize>,
    pos: usize,
    env: &mut impl Env,
) {
    let id = file.elements[pos].id().expect("looked up by id").to_owned();
    let mut target_ids: Vec<&str> = Vec::new();
    if let Some(container_id) = file.elements[pos].container_id() {
        target_ids.push(container_id);
    }
    if let Some(start) = file.elements[pos].binding_target(LinearEnd::Start) {
        target_ids.push(start);
    }
    if let Some(end) = file.elements[pos].binding_target(LinearEnd::End) {
        target_ids.push(end);
    }
    let target_ids: Vec<String> = target_ids.into_iter().map(str::to_owned).collect();

    for target_id in target_ids {
        let Some(&target_pos) = id_to_pos.get(&target_id) else {
            continue;
        };
        if file.elements[target_pos].is_deleted() {
            continue;
        }
        let before = file.elements[target_pos].clone();
        file.elements[target_pos].remove_bound_element(&id);
        if file.elements[target_pos] != before {
            bump_version(&mut file.elements[target_pos], env);
        }
    }
}

/// `BindableElement.unbindAffected`: clears whichever of a non-deleted bound element's
/// `containerId`, `startBinding` or `endBinding` points at the element at `pos`'s id.
fn unbind_bound_elements(
    file: &mut SceneFile,
    id_to_pos: &HashMap<String, usize>,
    pos: usize,
    env: &mut impl Env,
) {
    let id = file.elements[pos].id().expect("looked up by id").to_owned();
    let bound_ids: Vec<String> = file.elements[pos]
        .bound_elements()
        .into_iter()
        .map(|(bound_id, _)| bound_id.to_owned())
        .collect();

    for bound_id in bound_ids {
        let Some(&bound_pos) = id_to_pos.get(&bound_id) else {
            continue;
        };
        if file.elements[bound_pos].is_deleted() {
            continue;
        }
        let before = file.elements[bound_pos].clone();
        if file.elements[bound_pos].container_id() == Some(id.as_str()) {
            file.elements[bound_pos].clear_container_id();
        }
        if file.elements[bound_pos].binding_target(LinearEnd::Start) == Some(id.as_str()) {
            file.elements[bound_pos].clear_binding(LinearEnd::Start);
        }
        if file.elements[bound_pos].binding_target(LinearEnd::End) == Some(id.as_str()) {
            file.elements[bound_pos].clear_binding(LinearEnd::End);
        }
        if file.elements[bound_pos] != before {
            bump_version(&mut file.elements[bound_pos], env);
        }
    }
}

/// `getLockedLinearCursorAlignSize` without a custom angle: `point` snapped to 15° steps
/// around `origin`, both in the same coordinate space (scene coordinates, as
/// [`move_linear_point`] uses it).
pub fn lock_linear_angle(origin: [f64; 2], point: [f64; 2]) -> [f64; 2] {
    let mut width = point[0] - origin[0];
    let mut height = point[1] - origin[1];

    let angle = rough::js::atan2(height, width);
    let locked_angle = rough::js::math_round(angle / SHIFT_LOCKING_ANGLE) * SHIFT_LOCKING_ANGLE;

    if locked_angle == 0.0 {
        height = 0.0;
    } else if locked_angle == std::f64::consts::FRAC_PI_2 {
        width = 0.0;
    } else {
        // Locked-angle line through `origin`, and the line through `point` perpendicular to
        // it: `width`/`height` become the offset from `origin` to where they cross.
        let a1 = locked_angle.tan();
        let b1 = -1.0;
        let c1 = origin[1] - a1 * origin[0];
        let a2 = -1.0 / a1;
        let b2 = -1.0;
        let c2 = point[1] - a2 * point[0];
        let denom = a1 * b2 - a2 * b1;
        let intersect_x = (b1 * c2 - b2 * c1) / denom;
        let intersect_y = (c1 * a2 - c2 * a1) / denom;
        width = intersect_x - origin[0];
        height = intersect_y - origin[1];
    }

    [origin[0] + width, origin[1] + height]
}

/// A local point of `angle`'s line or arrow, in scene coordinates: `angle`'s rotation about
/// `center`, applied to the point plus the element's origin.
fn to_scene(point: [f64; 2], base: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    let absolute = [point[0] + base[0], point[1] + base[1]];
    if angle == 0.0 {
        absolute
    } else {
        rotate_point(absolute, center, angle)
    }
}

/// The inverse of [`to_scene`]: a scene point's local coordinates.
fn to_local(point: [f64; 2], base: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    let unrotated = if angle == 0.0 {
        point
    } else {
        rotate_point(point, center, -angle)
    };
    [unrotated[0] - base[0], unrotated[1] - base[1]]
}

/// `LinearElementEditor.movePoints` for one point (`pointUpdates` holding a single entry) plus
/// `_updatePoints` and, when `shift`, `_getShiftLockedDelta`: moves the point at `index` of a
/// line or arrow to `target` (scene coordinates), reading `start`'s points, position and angle
/// as the pre-drag state. Moving point 0 keeps the points array's `[0, 0]`-at-the-first-point
/// invariant by shifting every other point the opposite way and moving the element's `x`/`y`
/// to compensate, rotated to account for how the new points' unrotated bounding box (and so
/// its center) can shift under rotation even when the points move by a uniform offset. A no-op
/// for anything but a line or arrow, or an out-of-range `index`.
pub fn move_linear_point(
    start: &Element,
    element: &mut Element,
    index: usize,
    target: [f64; 2],
    shift: bool,
    env: &mut impl Env,
) {
    let (points, base, angle) = match start {
        Element::Line(l) | Element::Arrow(l) => (&l.points, [l.base.x, l.base.y], l.base.angle),
        _ => return,
    };
    if index >= points.len() {
        return;
    }
    let (prev_coords, center) =
        element_absolute_coords(start).expect("index in range implies a point");

    let new_local_point = if shift {
        let pivot_index = if index == 0 { 1 } else { index - 1 };
        match points.get(pivot_index) {
            Some(&pivot) => {
                let pivot_scene = to_scene(pivot, base, center, angle);
                let locked_scene = lock_linear_angle(pivot_scene, target);
                to_local(locked_scene, base, center, angle)
            }
            None => to_local(target, base, center, angle),
        }
    } else {
        to_local(target, base, center, angle)
    };

    let offset = if index == 0 {
        new_local_point
    } else {
        [0.0, 0.0]
    };
    let next_points: Vec<[f64; 2]> = points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let current = if i == index { new_local_point } else { p };
            [current[0] - offset[0], current[1] - offset[1]]
        })
        .collect();

    let mut next_start = start.clone();
    if let Element::Line(l) | Element::Arrow(l) = &mut next_start {
        l.points = next_points.clone();
    }
    let (next_coords, _) = element_absolute_coords(&next_start).expect("same points, non-empty");
    let prev_center = [
        (prev_coords[0] + prev_coords[2]) / 2.0,
        (prev_coords[1] + prev_coords[3]) / 2.0,
    ];
    let next_center = [
        (next_coords[0] + next_coords[2]) / 2.0,
        (next_coords[1] + next_coords[3]) / 2.0,
    ];
    let center_shift = [
        prev_center[0] - next_center[0],
        prev_center[1] - next_center[1],
    ];
    let rotated_offset = rotate_point(offset, center_shift, angle);

    let before = element.clone();
    if let Element::Line(l) | Element::Arrow(l) = element {
        let [width, height] = size_from_points(&next_points);
        l.points = next_points;
        l.base.x = base[0] + rotated_offset[0];
        l.base.y = base[1] + rotated_offset[1];
        l.base.width = width;
        l.base.height = height;
    }
    if *element != before {
        bump_version(element, env);
    }
}

/// `insertElementsAtIndex` for a single element with no frame, as called by
/// `insertNewElements`: appends `element`, syncs its `index` and returns its position.
pub fn append_element(file: &mut SceneFile, element: Element, env: &mut impl Env) -> usize {
    let moved: HashSet<String> = element.id().map(str::to_owned).into_iter().collect();
    let position = file.elements.len();
    file.elements.push(element);
    fractional_index::sync_moved_indices(&mut file.elements, &moved, env);
    position
}

/// The repairs `restoreElements` always applies, regardless of `repairBindings`: an element
/// sharing an `id` with one already seen gets a fresh nanoid (the first occurrence keeps its
/// id), then every element's `index` is fixed up by `syncInvalidIndices`.
pub fn repair_on_load(file: &mut SceneFile, env: &mut impl Env) {
    let mut seen: HashSet<String> = HashSet::new();
    for element in &mut file.elements {
        let Some(id) = element.id().map(str::to_owned) else {
            continue;
        };
        if seen.contains(&id) {
            let new_id = random_id(env);
            let mut value = element.to_value();
            if let Some(map) = value.as_object_mut() {
                map.insert("id".into(), serde_json::json!(new_id));
                seen.insert(new_id);
            } else {
                seen.insert(id);
            }
            *element = Element::from_value(value);
        } else {
            seen.insert(id);
        }
    }
    fractional_index::sync_invalid_indices(&mut file.elements, env);
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::element::LinearEnd;
    use crate::sample;

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

    fn rect(id: &str, r: [f64; 4]) -> Value {
        sample::generic("rectangle", id, r)
    }

    fn xy(file: &SceneFile, id: &str) -> (f64, f64) {
        let e = file.elements.iter().find(|e| e.id() == Some(id)).expect(id);
        let p = e.placement().expect("placement");
        (p.x, p.y)
    }

    fn get<'a>(file: &'a SceneFile, id: &str) -> &'a Element {
        file.elements.iter().find(|e| e.id() == Some(id)).expect(id)
    }

    fn bindings_scene() -> SceneFile {
        sample::file(vec![
            sample::with(
                rect("r", [0.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]}),
            ),
            sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some("r")),
            sample::with(
                sample::linear("arrow", "a", [0.0, 200.0], &[[0.0, 0.0], [100.0, 0.0]]),
                json!({
                    "boundElements": [{"id": "l", "type": "text"}],
                    "startBinding": {"elementId": "r", "fixedPoint": [0.5, 1.0], "mode": "orbit"},
                    "endBinding": {"elementId": "s", "fixedPoint": [0.0, 0.5], "mode": "orbit"}
                }),
            ),
            sample::text("l", [40.0, 190.0, 20.0, 20.0], "label", Some("a")),
            sample::with(
                rect("s", [300.0, 150.0, 50.0, 50.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            json!({"id": "f", "type": "frame", "x": 500, "y": 0, "width": 100, "height": 100, "angle": 0,
                   "isDeleted": false, "version": 1, "versionNonce": 1}),
            sample::with(
                rect("c", [510.0, 10.0, 10.0, 10.0]),
                json!({"frameId": "f"}),
            ),
            rect("other", [900.0, 900.0, 10.0, 10.0]),
        ])
    }

    #[test]
    fn shift_lock_keeps_the_longer_axis() {
        assert_eq!(lock_drag_axis([3.0, -10.0]), [0.0, -10.0]);
        assert_eq!(lock_drag_axis([10.0, 4.0]), [10.0, 0.0]);
        assert_eq!(lock_drag_axis([5.0, -5.0]), [5.0, -5.0]);
    }

    #[test]
    fn dragging_moves_dependents_once() {
        let start = bindings_scene();
        let selection = Selection::from_ids(["r", "a", "f", "t"]);
        let targets = drag_targets(&start, &selection);
        assert_eq!(targets, vec![0, 1, 2, 3, 5, 6]);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [10.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "r"), (10.0, 5.0));
        assert_eq!(xy(&file, "t"), (40.0, 45.0));
        assert_eq!(xy(&file, "a"), (10.0, 205.0));
        assert_eq!(xy(&file, "l"), (50.0, 195.0));
        assert_eq!(xy(&file, "f"), (510.0, 5.0));
        assert_eq!(xy(&file, "c"), (520.0, 15.0));
        assert_eq!(get(&file, "other"), get(&start, "other"));
        assert_eq!(get(&file, "r").version(), get(&start, "r").version() + 1.0);
        // Re-applying from the same start is idempotent, not cumulative.
        apply_drag(&mut file, &start, &targets, [10.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "r"), (10.0, 5.0));
    }

    /// An arrow bound at both ends to shapes it has no other relation to (no label, so
    /// dragging the arrow alone makes it the sole drag target).
    fn bound_arrow_scene() -> SceneFile {
        sample::file(vec![
            sample::with(
                rect("r", [0.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                rect("s", [300.0, 150.0, 50.0, 50.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 200.0], &[[0.0, 0.0], [100.0, 0.0]]),
                json!({
                    "startBinding": {"elementId": "r", "fixedPoint": [0.5, 1.0], "mode": "orbit"},
                    "endBinding": {"elementId": "s", "fixedPoint": [0.0, 0.5], "mode": "orbit"}
                }),
            ),
        ])
    }

    #[test]
    fn raw_arrow_dragged_past_threshold_keeps_its_binding_on_both_sides() {
        let mut arrow = sample::with(
            sample::linear("arrow", "a", [0.0, 200.0], &[[0.0, 0.0], [100.0, 0.0]]),
            json!({
                "startBinding": {"elementId": "r", "fixedPoint": [0.5, 1.0], "mode": "orbit"},
                "endBinding": {"elementId": "s", "fixedPoint": [0.0, 0.5], "mode": "orbit"}
            }),
        );
        // Missing `strokeStyle` (no `#[serde(default)]` on `ElementBase`) fails the exact
        // round-trip check, so this arrow loads as `Element::Raw`, not `Element::Arrow`.
        if let Value::Object(map) = &mut arrow {
            map.remove("strokeStyle");
        }
        assert!(matches!(
            Element::from_value(arrow.clone()),
            Element::Raw(_)
        ));

        let start = sample::file(vec![
            sample::with(
                rect("r", [0.0, 0.0, 100.0, 100.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            sample::with(
                rect("s", [300.0, 150.0, 50.0, 50.0]),
                json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
            ),
            arrow,
        ]);
        let selection = Selection::from_ids(["a"]);
        let targets = drag_targets(&start, &selection);
        assert_eq!(targets, vec![2]);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [11.0, 0.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), (11.0, 200.0));
        // The arrow's own binding fields cannot be cleared (`Raw`'s restricted mutation
        // surface has no binding fields), so the target side is left untouched too: both
        // stay bound rather than the target losing the arrow one-sidedly.
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), Some("s"));
        assert_eq!(get(&file, "r").bound_elements(), vec![("a", "arrow")]);
        assert_eq!(get(&file, "s").bound_elements(), vec![("a", "arrow")]);
    }

    #[test]
    fn lone_bound_arrow_unbinds_both_ends_past_the_dragging_threshold() {
        let start = bound_arrow_scene();
        let selection = Selection::from_ids(["a"]);
        let targets = drag_targets(&start, &selection);
        assert_eq!(targets, vec![2]);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [11.0, 0.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), (11.0, 200.0));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), None);
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), None);
        assert_eq!(get(&file, "r").bound_elements(), vec![]);
        assert_eq!(get(&file, "s").bound_elements(), vec![]);
    }

    #[test]
    fn lone_bound_arrow_dragged_past_threshold_then_back_settles_at_the_new_offset() {
        let start = bound_arrow_scene();
        let selection = Selection::from_ids(["a"]);
        let targets = drag_targets(&start, &selection);
        let mut file = start.clone();
        // Past the threshold: the arrow moves and unbinds both ends.
        apply_drag(&mut file, &start, &targets, [30.0, 0.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), (30.0, 200.0));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), None);
        // Same gesture, dragged back under the threshold: the arrow is no longer bound (in
        // `file`, mutated by the call above), so the lone-arrow guard no longer applies and it
        // follows the pointer back down to the new offset instead of staying at +30.
        apply_drag(&mut file, &start, &targets, [3.0, 0.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), (3.0, 200.0));
    }

    #[test]
    fn dragging_an_arrow_with_its_start_shape_keeps_only_that_binding() {
        let start = bound_arrow_scene();
        let selection = Selection::from_ids(["r", "a"]);
        let targets = drag_targets(&start, &selection);
        assert_eq!(targets, vec![0, 2]);
        let mut file = start.clone();
        // Under the dragging threshold: with more than one target, the threshold guard does
        // not apply at all (matches `dragSelectedElements`'s `elementsToUpdate.size > 1`).
        apply_drag(&mut file, &start, &targets, [4.0, 0.0], &mut TestEnv(0));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), None);
        assert_eq!(get(&file, "r").bound_elements(), vec![("a", "arrow")]);
        assert_eq!(get(&file, "s").bound_elements(), vec![]);
    }

    #[test]
    fn lone_bound_arrow_stays_put_under_the_dragging_threshold() {
        let start = bound_arrow_scene();
        let selection = Selection::from_ids(["a"]);
        let targets = drag_targets(&start, &selection);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [5.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), xy(&start, "a"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), Some("s"));
        assert_eq!(get(&file, "a").version(), get(&start, "a").version());
    }

    #[test]
    fn labeled_lone_bound_arrow_stays_put_with_its_label_under_the_threshold() {
        let start = bindings_scene();
        let selection = Selection::from_ids(["a"]);
        let targets = drag_targets(&start, &selection);
        // "a" and its label "l": more than one target, but "l" is not primary (its container,
        // "a", is itself a target), so the lone-arrow guard still applies to "a".
        assert_eq!(targets, vec![2, 3]);
        let mut file = start.clone();
        apply_drag(&mut file, &start, &targets, [5.0, 5.0], &mut TestEnv(0));
        assert_eq!(xy(&file, "a"), xy(&start, "a"));
        assert_eq!(xy(&file, "l"), xy(&start, "l"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), Some("s"));
        assert_eq!(get(&file, "a").version(), get(&start, "a").version());
        assert_eq!(get(&file, "l").version(), get(&start, "l").version());
    }

    #[test]
    fn deleting_a_container_takes_its_text_and_unbinds_arrows() {
        let mut file = bindings_scene();
        let next = delete_selection(&mut file, &Selection::from_ids(["r"]), &mut TestEnv(0));
        assert!(next.is_empty());
        assert!(get(&file, "r").is_deleted() && get(&file, "t").is_deleted());
        assert!(!get(&file, "a").is_deleted());
        assert_eq!(get(&file, "a").binding_target(LinearEnd::Start), None);
        assert_eq!(get(&file, "a").binding_target(LinearEnd::End), Some("s"));
    }

    #[test]
    fn deleting_an_arrow_takes_its_label_and_cleans_bound_elements() {
        let mut file = bindings_scene();
        delete_selection(&mut file, &Selection::from_ids(["a"]), &mut TestEnv(0));
        assert!(get(&file, "a").is_deleted() && get(&file, "l").is_deleted());
        assert_eq!(get(&file, "r").bound_elements(), vec![("t", "text")]);
        assert_eq!(get(&file, "s").bound_elements(), vec![]);
    }

    #[test]
    fn deleting_a_frame_releases_and_selects_its_children() {
        let mut file = bindings_scene();
        let next = delete_selection(&mut file, &Selection::from_ids(["f"]), &mut TestEnv(0));
        assert_eq!(next, Selection::from_ids(["c"]));
        assert!(get(&file, "f").is_deleted() && !get(&file, "c").is_deleted());
        assert_eq!(get(&file, "c").frame_id(), None);
    }

    #[test]
    fn moving_a_point_renormalizes_the_first_point() {
        let arrow = Element::from_value(sample::linear(
            "arrow",
            "a",
            [10.0, 10.0],
            &[[0.0, 0.0], [100.0, 0.0]],
        ));
        let mut moved = arrow.clone();
        move_linear_point(&arrow, &mut moved, 0, [0.0, 30.0], false, &mut TestEnv(0));
        let v = moved.to_value();
        assert_eq!((v["x"].clone(), v["y"].clone()), (json!(0.0), json!(30.0)));
        assert_eq!(v["points"], json!([[0.0, 0.0], [110.0, -20.0]]));
        assert_eq!(
            (v["width"].clone(), v["height"].clone()),
            (json!(110.0), json!(20.0))
        );

        let mut moved = arrow.clone();
        move_linear_point(&arrow, &mut moved, 1, [50.0, 60.0], false, &mut TestEnv(0));
        assert_eq!(
            moved.to_value()["points"],
            json!([[0.0, 0.0], [40.0, 50.0]])
        );

        let mut locked = arrow.clone();
        move_linear_point(&arrow, &mut locked, 1, [110.0, 13.0], true, &mut TestEnv(0));
        let points = locked.to_value()["points"].clone();
        assert!(
            (points[1][0].as_f64().unwrap() - 100.0).abs() < 1e-9
                && points[1][1].as_f64().unwrap().abs() < 1e-9,
            "{points}"
        );
        let snapped = lock_linear_angle([0.0, 0.0], [10.0, 9.0]);
        assert!((snapped[0] - snapped[1]).abs() < 1e-9, "{snapped:?}");
    }

    #[test]
    fn appended_elements_sort_after_everything() {
        let mut file = sample::file(vec![
            rect("a", [0.0, 0.0, 1.0, 1.0]),
            sample::with(rect("b", [0.0, 0.0, 1.0, 1.0]), json!({"index": "a1"})),
        ]);
        let new = Element::from_value(sample::with(
            rect("n", [0.0, 0.0, 1.0, 1.0]),
            json!({"index": null}),
        ));
        assert_eq!(append_element(&mut file, new, &mut TestEnv(0)), 2);
        let index = file.elements[2].index().expect("index").to_string();
        assert!(index.as_str() > "a1", "{index}");
    }

    #[test]
    fn load_repairs_rename_duplicates_and_sync_indices() {
        let mut file = sample::file(vec![
            rect("x", [0.0, 0.0, 1.0, 1.0]),
            rect("x", [0.0, 0.0, 1.0, 1.0]),
            rect("y", [0.0, 0.0, 1.0, 1.0]),
        ]);
        repair_on_load(&mut file, &mut TestEnv(0));
        let ids: Vec<&str> = file.elements.iter().map(|e| e.id().unwrap()).collect();
        assert_eq!(ids[0], "x");
        assert!(
            ids[1] != "x" && ids[1] != "y" && ids[1].len() == 21,
            "{ids:?}"
        );
        assert_eq!(ids[2], "y");
        let indices: Vec<&str> = file.elements.iter().map(|e| e.index().unwrap()).collect();
        assert!(indices.windows(2).all(|w| w[0] < w[1]), "{indices:?}");
    }
}
