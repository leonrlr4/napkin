//! Moving selected elements one layer up or down, ported from `packages/element/src/zindex.ts`
//! (`getIndicesToMove`, `toContiguousGroups`, `getTargetIndex`,
//! `getTargetIndexAccountingForBinding`, `shiftElementsByOne`, `moveOneLeft`, `moveOneRight`) at
//! the pinned commit. `moveAllLeft`/`moveAllRight` (`Ctrl+Shift+[`/`]`, `shiftElementsToEnd`) are
//! not ported: napkin only binds `Ctrl+[`/`Ctrl+]`.
//!
//! napkin has no "editing group" state (no equivalent of Excalidraw double-clicking into a
//! group), so every place the JS reads `appState.editingGroupId` is ported with that value
//! hardcoded to `null`; the branches that only trigger when it is set are dropped rather than
//! kept as dead code. Frame bookkeeping (`containingFrame`, `getContiguousFrameRangeElements`)
//! is ported in full even though napkin does not yet draw frame elements specially, since a
//! loaded `.excalidraw` file can still contain `frameId`-tagged elements.

use std::collections::HashSet;

use crate::element::Element;
use crate::env::Env;
use crate::fractional_index::sync_moved_indices;
use crate::selection::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
}

/// `isFrameLikeElement`.
fn is_frame_like(element: &Element) -> bool {
    matches!(element.kind(), "frame" | "magicframe")
}

/// `isOfTargetFrame`.
fn is_of_target_frame(element: &Element, frame_id: &str) -> bool {
    element.frame_id() == Some(frame_id) || element.id() == Some(frame_id)
}

/// First index in `elements` at or after `from` (clamped into range) for which `pred` holds
/// (`findIndex`, `packages/common/src/utils.ts`).
fn find_index(elements: &[Element], from: usize, pred: impl Fn(&Element) -> bool) -> Option<usize> {
    let from = from.min(elements.len());
    elements[from..].iter().position(pred).map(|i| i + from)
}

/// Last index in `elements` at or before `from` (clamped into range) for which `pred` holds
/// (`findLastIndex`).
fn find_last_index(
    elements: &[Element],
    from: usize,
    pred: impl Fn(&Element) -> bool,
) -> Option<usize> {
    if elements.is_empty() {
        return None;
    }
    let from = from.min(elements.len() - 1);
    (0..=from).rev().find(|&i| pred(&elements[i]))
}

/// First position in `elements` whose id is `id`, standing in for the source's `elements.indexOf`
/// (identity comparison on array elements that all came from the same array): ids are unique in
/// a well-formed document, so looking one up by id lands on the same position.
fn index_of_id(elements: &[Element], id: &str) -> Option<usize> {
    elements.iter().position(|e| e.id() == Some(id))
}

/// `isBoundToContainer`-filtered `containerId`, and the first non-`"arrow"` `boundElements`
/// entry's id: both treat an empty string as absent, same as JS `containerId &&` / a truthy
/// `boundElementId`.
fn truthy(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `getContiguousFrameRangeElements`, returning just the first/last matching position instead of
/// the slice between them (`get_target_index` only ever reads those two positions back out of
/// it via `indexOf`).
fn contiguous_frame_range(elements: &[Element], frame_id: &str) -> Option<(usize, usize)> {
    let mut range = None;
    for (i, element) in elements.iter().enumerate() {
        if is_of_target_frame(element, frame_id) {
            range = Some(match range {
                None => (i, i),
                Some((start, _)) => (start, i),
            });
        }
    }
    range
}

/// `getTargetIndexAccountingForBinding`: a tightly-bound label and its container (or a
/// container and its label) count as one unit, so the near edge of the pair is the target when
/// only its outer element was reached.
fn target_index_accounting_for_binding(
    next_element: &Element,
    elements: &[Element],
    direction: Direction,
) -> Option<usize> {
    let next_index = index_of_id(elements, next_element.id()?)?;

    if let Some(container_id) = truthy(next_element.container_id()) {
        let container_index = index_of_id(elements, container_id)?;
        return Some(match direction {
            Direction::Left => container_index.min(next_index),
            Direction::Right => container_index.max(next_index),
        });
    }

    let bound_text_id = next_element
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind != "arrow")
        .map(|(id, _)| id);
    let bound_text_id = truthy(bound_text_id)?;
    let bound_index = index_of_id(elements, bound_text_id)?;
    Some(match direction {
        Direction::Left => bound_index.min(next_index),
        Direction::Right => bound_index.max(next_index),
    })
}

/// Positions of every element whose outermost group is `group_id`, in array order
/// (`getElementsInGroup`, returning positions instead of elements).
fn elements_in_group(elements: &[Element], group_id: &str) -> Vec<usize> {
    elements
        .iter()
        .enumerate()
        .filter(|(_, e)| e.group_ids().contains(&group_id))
        .map(|(i, _)| i)
        .collect()
}

/// `getTargetIndex` with `appState.editingGroupId` hardcoded to `null` (see the module doc
/// comment): the next non-deleted candidate in the move direction (restricted to
/// `containing_frame`'s children when moving frame contents), widened to cover a frame range, a
/// tightly-bound label/container pair, or a whole sibling group when the candidate belongs to
/// one. `None` when there is no candidate (`-1` in the source).
fn get_target_index(
    elements: &[Element],
    boundary_index: usize,
    direction: Direction,
    containing_frame: Option<&str>,
) -> Option<usize> {
    let index_filter = |element: &Element| {
        if element.is_deleted() {
            return false;
        }
        match containing_frame {
            Some(frame) => element.frame_id() == Some(frame),
            None => true,
        }
    };

    let candidate_index = match direction {
        Direction::Left => {
            find_last_index(elements, boundary_index.saturating_sub(1), index_filter)
        }
        Direction::Right => find_index(elements, boundary_index + 1, index_filter),
    }?;

    let next_element = &elements[candidate_index];

    if containing_frame.is_none()
        && (next_element.frame_id().is_some() || is_frame_like(next_element))
    {
        let frame_id = truthy(next_element.frame_id()).or_else(|| next_element.id())?;
        let (range_start, range_end) = contiguous_frame_range(elements, frame_id)?;
        return Some(match direction {
            Direction::Left => range_start,
            Direction::Right => range_end,
        });
    }

    if next_element.group_ids().is_empty() {
        return Some(
            target_index_accounting_for_binding(next_element, elements, direction)
                .unwrap_or(candidate_index),
        );
    }

    let sibling_group_id = *next_element
        .group_ids()
        .last()
        .expect("checked non-empty above");
    let sibling_group = elements_in_group(elements, sibling_group_id);
    if !sibling_group.is_empty() {
        return Some(match direction {
            Direction::Left => sibling_group[0],
            Direction::Right => *sibling_group.last().expect("checked non-empty above"),
        });
    }

    Some(candidate_index)
}

/// Ids `getSelectedElements(elements, appState, { includeBoundTextElement: true,
/// includeElementsInFrames: true })` would select, expanded from the raw ids in `selected`:
/// non-deleted bound labels of a selected container join first, then the non-deleted children of
/// any selected frame-like element.
fn expand_selected_ids(elements: &[Element], selected: &Selection) -> HashSet<String> {
    let mut ids: HashSet<String> = HashSet::new();
    for element in elements {
        if element.is_deleted() {
            continue;
        }
        let Some(id) = element.id() else { continue };
        if selected.contains(id) {
            ids.insert(id.to_owned());
            continue;
        }
        if let Some(container_id) = truthy(element.container_id())
            && selected.contains(container_id)
        {
            ids.insert(id.to_owned());
        }
    }

    let frame_owners: Vec<&str> = elements
        .iter()
        .filter(|e| {
            !e.is_deleted() && e.id().is_some_and(|id| ids.contains(id)) && is_frame_like(e)
        })
        .filter_map(Element::id)
        .collect();
    if !frame_owners.is_empty() {
        for element in elements {
            if element.is_deleted() {
                continue;
            }
            let Some(id) = element.id() else { continue };
            if element
                .frame_id()
                .is_some_and(|f| frame_owners.contains(&f))
            {
                ids.insert(id.to_owned());
            }
        }
    }

    ids
}

/// `getIndicesToMove`: positions of the (expanded) selected elements, plus any run of deleted
/// elements bridging two of them.
fn get_indices_to_move(elements: &[Element], selected_ids: &HashSet<String>) -> Vec<usize> {
    let mut selected_indices = Vec::new();
    let mut deleted_indices: Vec<usize> = Vec::new();
    let mut include_deleted_index: Option<usize> = None;

    for (index, element) in elements.iter().enumerate() {
        let is_selected = element.id().is_some_and(|id| selected_ids.contains(id));
        if is_selected {
            selected_indices.append(&mut deleted_indices);
            selected_indices.push(index);
            include_deleted_index = Some(index + 1);
        } else if element.is_deleted() && include_deleted_index == Some(index) {
            include_deleted_index = Some(index + 1);
            deleted_indices.push(index);
        } else {
            deleted_indices.clear();
        }
    }
    selected_indices
}

/// `toContiguousGroups`.
fn to_contiguous_groups(indices: &[usize]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, &value) in indices.iter().enumerate() {
        if i == 0 || indices[i - 1] + 1 != value {
            groups.push(Vec::new());
        }
        groups
            .last_mut()
            .expect("pushed above when needed")
            .push(value);
    }
    groups
}

/// `hasSameElementIds`: same length, same multiset of ids (ignoring order).
fn has_same_element_ids(prev: &[Element], next: &[Element]) -> bool {
    if prev.len() != next.len() {
        return false;
    }
    let mut remaining: Vec<&str> = prev.iter().filter_map(Element::id).collect();
    if remaining.len() != prev.len() {
        return false;
    }
    for element in next {
        let Some(id) = element.id() else {
            return false;
        };
        match remaining.iter().position(|&r| r == id) {
            Some(i) => {
                remaining.swap_remove(i);
            }
            None => return false,
        }
    }
    true
}

/// `moveOneLeft`/`moveOneRight` (`shiftElementsByOne`): the reordered elements, indices synced
/// with [`sync_moved_indices`] for the moved ones. `None` when nothing moves, i.e. the result
/// would be identical to `elements` (this holds even when a group's own move was rejected, since
/// napkin still lets `sync_moved_indices` regenerate that group's index in place; a rejected move
/// only reaches `None` when the regenerated index happens to equal the element's current one).
pub fn move_one(
    elements: &[Element],
    selected: &Selection,
    direction: Direction,
    env: &mut impl Env,
) -> Option<Vec<Element>> {
    let selected_ids = expand_selected_ids(elements, selected);
    let indices_to_move = get_indices_to_move(elements, &selected_ids);
    let moved_ids: HashSet<String> = indices_to_move
        .iter()
        .filter_map(|&i| elements[i].id())
        .map(String::from)
        .collect();

    let mut grouped_indices = to_contiguous_groups(&indices_to_move);
    if direction == Direction::Right {
        grouped_indices.reverse();
    }

    let selected_frame_ids: HashSet<&str> = indices_to_move
        .iter()
        .map(|&i| &elements[i])
        .filter(|e| is_frame_like(e))
        .filter_map(Element::id)
        .collect();

    let mut working: Vec<Element> = elements.to_vec();

    for indices in &grouped_indices {
        let leading_index = indices[0];
        let trailing_index = indices[indices.len() - 1];
        let boundary_index = match direction {
            Direction::Left => leading_index,
            Direction::Right => trailing_index,
        };

        let containing_frame: Option<String> = if indices.iter().any(|&idx| {
            working[idx]
                .frame_id()
                .is_some_and(|f| selected_frame_ids.contains(f))
        }) {
            None
        } else {
            working[boundary_index].frame_id().map(String::from)
        };

        let Some(target_index) = get_target_index(
            &working,
            boundary_index,
            direction,
            containing_frame.as_deref(),
        ) else {
            continue;
        };
        if boundary_index == target_index {
            continue;
        }

        working = match direction {
            Direction::Left => {
                let leading = working[..target_index].to_vec();
                let target = working[leading_index..=trailing_index].to_vec();
                let displaced = working[target_index..leading_index].to_vec();
                let trailing = working[trailing_index + 1..].to_vec();
                [leading, target, displaced, trailing].concat()
            }
            Direction::Right => {
                let leading = working[..leading_index].to_vec();
                let displaced = working[trailing_index + 1..=target_index].to_vec();
                let target = working[leading_index..=trailing_index].to_vec();
                let trailing = working[target_index + 1..].to_vec();
                [leading, displaced, target, trailing].concat()
            }
        };
    }

    if !has_same_element_ids(elements, &working) {
        return None;
    }

    sync_moved_indices(&mut working, &moved_ids, env);

    (working.as_slice() != elements).then_some(working)
}
