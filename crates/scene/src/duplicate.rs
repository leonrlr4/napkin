//! `duplicateElements` and `duplicateElement` (`packages/element/src/duplicate.ts`),
//! `fixDuplicatedBindingsAfterDuplication` (`packages/element/src/binding.ts`) and
//! `normalizeElementOrder`/`defragmentGroups`/`normalizeBoundElementsOrder`
//! (`packages/element/src/sortElements.ts`), at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. Always `randomizeSeed: true`, since napkin has
//! no "retain seed" plain-paste mode; `editingGroupId` is always `None`, since napkin has no
//! group-editing UI. Frame duplication (`isFrameLikeElement`, `bindElementsToFramesAfterDuplication`)
//! is not ported: napkin has no typed frame element (spec §1.2's element list), so a frame
//! always falls back to [`Element::Raw`] and is duplicated like any other untyped element,
//! never entering the frame-specific branches.

use std::collections::{HashMap, HashSet};

use crate::element::{Element, LinearEnd};
use crate::env::{Env, random_id, random_integer};
use crate::json::Slot;
use crate::selection::Selection;

/// `DEFAULT_GRID_SIZE` (`packages/common/src/constants.ts`): half of it is
/// `actionDuplicateSelection`'s offset for [`DuplicateMode::InPlace`].
pub const DEFAULT_GRID_SIZE: f64 = 20.0;

/// How [`duplicate_elements`] places and returns the copies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DuplicateMode {
    /// `type: "in-place"`: each duplicate is offset from its original by `offset` and spliced
    /// into the returned list right after the last element of its original's group/pair (the
    /// same z-order `duplicateElements` gives, `insertBeforeOrAfterIndex`).
    InPlace { offset: [f64; 2] },
    /// `type: "everything"`: every element in `elements` is duplicated (unpositioned; the
    /// caller has already moved the input elements where it wants the copies to land),
    /// `ids` is ignored.
    Everything,
}

/// The result of [`duplicate_elements`].
pub struct Duplicated {
    /// In-place mode: the full element list with duplicates inserted next to their originals,
    /// fractional indices not yet synced (the caller runs
    /// [`crate::fractional_index::sync_moved_indices`] over `new_ids`). Everything mode: only
    /// the duplicates, in creation order, indices likewise unsynced.
    pub elements: Vec<Element>,
    /// Ids of the new elements, in creation order.
    pub new_ids: Vec<String>,
}

/// `duplicateElements` with `randomizeSeed: true`: fresh ids and seeds, `groupIds` remapped
/// consistently across every duplicated element, and bindings/containers remapped by
/// [`fix_duplicated_bindings`] (a reference to an element that was not itself duplicated is
/// dropped). `ids` is the selection to duplicate for [`DuplicateMode::InPlace`] (mirroring
/// `idsOfElementsToDuplicate`); ignored for [`DuplicateMode::Everything`], which duplicates
/// every element in `elements` (mirroring `new Map(elements.map((el) => [el.id, el]))`).
pub fn duplicate_elements(
    elements: &[Element],
    ids: &Selection,
    mode: DuplicateMode,
    env: &mut impl Env,
) -> Duplicated {
    let elements = normalize_element_order(elements);

    let selected_groups = match mode {
        DuplicateMode::InPlace { .. } => selected_group_ids(&elements, ids),
        DuplicateMode::Everything => HashSet::new(),
    };

    let mut to_duplicate: HashSet<String> = match mode {
        DuplicateMode::Everything => elements
            .iter()
            .filter_map(Element::id)
            .map(str::to_owned)
            .collect(),
        DuplicateMode::InPlace { .. } => elements
            .iter()
            .filter_map(Element::id)
            .filter(|id| ids.contains(id))
            .map(str::to_owned)
            .collect(),
    };
    // "For sanity" (duplicateElements' own comment): a fully selected group pulls in every one
    // of its members, even a locked one that is not itself selectable.
    for group_id in &selected_groups {
        for element in &elements {
            if element.group_ids().contains(&group_id.as_str())
                && let Some(id) = element.id()
            {
                to_duplicate.insert(id.to_owned());
            }
        }
    }

    let mut working: Vec<Element> = elements.clone();
    let mut processed: HashSet<String> = HashSet::new();
    let mut group_id_map: HashMap<String, String> = HashMap::new();
    let mut orig_id_to_dup_id: HashMap<String, String> = HashMap::new();
    let mut orig_by_dup_id: HashMap<String, Element> = HashMap::new();
    let mut new_ids: Vec<String> = Vec::new();

    for element in &elements {
        let Some(id) = element.id().map(str::to_owned) else {
            continue;
        };
        if processed.contains(&id) || !to_duplicate.contains(&id) {
            continue;
        }

        if let Some(group_id) = selected_group_for_element(element, &selected_groups) {
            let members: Vec<Element> = elements
                .iter()
                .filter(|e| e.group_ids().contains(&group_id.as_str()))
                .cloned()
                .collect();
            let target = find_last_index(&working, |e| e.group_ids().contains(&group_id.as_str()));
            let copies = copy_elements(
                &members,
                &mut processed,
                &mut group_id_map,
                &mut orig_id_to_dup_id,
                &mut orig_by_dup_id,
                &mut new_ids,
                env,
            );
            insert_at(&mut working, target, copies);
            continue;
        }

        if has_bound_text_element(element) {
            let bound_text_id = element
                .bound_elements()
                .into_iter()
                .find(|&(_, kind)| kind == "text")
                .map(|(bid, _)| bid.to_owned());
            let target = find_last_index(&working, |e| {
                e.id() == Some(id.as_str()) || e.container_id() == Some(id.as_str())
            });
            let group: Vec<Element> = match bound_text_id
                .and_then(|bid| elements.iter().find(|e| e.id() == Some(bid.as_str())))
            {
                Some(text) => vec![element.clone(), text.clone()],
                None => vec![element.clone()],
            };
            let copies = copy_elements(
                &group,
                &mut processed,
                &mut group_id_map,
                &mut orig_id_to_dup_id,
                &mut orig_by_dup_id,
                &mut new_ids,
                env,
            );
            insert_at(&mut working, target, copies);
            continue;
        }

        if let Some(container_id) = element.container_id().map(str::to_owned) {
            let container = elements
                .iter()
                .find(|e| e.id() == Some(container_id.as_str()));
            let target = find_last_index(&working, |e| {
                e.id() == Some(id.as_str()) || e.id() == Some(container_id.as_str())
            });
            let group: Vec<Element> = match container {
                Some(c) => vec![c.clone(), element.clone()],
                None => vec![element.clone()],
            };
            let copies = copy_elements(
                &group,
                &mut processed,
                &mut group_id_map,
                &mut orig_id_to_dup_id,
                &mut orig_by_dup_id,
                &mut new_ids,
                env,
            );
            insert_at(&mut working, target, copies);
            continue;
        }

        let target = find_last_index(&working, |e| e.id() == Some(id.as_str()));
        let copies = copy_elements(
            std::slice::from_ref(element),
            &mut processed,
            &mut group_id_map,
            &mut orig_id_to_dup_id,
            &mut orig_by_dup_id,
            &mut new_ids,
            env,
        );
        insert_at(&mut working, target, copies);
    }

    let dup_ids: HashSet<String> = orig_id_to_dup_id.values().cloned().collect();
    for element in &mut working {
        let Some(id) = element.id().map(str::to_owned) else {
            continue;
        };
        if !dup_ids.contains(&id) {
            continue;
        }
        fix_duplicated_bindings(element, &orig_id_to_dup_id);
        if let DuplicateMode::InPlace { offset } = mode
            && let Some(orig) = orig_by_dup_id.get(&id)
            && let Some(placement) = orig.placement()
        {
            element.set_position(placement.x + offset[0], placement.y + offset[1]);
        }
    }

    let result_elements = match mode {
        DuplicateMode::InPlace { .. } => working,
        DuplicateMode::Everything => working
            .into_iter()
            .filter(|e| e.id().is_some_and(|id| dup_ids.contains(id)))
            .collect(),
    };

    Duplicated {
        elements: result_elements,
        new_ids,
    }
}

/// `hasBoundTextElement`: a text-bindable container (`isTextBindableContainer`; napkin's
/// typed elements have no `stickynote`) that lists a bound text.
fn has_bound_text_element(element: &Element) -> bool {
    matches!(
        element.kind(),
        "rectangle" | "diamond" | "ellipse" | "arrow"
    ) && element
        .bound_elements()
        .iter()
        .any(|&(_, kind)| kind == "text")
}

/// The outermost-first `groupId` of `element` that names a fully selected group, `None` when
/// none of its groups are fully selected (`getSelectedGroupForElement`, `editingGroupId`
/// always `None`).
fn selected_group_for_element(element: &Element, selected: &HashSet<String>) -> Option<String> {
    element
        .group_ids()
        .into_iter()
        .find(|g| selected.contains(*g))
        .map(str::to_owned)
}

/// Every `groupId` (at any nesting level) every one of whose members is in `ids`: the
/// napkin equivalent of `appState.selectedGroupIds`, which [`crate::selection::select_groups`]
/// always keeps closed over full group membership when a selection is formed through the
/// editor, so a group is "selected" exactly when `ids` already contains all its members.
fn selected_group_ids(elements: &[Element], ids: &Selection) -> HashSet<String> {
    let mut candidates: HashSet<String> = HashSet::new();
    for element in elements {
        if element.id().is_some_and(|id| ids.contains(id)) {
            candidates.extend(element.group_ids().into_iter().map(str::to_owned));
        }
    }
    candidates.retain(|group_id| {
        elements.iter().all(|e| {
            !e.group_ids().contains(&group_id.as_str()) || e.id().is_some_and(|id| ids.contains(id))
        })
    });
    candidates
}

/// `findLastIndex`.
fn find_last_index(elements: &[Element], pred: impl Fn(&Element) -> bool) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| pred(e))
        .map(|(i, _)| i)
}

/// `insertBeforeOrAfterIndex`: appends `new_elements` when `target` sits at or past the end of
/// `list`; otherwise splices them in right after `target`. `target` of `None` behaves like
/// JS's `-1` (not found): insert at the very front.
fn insert_at(list: &mut Vec<Element>, target: Option<usize>, new_elements: Vec<Element>) {
    if new_elements.is_empty() {
        return;
    }
    let pos = target.map_or(0, |i| i + 1);
    if pos >= list.len() {
        list.extend(new_elements);
    } else {
        let tail = list.split_off(pos);
        list.extend(new_elements);
        list.extend(tail);
    }
}

/// `copyElements`: duplicates every not-yet-processed element of `group` (in order), recording
/// each in the shared bookkeeping maps `duplicateElements` threads through the whole call.
fn copy_elements(
    group: &[Element],
    processed: &mut HashSet<String>,
    group_id_map: &mut HashMap<String, String>,
    orig_id_to_dup_id: &mut HashMap<String, String>,
    orig_by_dup_id: &mut HashMap<String, Element>,
    new_ids: &mut Vec<String>,
    env: &mut impl Env,
) -> Vec<Element> {
    let mut copies = Vec::new();
    for orig in group {
        let Some(id) = orig.id().map(str::to_owned) else {
            continue;
        };
        if processed.contains(&id) {
            continue;
        }
        processed.insert(id.clone());
        let dup = duplicate_element(orig, group_id_map, env);
        let dup_id = dup.id().expect("duplicated element has an id").to_owned();
        processed.insert(dup_id.clone());
        orig_id_to_dup_id.insert(id, dup_id.clone());
        orig_by_dup_id.insert(dup_id.clone(), orig.clone());
        new_ids.push(dup_id);
        copies.push(dup);
    }
    copies
}

/// `duplicateElement` with `randomizeSeed: true`: a fresh id, a stamped `updated`/`created`, a
/// fresh `seed` plus the version bump `bumpVersion` would give it (spec §5.3, fused into one
/// JSON edit rather than calling [`crate::new_element::bump_version`] separately, which would
/// stamp `updated` a second time), and `groupIds` remapped through `group_id_map` so every
/// element duplicated in the same call agrees on each old group's new id.
fn duplicate_element(
    orig: &Element,
    group_id_map: &mut HashMap<String, String>,
    env: &mut impl Env,
) -> Element {
    let new_group_ids: Vec<String> = orig
        .group_ids()
        .into_iter()
        .map(|old| {
            group_id_map
                .entry(old.to_owned())
                .or_insert_with(|| random_id(env))
                .clone()
        })
        .collect();

    let mut value = orig.to_value();
    let now = env.now_ms();
    let new_id = random_id(env);
    let seed = random_integer(env);
    let version_nonce = random_integer(env);
    if let Some(map) = value.as_object_mut() {
        let version = map
            .get("version")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        map.insert("id".into(), serde_json::json!(new_id));
        map.insert("updated".into(), serde_json::json!(now));
        map.insert("created".into(), serde_json::json!(now));
        map.insert("seed".into(), serde_json::json!(seed));
        map.insert("version".into(), serde_json::json!(version + 1.0));
        map.insert("versionNonce".into(), serde_json::json!(version_nonce));
        map.insert("groupIds".into(), serde_json::json!(new_group_ids));
    }
    Element::from_value(value)
}

/// `startBinding`/`endBinding`'s JSON key for `end`.
fn binding_field(end: LinearEnd) -> &'static str {
    match end {
        LinearEnd::Start => "startBinding",
        LinearEnd::End => "endBinding",
    }
}

/// `fixDuplicatedBindingsAfterDuplication`, minus the elbow-arrow point recompute (napkin
/// cannot create or duplicate an elbow arrow's routing, spec §1.2): remaps `boundElements`,
/// `containerId` and `startBinding`/`endBinding.elementId` through `remap` (old id -> new id),
/// dropping a reference whose target was not itself duplicated.
fn fix_duplicated_bindings(element: &mut Element, remap: &HashMap<String, String>) {
    let bound: Vec<(String, String)> = element
        .bound_elements()
        .into_iter()
        .map(|(id, kind)| (id.to_owned(), kind.to_owned()))
        .collect();
    for (old_id, kind) in &bound {
        element.remove_bound_element(old_id);
        if let Some(new_id) = remap.get(old_id) {
            element.add_bound_element(new_id, kind);
        }
    }

    if let Element::Text(text) = element
        && let Some(container_id) = text.container_id.value().cloned()
    {
        text.container_id = remap
            .get(&container_id)
            .cloned()
            .map_or(Slot::Null, Slot::Value);
    }

    for end in [LinearEnd::Start, LinearEnd::End] {
        let Some(target_id) = element.binding_target(end).map(str::to_owned) else {
            continue;
        };
        match remap.get(&target_id) {
            Some(new_id) => {
                let mut binding = element
                    .to_value()
                    .get(binding_field(end))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                if let Some(map) = binding.as_object_mut() {
                    map.insert("elementId".into(), serde_json::json!(new_id));
                }
                element.set_binding(end, binding);
            }
            None => element.clear_binding(end),
        }
    }
}

/// `normalizeElementOrder`: makes every group's members contiguous ([`defragment_groups`]),
/// then moves a bound text element right after its container
/// ([`normalize_bound_elements_order`]).
fn normalize_element_order(elements: &[Element]) -> Vec<Element> {
    normalize_bound_elements_order(defragment_groups(elements))
}

/// `defragmentGroups`: recursively buckets elements by their `groupId` at each nesting level
/// (outermost first), preserving each bucket's and each loose element's first-occurrence
/// position, so every group ends up contiguous without disturbing relative order otherwise.
fn defragment_groups(elements: &[Element]) -> Vec<Element> {
    fn group_id_at_level(element: &Element, level: usize) -> Option<&str> {
        let ids = element.group_ids();
        ids.len().checked_sub(level + 1).map(|i| ids[i])
    }

    fn order_level(level_elements: Vec<Element>, level: usize) -> Vec<Element> {
        enum Slot {
            Loose(Box<Element>),
            Group(usize),
        }
        let mut slots: Vec<Slot> = Vec::new();
        let mut bucket_index: HashMap<String, usize> = HashMap::new();
        let mut buckets: Vec<Vec<Element>> = Vec::new();
        for element in level_elements {
            match group_id_at_level(&element, level).map(str::to_owned) {
                None => slots.push(Slot::Loose(Box::new(element))),
                Some(group_id) => {
                    let idx = *bucket_index.entry(group_id).or_insert_with(|| {
                        buckets.push(Vec::new());
                        slots.push(Slot::Group(buckets.len() - 1));
                        buckets.len() - 1
                    });
                    buckets[idx].push(element);
                }
            }
        }
        slots
            .into_iter()
            .flat_map(|slot| match slot {
                Slot::Loose(e) => vec![*e],
                Slot::Group(idx) => order_level(std::mem::take(&mut buckets[idx]), level + 1),
            })
            .collect()
    }

    order_level(elements.to_vec(), 0)
}

/// `normalizeBoundElementsOrder`: a container is immediately followed by its bound text label,
/// preferring the container's own z-order; a bound text element elsewhere in the array is
/// dropped from its own position (it will already have been placed by its container).
fn normalize_bound_elements_order(elements: Vec<Element>) -> Vec<Element> {
    let by_id: HashMap<String, Element> = elements
        .iter()
        .filter_map(|e| e.id().map(|id| (id.to_owned(), e.clone())))
        .collect();
    let mut added: HashSet<String> = HashSet::new();
    let mut result: Vec<Element> = Vec::new();

    for element in &elements {
        let Some(id) = element.id() else {
            result.push(element.clone());
            continue;
        };
        if added.contains(id) {
            continue;
        }

        let bound_elements = element.bound_elements();
        if !bound_elements.is_empty() {
            let bound_text = bound_elements.into_iter().find(|&(_, kind)| kind == "text");
            added.insert(id.to_owned());
            result.push(element.clone());
            if let Some((text_id, _)) = bound_text
                && let Some(text) = by_id.get(text_id)
            {
                added.insert(text_id.to_owned());
                result.push(text.clone());
            }
            continue;
        }

        if element.kind() == "text"
            && let Some(container_id) = element.container_id()
            && let Some(container) = by_id.get(container_id)
            && container.bound_elements().iter().any(|&(bid, _)| bid == id)
        {
            continue;
        }

        added.insert(id.to_owned());
        result.push(element.clone());
    }

    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample;

    /// Distinct random bytes on every call, so every generated id/seed/group id is unique
    /// (matching `crates/scene/tests/baseline.rs`'s `DistinctIdEnv`).
    #[derive(Default)]
    struct TestEnv {
        next: u8,
    }

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(self.next);
            self.next = self.next.wrapping_add(1);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    #[test]
    fn everything_mode_duplicates_every_element_unpositioned_and_in_order() {
        let elements = vec![
            Element::from_value(sample::generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0])),
            Element::from_value(sample::generic("ellipse", "e", [50.0, 0.0, 10.0, 10.0])),
        ];
        let result = duplicate_elements(
            &elements,
            &Selection::new(),
            DuplicateMode::Everything,
            &mut TestEnv::default(),
        );
        assert_eq!(result.elements.len(), 2);
        assert_eq!(result.new_ids.len(), 2);
        // Positions are untouched: `Everything` never applies an offset of its own.
        assert_eq!(result.elements[0].placement().unwrap().x, 0.0);
        assert_eq!(result.elements[1].placement().unwrap().x, 50.0);
        for (orig, dup) in elements.iter().zip(&result.elements) {
            assert_ne!(orig.id(), dup.id());
            assert_eq!(dup.kind(), orig.kind());
        }
    }

    #[test]
    fn in_place_drops_a_binding_to_an_element_that_was_not_duplicated() {
        let elements = vec![
            Element::from_value(sample::generic("rectangle", "r1", [0.0, 0.0, 10.0, 10.0])),
            Element::from_value(sample::with(
                sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]),
                json!({"startBinding": {"elementId": "r1", "focus": 0.0, "gap": 5.0}}),
            )),
        ];
        let result = duplicate_elements(
            &elements,
            &Selection::from_ids(["a"]),
            DuplicateMode::InPlace { offset: [1.0, 1.0] },
            &mut TestEnv::default(),
        );
        // Only the arrow was duplicated; `r1` was not, so the copy's binding is dropped.
        assert_eq!(result.new_ids.len(), 1);
        let dup = result
            .elements
            .iter()
            .find(|e| e.id() == Some(result.new_ids[0].as_str()))
            .unwrap();
        assert_eq!(dup.binding_target(LinearEnd::Start), None);
    }
}
