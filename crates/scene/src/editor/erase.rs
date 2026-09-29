//! The eraser tool's pointer gesture, ported from `packages/excalidraw/eraser/index.ts`'s
//! `EraserTrail` (`updateElementsToBeErased` and its `eraserTest` hit test) and
//! `packages/excalidraw/components/App.tsx`'s `handleEraser`, the `isEraserActive` branch of
//! `onPointerUpFromPointerDownHandler` (~12377 to 12400) and `eraseElements` (~12748); all at
//! commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! `pointer_down` hits at the press position directly (`hitElementItself`), matching JS's own
//! zero-drag pointer-up fallback (`getElementsAtPosition`) rather than requiring a first move;
//! every later `pointer_move` instead tests the segment from the previous point to the current
//! one (`collision::segment_hits_element`), so a fast stroke cannot skip over a thin shape by
//! landing neither endpoint near it. `eraserTest` also lets the same stroke restore an element
//! it already marked, while Alt is held (`event.altKey`, JS's `restoreToErase`); napkin does not
//! port that Alt-to-restore mode, so an eraser stroke only ever grows `pending`, never shrinks it.
//!
//! `eraserTest` picks a bespoke tolerance per element kind (15 for freedraw,
//! `strokeWidth`-based for arrow and open line) and, for every other kind, the exact
//! intersection `intersectElementWithLineSegment` computes via `curveIntersectLineSegment`'s
//! Newton solver. `collision::segment_hits_element` uses `collision::hit_threshold` uniformly
//! instead, and a distance-to-outline test for every kind (a 16-piece polyline approximation of
//! any curved outline segment, or, for an ellipse specifically, a 32-sided polygon standing in
//! for its outline entirely, since unlike a rectanguloid or diamond it has no existing Bezier
//! deconstruction to flatten) rather than an exact intersection, so this deliberately disagrees
//! with JS's precise tolerances and true curve intersections; see this module's own
//! `collision::segment_hits_element` doc comment.

use std::collections::HashSet;
use std::sync::Arc;

use crate::collision;
use crate::edit;
use crate::element::Element;
use crate::env::Env;
use crate::file::SceneFile;
use crate::selection::{self, Selection};

use super::{Editor, PointerEvent, Tool, clone_scene};

/// The eraser tool's gesture in progress, if any.
pub(super) enum Gesture {
    None,
    Active(ActiveState),
}

pub(super) struct ActiveState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    /// The latest pointer position seen, so the next `pointer_move` can test the segment from
    /// here to its own position.
    last: [f64; 2],
    /// Ids of the elements this stroke will delete, exposed as [`super::Editor::pending_erasure`]
    /// so the app can draw them faded (`ELEMENT_READY_TO_ERASE_OPACITY`).
    pending: HashSet<String>,
}

/// [`super::Editor::pending_erasure`] while `gesture` is active, else an empty, shared set.
pub(super) fn pending(gesture: &Gesture) -> &HashSet<String> {
    static EMPTY: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
    match gesture {
        Gesture::Active(state) => &state.pending,
        Gesture::None => &EMPTY,
    }
}

pub(super) fn pointer_down(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Eraser {
        return;
    }
    let start = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();
    let mut pending = HashSet::new();
    add_point_hits(
        &editor.file,
        &mut editor.geometry,
        event.at,
        event.zoom,
        &mut pending,
    );
    editor.erase_gesture = Gesture::Active(ActiveState {
        start,
        selection_before,
        last: event.at,
        pending,
    });
}

pub(super) fn pointer_move(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Eraser {
        return;
    }
    let Gesture::Active(mut state) = std::mem::replace(&mut editor.erase_gesture, Gesture::None)
    else {
        return;
    };
    add_segment_hits(
        &editor.file,
        &mut editor.geometry,
        state.last,
        event.at,
        event.zoom,
        &mut state.pending,
    );
    state.last = event.at;
    editor.erase_gesture = Gesture::Active(state);
}

pub(super) fn pointer_up(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Eraser {
        return;
    }
    finish_gesture(editor, Some(event));
}

/// Ends an in-progress eraser stroke, whether by [`pointer_up`] or a tool switch: deletes
/// everything in [`ActiveState::pending`] as one history step (`edit::erase_selection`, which
/// also deletes a pending frame's children outright and a pending container's bound text,
/// unlike `edit::delete_selection`'s keyboard-Delete rules), then clears the pending set.
/// `event` is unused: the segment up to the release position was already covered by the last
/// `pointer_move` (or, for a plain click, by `pointer_down` itself), matching `eraseElements`
/// reading `elementsPendingErasure` as already final rather than extending the trail on
/// release. Napkin's selection is already empty throughout an eraser stroke (`Editor::set_tool`
/// clears it for any tool but Selection/Hand), so unlike `Command::Delete` there is no
/// resulting selection to update here.
pub(super) fn finish_gesture(editor: &mut Editor<impl Env>, _event: Option<PointerEvent>) {
    let Gesture::Active(state) = std::mem::replace(&mut editor.erase_gesture, Gesture::None) else {
        return;
    };
    if state.pending.is_empty() {
        return;
    }
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    edit::erase_selection(file, &state.pending, &mut editor.env);
    editor.finish_edit(&state.start, &state.selection_before);
}

/// `Command::Escape` while a stroke is in progress: abandons it outright, clearing `pending`
/// without deleting anything and leaving no history entry (`eraserTrail.endPath()`'s own
/// `elementsToErase.clear()`, but reached here instead of by a release). `None` when no eraser
/// gesture is active, so `Editor::command` falls through to its ordinary `Escape` handling.
pub(super) fn escape(editor: &mut Editor<impl Env>) -> Option<bool> {
    match std::mem::replace(&mut editor.erase_gesture, Gesture::None) {
        Gesture::None => None,
        Gesture::Active(_) => Some(true),
    }
}

/// Every non-deleted, non-locked element whose outline contains or lies within its own
/// [`collision::hit_threshold`] of `point`, folded into `pending` with its group, bound text
/// and container (`updateElementsToBeErased`'s point-hit path, taken at `pointer_down` the same
/// way JS's zero-drag pointer-up fallback, `getElementsAtPosition`, does).
fn add_point_hits(
    file: &SceneFile,
    geometry: &mut crate::geometry::GeometryCache,
    point: [f64; 2],
    zoom: f64,
    pending: &mut HashSet<String>,
) {
    for element in &file.elements {
        if element.is_deleted() || element.is_locked() {
            continue;
        }
        let Some(id) = element.id() else { continue };
        let threshold = collision::hit_threshold(element, zoom);
        if collision::hit_element_itself(geometry, element, point, threshold) {
            expand_and_insert(file, id, pending);
        }
    }
}

/// Every non-deleted, non-locked, not-yet-pending element whose outline the segment from `from`
/// to `to` crosses or comes within its own [`collision::hit_threshold`] of, folded into
/// `pending` with its group, bound text and container
/// (`updateElementsToBeErased`'s segment-hit path, taken at every `pointer_move`).
fn add_segment_hits(
    file: &SceneFile,
    geometry: &mut crate::geometry::GeometryCache,
    from: [f64; 2],
    to: [f64; 2],
    zoom: f64,
    pending: &mut HashSet<String>,
) {
    for element in &file.elements {
        if element.is_deleted() || element.is_locked() {
            continue;
        }
        let Some(id) = element.id() else { continue };
        if pending.contains(id) {
            continue;
        }
        let threshold = collision::hit_threshold(element, zoom);
        if collision::segment_hits_element(geometry, element, from, to, threshold) {
            expand_and_insert(file, id, pending);
        }
    }
}

/// Adds `hit_id`'s whole group as bare ids (`selection::select_groups`, matching
/// `updateElementsToBeErased`'s `shallowestGroupId`/`getElementsInGroup`: every member joins
/// `pending`, but none of them gets its own bound text or container examined), then, for
/// `hit_id` alone, its bound text and its own container, if either exists
/// (`hasBoundTextElement`/`getBoundTextElementId` and `isBoundToContainer`, checked only on the
/// element `eraserTest` actually matched, not on the rest of its group) so erasing a labelled
/// shape also erases its label and erasing a label also erases the shape it labels.
fn expand_and_insert(file: &SceneFile, hit_id: &str, pending: &mut HashSet<String>) {
    let grouped = selection::select_groups(file, &Selection::from_ids([hit_id.to_owned()]));
    for id in grouped.iter() {
        pending.insert(id.to_owned());
    }
    let Some(element) = find(file, hit_id) else {
        return;
    };
    if let Some(container_id) = element.container_id() {
        pending.insert(container_id.to_owned());
    }
    for (bound_id, kind) in element.bound_elements() {
        if kind == "text" {
            pending.insert(bound_id.to_owned());
        }
    }
}

fn find<'a>(file: &'a SceneFile, id: &str) -> Option<&'a Element> {
    file.elements.iter().find(|e| e.id() == Some(id))
}
