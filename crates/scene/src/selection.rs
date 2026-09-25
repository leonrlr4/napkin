//! Which elements a pointer or box selection touches, and the priority and grouping rules
//! for turning "touches" into "selected": `getElementsAtPosition`, `getElementAtPosition` and
//! `hitElement` (`packages/excalidraw/components/App.tsx`, roughly lines 6614-6790) and
//! `isHittingCommonBoundingBoxOfSelectedElements` (same file, ~line 9864);
//! `hitElementBoundingBox` and `hitElementBoundText` (`packages/element/src/collision.ts`);
//! `getElementsWithinSelection` and `shouldIgnoreElementFromSelection`
//! (`packages/element/src/selection.ts`); `elementsOverlappingBBox`'s `"contain"` path
//! (`packages/element/src/bounds.ts`); `selectGroupsForSelectedElements`
//! (`packages/element/src/groups.ts`); `actionSelectAll`
//! (`packages/excalidraw/actions/actionSelectAll.ts`); and `hasBoundingBox`
//! (`packages/element/src/transformHandles.ts`); all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! Frame clipping (`isCursorInFrame`) and frame-child deduplication are not ported: napkin
//! does not select anything inside a frame beyond the frame itself, so box selection skips
//! `elementsOverlappingBBox`'s frame bookkeeping entirely. `hitElementBoundText` and box
//! selection's label bounds both read a bound text element's own stored `x`/`y` instead of
//! porting `LinearElementEditor.getBoundTextElementPosition`'s recomputed position for an
//! arrow label; this only disagrees with Excalidraw once a multi-point arrow's label has
//! drifted from that stored position.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::collision::{
    DEFAULT_COLLISION_THRESHOLD, DEFAULT_TRANSFORM_HANDLE_SPACING, hit_element_itself,
    hit_threshold, is_point_in_element, stroke_width,
};
use crate::element::Element;
use crate::file::SceneFile;
use crate::geometry::{Bounds, GeometryCache, bounds_contain, rotate_point};

/// Selected element ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection(BTreeSet<String>);

impl Selection {
    pub fn new() -> Selection {
        Selection::default()
    }

    pub fn from_ids<S: Into<String>>(ids: impl IntoIterator<Item = S>) -> Selection {
        Selection(ids.into_iter().map(Into::into).collect())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    pub fn insert(&mut self, id: impl Into<String>) -> bool {
        self.0.insert(id.into())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.0.remove(id)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Positions in `file` of selected, non-deleted elements, ascending.
    pub fn positions(&self, file: &SceneFile) -> Vec<usize> {
        file.elements
            .iter()
            .enumerate()
            .filter(|(_, element)| {
                !element.is_deleted() && element.id().is_some_and(|id| self.contains(id))
            })
            .map(|(index, _)| index)
            .collect()
    }
}

/// Neither deleted, locked, nor text bound to a container (`shouldIgnoreElementFromSelection`,
/// plus the `isDeleted` check every one of its callers already applies by working from a
/// non-deleted element list).
pub fn is_selectable(element: &Element) -> bool {
    !element.is_deleted() && !element.is_locked() && element.container_id().is_none()
}

/// `selectGroupsForSelectedElements` without `editingGroupId`: every element sharing an
/// outermost group with a selected element joins the selection too.
pub fn select_groups(file: &SceneFile, selection: &Selection) -> Selection {
    let mut selected_group_ids: BTreeSet<&str> = BTreeSet::new();
    for element in &file.elements {
        let Some(id) = element.id() else { continue };
        if !selection.contains(id) {
            continue;
        }
        if let Some(&outermost) = element.group_ids().last() {
            selected_group_ids.insert(outermost);
        }
    }

    let mut result = selection.clone();
    for element in &file.elements {
        if element.is_deleted() {
            continue;
        }
        let Some(id) = element.id() else { continue };
        if element
            .group_ids()
            .iter()
            .any(|group_id| selected_group_ids.contains(group_id))
        {
            result.insert(id);
        }
    }
    result
}

/// `actionSelectAll`: every selectable element, with its groups folded in.
pub fn select_all(file: &SceneFile) -> Selection {
    let base = Selection::from_ids(
        file.elements
            .iter()
            .filter(|element| is_selectable(element))
            .filter_map(|element| element.id()),
    );
    select_groups(file, &base)
}

/// `getElementsWithinSelection` in `"contain"` mode, i.e. `elementsOverlappingBBox`'s
/// `type: "contain"` path: an element joins the selection only when `rect` fully contains its
/// bounds (widened by half its stroke width, and by its bound text label's bounds when it is
/// an arrow), and a grouped element is kept only when every other selectable member of its
/// outermost group is contained too.
pub fn box_select(geometry: &mut GeometryCache, file: &SceneFile, rect: Bounds) -> Selection {
    let [rx1, ry1, rx2, ry2] = rect;
    let selection_bounds = [rx1.min(rx2), ry1.min(ry2), rx1.max(rx2), ry1.max(ry2)];

    let index_by_id: HashMap<&str, usize> = file
        .elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| element.id().map(|id| (id, index)))
        .collect();

    let mut groups: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut contained: BTreeSet<&str> = BTreeSet::new();

    for element in &file.elements {
        if !is_selectable(element) {
            continue;
        }
        let Some(id) = element.id() else { continue };

        if let Some(&outermost) = element.group_ids().last() {
            groups.entry(outermost).or_default().push(id);
        }

        let Some(bounds) = geometry.bounds(element) else {
            continue;
        };
        let half_stroke = stroke_width(element) / 2.0;
        let mut element_aabb = [
            bounds[0] - half_stroke,
            bounds[1] - half_stroke,
            bounds[2] + half_stroke,
            bounds[3] + half_stroke,
        ];

        if matches!(element, Element::Arrow(_))
            && let Some((text_id, _)) = element
                .bound_elements()
                .into_iter()
                .find(|&(_, kind)| kind == "text")
            && let Some(placement) = index_by_id
                .get(text_id)
                .and_then(|&index| file.elements[index].placement())
        {
            let label_aabb = [
                placement.x,
                placement.y,
                placement.x + placement.width,
                placement.y + placement.height,
            ];
            element_aabb = [
                element_aabb[0].min(label_aabb[0]),
                element_aabb[1].min(label_aabb[1]),
                element_aabb[2].max(label_aabb[2]),
                element_aabb[3].max(label_aabb[3]),
            ];
        }

        if bounds_contain(&selection_bounds, &element_aabb) {
            contained.insert(id);
        }
    }

    let mut result = contained.clone();
    for &id in &contained {
        let Some(&outermost) = index_by_id
            .get(id)
            .map(|&index| file.elements[index].group_ids())
            .as_ref()
            .and_then(|group_ids| group_ids.last())
        else {
            continue;
        };
        if let Some(members) = groups.get(outermost)
            && !members.iter().all(|member| contained.contains(member))
        {
            result.remove(id);
        }
    }

    Selection::from_ids(result)
}

/// `hasBoundingBox`, taking the size of the whole current selection directly rather than a
/// single-element array: `hitElement` is the only caller, and it always wants the answer for
/// the whole selection, so the "more than one element" branch checks the selection's size
/// instead of a length-1 array that could never satisfy it.
fn has_bounding_box(element: &Element, selected_count: usize) -> bool {
    if selected_count > 1 {
        return true;
    }
    match element {
        Element::Arrow(l) if l.elbowed == Some(true) => false,
        Element::Line(l) | Element::Arrow(l) => l.points.len() > 2,
        Element::Raw(v) => {
            let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
            if kind == "arrow" && v.get("elbowed").and_then(Value::as_bool) == Some(true) {
                return false;
            }
            if kind == "line" || kind == "arrow" {
                let points = v
                    .get("points")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                return points > 2;
            }
            true
        }
        _ => true,
    }
}

/// `hitElementBoundingBox`: whether `point`, rotated back into the element's own unrotated
/// frame, falls within its unrotated bounds widened by `tolerance`. `pub(crate)`: the editor's
/// selection tool reuses it for its own `hitElementBoundingBoxOnly` check (a release-time click
/// that only grazed the padded bounding box, not the element's own outline or bound text).
pub(crate) fn hit_element_bounding_box(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
    tolerance: f64,
) -> bool {
    let Some((unrotated, center)) = geometry.absolute_coords(element) else {
        return false;
    };
    let angle = element.placement().map_or(0.0, |p| p.angle);
    let local = if angle == 0.0 {
        point
    } else {
        rotate_point(point, center, -angle)
    };
    let [x1, y1, x2, y2] = unrotated;
    local[0] >= x1 - tolerance
        && local[0] <= x2 + tolerance
        && local[1] >= y1 - tolerance
        && local[1] <= y2 + tolerance
}

/// `hitElementBoundText`: whether `point` falls inside `element`'s bound text label, using the
/// label's own stored placement (see the module doc comment for why
/// `getBoundTextElementPosition` is not ported). `pub(crate)`, for the same reason as
/// [`hit_element_bounding_box`].
pub(crate) fn hit_element_bound_text(
    geometry: &mut GeometryCache,
    elements: &[Element],
    index_by_id: &HashMap<&str, usize>,
    element: &Element,
    point: [f64; 2],
) -> bool {
    let Some((text_id, _)) = element
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")
    else {
        return false;
    };
    let Some(&index) = index_by_id.get(text_id) else {
        return false;
    };
    is_point_in_element(geometry, &elements[index], point)
}

/// `hitElement`: a selected element (with a bounding box, see [`has_bounding_box`]) is hit
/// anywhere inside that box; otherwise a bound text label counts as part of its owner; failing
/// both, the element's own outline decides.
fn hit_element(
    geometry: &mut GeometryCache,
    elements: &[Element],
    index_by_id: &HashMap<&str, usize>,
    element: &Element,
    point: [f64; 2],
    zoom: f64,
    selection: &Selection,
) -> bool {
    let threshold = hit_threshold(element, zoom);

    if element.id().is_some_and(|id| selection.contains(id))
        && has_bounding_box(element, selection.len())
        && hit_element_bounding_box(geometry, element, point, threshold)
    {
        return true;
    }

    if hit_element_bound_text(geometry, elements, index_by_id, element, point) {
        return true;
    }

    hit_element_itself(geometry, element, point, threshold)
}

/// `getElementsAtPosition`: positions of hit elements in z-order (bottom first), with
/// `embeddable`/`iframe` elements moved to the end regardless of their actual stacking order.
/// Locked, deleted and container-bound-text elements never hit directly (`is_selectable`).
pub fn elements_at(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    point: [f64; 2],
    zoom: f64,
    selection: &Selection,
) -> Vec<usize> {
    let index_by_id: HashMap<&str, usize> = file
        .elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| element.id().map(|id| (id, index)))
        .collect();

    let mut hits = Vec::new();
    let mut iframe_like = Vec::new();
    for (index, element) in file.elements.iter().enumerate() {
        if !is_selectable(element) {
            continue;
        }
        if hit_element(
            geometry,
            &file.elements,
            &index_by_id,
            element,
            point,
            zoom,
            selection,
        ) {
            if matches!(element.kind(), "iframe" | "embeddable") {
                iframe_like.push(index);
            } else {
                hits.push(index);
            }
        }
    }
    hits.extend(iframe_like);
    hits
}

/// `getElementAtPosition`: the topmost hit, re-tested with `hitElementItself` at half its
/// threshold for precision when it overlaps another hit element; the element below it wins
/// when that precise retest fails.
pub fn element_at(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    point: [f64; 2],
    zoom: f64,
    selection: &Selection,
) -> Option<usize> {
    let hits = elements_at(geometry, file, point, zoom, selection);
    match hits.len() {
        0 => None,
        1 => Some(hits[0]),
        _ => {
            let top = hits[hits.len() - 1];
            let element = &file.elements[top];
            let threshold = hit_threshold(element, zoom) / 2.0;
            if hit_element_itself(geometry, element, point, threshold) {
                Some(top)
            } else {
                Some(hits[hits.len() - 2])
            }
        }
    }
}

/// `getCommonBounds` of the selected, non-deleted elements.
pub fn selected_bounds(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
) -> Option<Bounds> {
    geometry.common_bounds(
        file.elements
            .iter()
            .filter(|element| element.id().is_some_and(|id| selection.contains(id))),
    )
}

/// `isHittingCommonBoundingBoxOfSelectedElements`: with two or more elements selected, whether
/// `point` falls within their common bounds, widened by `DEFAULT_TRANSFORM_HANDLE_SPACING * 2
/// / zoom` plus `max(DEFAULT_COLLISION_THRESHOLD / zoom, 1)`.
pub fn hits_selection_box(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
    point: [f64; 2],
    zoom: f64,
) -> bool {
    if selection.len() < 2 {
        return false;
    }
    let Some([x1, y1, x2, y2]) = selected_bounds(geometry, file, selection) else {
        return false;
    };
    let threshold = (DEFAULT_COLLISION_THRESHOLD / zoom).max(1.0);
    let padding = DEFAULT_TRANSFORM_HANDLE_SPACING * 2.0 / zoom;
    point[0] > x1 - padding - threshold
        && point[0] < x2 + padding + threshold
        && point[1] > y1 - padding - threshold
        && point[1] < y2 + padding + threshold
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::sample;

    fn rect(id: &str, rect: [f64; 4]) -> Value {
        sample::generic("rectangle", id, rect)
    }

    fn at(file: &SceneFile, point: [f64; 2], selection: &Selection) -> Option<String> {
        let mut geometry = GeometryCache::default();
        element_at(&mut geometry, file, point, 1.0, selection)
            .map(|i| file.elements[i].id().unwrap().to_string())
    }

    #[test]
    fn topmost_wins_unless_its_precise_retest_fails() {
        let file = sample::file(vec![
            sample::with(
                rect("below", [0.0, 0.0, 100.0, 100.0]),
                json!({"backgroundColor": "#ffc9c9"}),
            ),
            rect("above", [0.0, 45.0, 100.0, 100.0]),
        ]);
        let none = Selection::new();
        // 5 units from `above`'s top edge: inside the threshold (6.8) but not half of it.
        assert_eq!(at(&file, [50.0, 50.0], &none).as_deref(), Some("below"));
        assert_eq!(at(&file, [50.0, 47.0], &none).as_deref(), Some("above"));
        assert_eq!(at(&file, [300.0, 300.0], &none), None);
    }

    #[test]
    fn deleted_locked_and_bound_text_are_not_hit_directly() {
        let file = sample::file(vec![
            sample::with(
                rect("deleted", [0.0, 0.0, 100.0, 100.0]),
                json!({"isDeleted": true, "backgroundColor": "#ffc9c9"}),
            ),
            sample::with(
                rect("locked", [200.0, 0.0, 100.0, 100.0]),
                json!({"locked": true, "backgroundColor": "#ffc9c9"}),
            ),
            sample::with(
                sample::with(
                    sample::linear("arrow", "a", [0.0, 300.0], &[[0.0, 0.0], [200.0, 0.0]]),
                    json!({"roughness": 0, "roundness": null}),
                ),
                json!({"boundElements": [{"id": "label", "type": "text"}]}),
            ),
            sample::text("label", [80.0, 290.0, 40.0, 20.0], "hi", Some("a")),
        ]);
        let none = Selection::new();
        assert_eq!(at(&file, [50.0, 50.0], &none), None);
        assert_eq!(at(&file, [250.0, 50.0], &none), None);
        // 9 units from the arrow, inside its label: the arrow is hit, never the label itself.
        assert_eq!(at(&file, [100.0, 309.0], &none).as_deref(), Some("a"));
        let mut geometry = GeometryCache::default();
        assert_eq!(
            elements_at(&mut geometry, &file, [100.0, 300.0], 1.0, &none),
            vec![2]
        );
    }

    #[test]
    fn a_selected_element_is_hit_anywhere_in_its_box() {
        let file = sample::file(vec![rect("r", [0.0, 0.0, 100.0, 100.0])]);
        assert_eq!(at(&file, [50.0, 50.0], &Selection::new()), None);
        assert_eq!(
            at(&file, [50.0, 50.0], &Selection::from_ids(["r"])).as_deref(),
            Some("r")
        );
    }

    #[test]
    fn groups_select_their_outermost_members() {
        let grouped = |id: &str, groups: &[&str]| {
            sample::with(
                rect(id, [0.0, 0.0, 10.0, 10.0]),
                json!({"groupIds": groups}),
            )
        };
        let file = sample::file(vec![
            grouped("a", &["g1"]),
            grouped("b", &["g1"]),
            grouped("c", &["lonely"]),
            grouped("d", &["inner", "outer"]),
            grouped("e", &["outer"]),
            sample::with(grouped("f", &["g1"]), json!({"isDeleted": true})),
        ]);
        assert_eq!(
            select_groups(&file, &Selection::from_ids(["a"])),
            Selection::from_ids(["a", "b"])
        );
        assert_eq!(
            select_groups(&file, &Selection::from_ids(["d"])),
            Selection::from_ids(["d", "e"])
        );
        assert_eq!(
            select_groups(&file, &Selection::from_ids(["c"])),
            Selection::from_ids(["c"])
        );
    }

    #[test]
    fn select_all_skips_deleted_locked_and_bound_text() {
        let file = sample::file(vec![
            rect("r", [0.0, 0.0, 10.0, 10.0]),
            sample::with(
                rect("deleted", [0.0, 0.0, 10.0, 10.0]),
                json!({"isDeleted": true}),
            ),
            sample::with(
                rect("locked", [0.0, 0.0, 10.0, 10.0]),
                json!({"locked": true}),
            ),
            sample::text("bound", [0.0, 0.0, 10.0, 10.0], "hi", Some("r")),
            sample::text("free", [0.0, 0.0, 10.0, 10.0], "hi", None),
        ]);
        assert_eq!(select_all(&file), Selection::from_ids(["r", "free"]));
    }

    #[test]
    fn box_selection_needs_full_containment_including_half_the_stroke() {
        let file = sample::file(vec![
            rect("r", [10.0, 10.0, 80.0, 80.0]),
            sample::with(
                rect("g1", [200.0, 0.0, 10.0, 10.0]),
                json!({"groupIds": ["g"]}),
            ),
            sample::with(
                rect("g2", [300.0, 0.0, 10.0, 10.0]),
                json!({"groupIds": ["g"]}),
            ),
        ]);
        let mut geometry = GeometryCache::default();
        assert!(box_select(&mut geometry, &file, [10.0, 10.0, 90.0, 90.0]).is_empty());
        assert_eq!(
            box_select(&mut geometry, &file, [9.0, 9.0, 91.0, 91.0]),
            Selection::from_ids(["r"])
        );
        assert!(box_select(&mut geometry, &file, [190.0, -10.0, 250.0, 20.0]).is_empty());
        assert_eq!(
            box_select(&mut geometry, &file, [190.0, -10.0, 350.0, 20.0]),
            Selection::from_ids(["g1", "g2"])
        );
    }

    #[test]
    fn selection_box_counts_only_for_multiple_elements() {
        let file = sample::file(vec![
            rect("a", [0.0, 0.0, 10.0, 10.0]),
            rect("b", [50.0, 50.0, 10.0, 10.0]),
        ]);
        let both = Selection::from_ids(["a", "b"]);
        let mut geometry = GeometryCache::default();
        assert_eq!(
            selected_bounds(&mut geometry, &file, &both),
            Some([0.0, 0.0, 60.0, 60.0])
        );
        assert!(hits_selection_box(
            &mut geometry,
            &file,
            &both,
            [30.0, 30.0],
            1.0
        ));
        // Bounds grow by 4 / zoom plus max(DEFAULT_COLLISION_THRESHOLD / zoom, 1): about 72.
        assert!(hits_selection_box(
            &mut geometry,
            &file,
            &both,
            [71.0, 30.0],
            1.0
        ));
        assert!(!hits_selection_box(
            &mut geometry,
            &file,
            &both,
            [73.0, 30.0],
            1.0
        ));
        assert!(!hits_selection_box(
            &mut geometry,
            &file,
            &Selection::from_ids(["a"]),
            [5.0, 5.0],
            1.0
        ));
    }
}
