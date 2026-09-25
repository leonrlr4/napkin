//! The selection tool's pointer gestures: click to select, drag to move, drag a corner to
//! resize, drag a line or arrow's own point, and box-select. Ported from
//! `packages/excalidraw/components/App.tsx`'s `handleSelectionOnPointerDown` (~9415),
//! `onPointerMoveFromPointerDownHandler` (~10685, drag and box-select branches near 10964 and
//! 11483) and `onPointerUpFromPointerDownHandler` (click narrowing and Shift removal, ~12380 to
//! 12615); `packages/element/src/collision.ts`'s `hitElementBoundingBox`,
//! `hitElementBoundText` and `hitElementBoundingBoxOnly`; and
//! `packages/element/src/linearElementEditor.ts`'s `getPointIndexUnderCursor` and
//! `handlePointerMove`'s `pointerOffset` handling; all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::collision;
use crate::edit;
use crate::element::Element;
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{Bounds, GeometryCache, rotate_point};
use crate::selection::{self, Selection};
use crate::transform::{self, HandleKind};

use super::{Cursor, Editor, PointerEvent, Tool, clone_scene};

/// The selection tool's gesture in progress, if any.
pub(super) enum Gesture {
    None,
    /// Pointer went down without immediately starting a drag, resize or point-drag: what the
    /// first move becomes, and what pointer-up does when there is no move at all, both depend
    /// on what pointer-down hit.
    Click(ClickState),
    Drag(DragState),
    Resize(ResizeState),
    PointDrag(PointDragState),
    BoxSelect(BoxSelectState),
}

pub(super) struct ClickState {
    origin: [f64; 2],
    zoom: f64,
    hit: Option<usize>,
    in_box: bool,
    /// Whether pointer-down itself added `hit` to the selection (a fresh, previously
    /// unselected element): if so, pointer-up must not narrow or remove it again.
    added_now: bool,
    /// The selection as it stood before this pointer-down's own click-select ran; the eventual
    /// history entry for a drag, resize or point-drag started from this click needs it.
    selection_before: Selection,
}

pub(super) struct DragState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    targets: Vec<usize>,
    /// The original pointer-down position: every move recomputes the total offset from here.
    origin: [f64; 2],
}

pub(super) struct ResizeState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    targets: Vec<usize>,
    handle: HandleKind,
    /// `getResizeOffsetXY`, computed once at pointer-down and subtracted from the pointer on
    /// every move so the grabbed point stays under the cursor.
    offset: [f64; 2],
}

pub(super) struct PointDragState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    index: usize,
    /// The grab offset between the pointer-down position and the point itself, kept for every
    /// move the same way `LinearElementEditor`'s `pointerOffset` is.
    offset: [f64; 2],
}

pub(super) struct BoxSelectState {
    origin: [f64; 2],
    selection_before: Selection,
    /// The rubber band's current bounds, normalized, for [`box_selection_overlay`].
    current: Option<Bounds>,
}

/// [`crate::editor::Overlay::box_selection`] while `gesture` is a box selection, else `None`.
pub(super) fn box_selection_overlay(gesture: &Gesture) -> Option<Bounds> {
    match gesture {
        Gesture::BoxSelect(state) => state.current,
        _ => None,
    }
}

/// A line, or a non-elbow arrow, with exactly two points: [`transform::selection_handles`]
/// shows no corner handles for it, and [`crate::editor::Overlay::outlines`] shows no outline
/// for it as the sole selected element either.
pub(super) fn is_two_point_linear(element: &Element) -> bool {
    matches!(element, Element::Line(l) | Element::Arrow(l) if l.points.len() <= 2)
}

/// A line, or an arrow that is not elbowed (an elbow arrow's own points are not directly
/// editable, spec §1.2).
pub(super) fn is_plain_linear(element: &Element) -> bool {
    matches!(element, Element::Line(_))
        || matches!(element, Element::Arrow(l) if l.elbowed != Some(true))
}

/// `LinearElementEditor.getPointsGlobalCoordinates`: a line or arrow's points in scene
/// coordinates. `None` for anything but a line or arrow.
pub(super) fn scene_points(
    geometry: &mut GeometryCache,
    element: &Element,
) -> Option<Vec<[f64; 2]>> {
    let (points, base_x, base_y, angle) = match element {
        Element::Line(l) | Element::Arrow(l) => (&l.points, l.base.x, l.base.y, l.base.angle),
        _ => return None,
    };
    if angle == 0.0 {
        return Some(
            points
                .iter()
                .map(|p| [p[0] + base_x, p[1] + base_y])
                .collect(),
        );
    }
    let (_, center) = geometry.absolute_coords(element)?;
    Some(
        points
            .iter()
            .map(|p| rotate_point([p[0] + base_x, p[1] + base_y], center, angle))
            .collect(),
    )
}

/// `LinearElementEditor.getPointIndexUnderCursor`: the single selected element's point closest
/// to `pointer`, tried from the last point to the first (later points render over earlier
/// ones), within `POINT_HANDLE_SIZE + 1` CSS pixels. Only a single selected line or non-elbow
/// arrow has editable points. Returns the point's scene position too, for the grab offset.
fn hit_point(
    editor: &mut Editor<impl Env>,
    pointer: [f64; 2],
    zoom: f64,
) -> Option<(usize, usize, [f64; 2])> {
    let positions = editor.selection.positions(&editor.file);
    let position = match positions.as_slice() {
        [position] => *position,
        _ => return None,
    };
    let element = editor.file.elements[position].clone();
    if !is_plain_linear(&element) {
        return None;
    }
    let points = scene_points(&mut editor.geometry, &element)?;
    for (index, &p) in points.iter().enumerate().rev() {
        let distance = rough::js::hypot(pointer[0] - p[0], pointer[1] - p[1]);
        if distance * zoom < 11.0 {
            return Some((position, index, [pointer[0] - p[0], pointer[1] - p[1]]));
        }
    }
    None
}

pub(super) fn pointer_down(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Selection {
        return;
    }

    if let Some((position, index, offset)) = hit_point(editor, event.at, event.zoom) {
        editor.gesture = Gesture::PointDrag(PointDragState {
            start: Arc::clone(&editor.file),
            selection_before: editor.selection.clone(),
            position,
            index,
            offset,
        });
        return;
    }

    if let Some(handle) = transform::handle_at(
        &mut editor.geometry,
        &editor.file,
        &editor.selection,
        event.at,
        event.zoom,
    ) {
        let offset = transform::resize_offset(
            &mut editor.geometry,
            &editor.file,
            &editor.selection,
            handle,
            event.at,
        );
        let targets = editor.selection.positions(&editor.file);
        editor.gesture = Gesture::Resize(ResizeState {
            start: Arc::clone(&editor.file),
            selection_before: editor.selection.clone(),
            targets,
            handle,
            offset,
        });
        return;
    }

    let hit = selection::element_at(
        &mut editor.geometry,
        &editor.file,
        event.at,
        event.zoom,
        &editor.selection,
    );
    let in_box = selection::hits_selection_box(
        &mut editor.geometry,
        &editor.file,
        &editor.selection,
        event.at,
        event.zoom,
    );
    let already_selected = hit.is_some_and(|pos| {
        editor.file.elements[pos]
            .id()
            .is_some_and(|id| editor.selection.contains(id))
    });
    let selection_before = editor.selection.clone();

    if (hit.is_none() || !already_selected) && !event.modifiers.shift && !in_box {
        editor.selection = Selection::new();
    }
    let mut added_now = false;
    if let Some(pos) = hit
        && !already_selected
        && let Some(id) = editor.file.elements[pos].id()
    {
        editor.selection.insert(id.to_owned());
        editor.selection = selection::select_groups(&editor.file, &editor.selection);
        added_now = true;
    }

    editor.gesture = Gesture::Click(ClickState {
        origin: event.at,
        zoom: event.zoom,
        hit,
        in_box,
        added_now,
        selection_before,
    });
}

pub(super) fn pointer_move(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Selection {
        return;
    }
    let gesture = std::mem::replace(&mut editor.gesture, Gesture::None);
    editor.gesture = match gesture {
        Gesture::None => {
            update_cursor(editor, event);
            Gesture::None
        }
        Gesture::Click(click) => {
            if event.at == click.origin {
                Gesture::Click(click)
            } else if click.hit.is_some() || click.in_box {
                let state = begin_drag(editor, &click);
                apply_drag_move(editor, &state, event);
                Gesture::Drag(state)
            } else {
                let mut state = begin_box_select(&click);
                apply_box_select_move(editor, &mut state, event);
                Gesture::BoxSelect(state)
            }
        }
        Gesture::Drag(state) => {
            apply_drag_move(editor, &state, event);
            Gesture::Drag(state)
        }
        Gesture::Resize(state) => {
            apply_resize_move(editor, &state, event);
            Gesture::Resize(state)
        }
        Gesture::PointDrag(state) => {
            apply_point_drag_move(editor, &state, event);
            Gesture::PointDrag(state)
        }
        Gesture::BoxSelect(mut state) => {
            apply_box_select_move(editor, &mut state, event);
            Gesture::BoxSelect(state)
        }
    };
}

pub(super) fn pointer_up(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Selection {
        return;
    }
    finish_gesture(editor, Some(event));
    update_cursor(editor, event);
}

/// Ends whatever selection-tool gesture is in progress. A drag, resize or point-drag applies
/// `event` at its final position first (when one is given), then always records one history
/// entry for it (`Editor::finish_edit`, a no-op when nothing actually changed); a click or box
/// selection just ends, since there is nothing to record for either. Shared by [`pointer_up`],
/// which passes the release event, and `Editor::set_tool`, which passes `None`: a tool switch
/// mid-gesture has no release position of its own, so whatever the last `pointer_move` already
/// applied to the scene stands as the gesture's final state.
pub(super) fn finish_gesture(editor: &mut Editor<impl Env>, event: Option<PointerEvent>) {
    let gesture = std::mem::replace(&mut editor.gesture, Gesture::None);
    match gesture {
        Gesture::None => {}
        Gesture::Click(click) => {
            if let Some(event) = event {
                finish_click(editor, click, event);
            }
        }
        Gesture::Drag(state) => {
            if let Some(event) = event {
                apply_drag_move(editor, &state, event);
            }
            editor.finish_edit(&state.start, &state.selection_before);
        }
        Gesture::Resize(state) => {
            if let Some(event) = event {
                apply_resize_move(editor, &state, event);
            }
            editor.finish_edit(&state.start, &state.selection_before);
        }
        Gesture::PointDrag(state) => {
            if let Some(event) = event {
                apply_point_drag_move(editor, &state, event);
            }
            editor.finish_edit(&state.start, &state.selection_before);
        }
        // The current selection is exactly the last box-select result already; nothing more
        // to apply or record.
        Gesture::BoxSelect(_) => {}
    }
}

/// `dragSelectedElements`'s targets, computed once as the drag begins (not recomputed on every
/// move), against the selection as it stands right now (including anything pointer-down itself
/// just added to it).
fn begin_drag(editor: &Editor<impl Env>, click: &ClickState) -> DragState {
    let targets = edit::drag_targets(&editor.file, &editor.selection);
    DragState {
        start: Arc::clone(&editor.file),
        selection_before: click.selection_before.clone(),
        targets,
        origin: click.origin,
    }
}

fn begin_box_select(click: &ClickState) -> BoxSelectState {
    BoxSelectState {
        origin: click.origin,
        selection_before: click.selection_before.clone(),
        current: None,
    }
}

fn apply_drag_move(editor: &mut Editor<impl Env>, state: &DragState, event: PointerEvent) {
    let mut offset = [event.at[0] - state.origin[0], event.at[1] - state.origin[1]];
    if event.modifiers.shift {
        offset = edit::lock_drag_axis(offset);
    }
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    edit::apply_drag(file, &state.start, &state.targets, offset, &mut editor.env);
}

fn apply_resize_move(editor: &mut Editor<impl Env>, state: &ResizeState, event: PointerEvent) {
    let pointer = [event.at[0] - state.offset[0], event.at[1] - state.offset[1]];
    let options = transform::ResizeOptions {
        keep_aspect_ratio: event.modifiers.shift,
        from_center: event.modifiers.alt,
    };
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    transform::resize_elements(
        &mut editor.geometry,
        file,
        &state.start,
        &state.targets,
        state.handle,
        pointer,
        options,
        &mut editor.env,
    );
}

fn apply_point_drag_move(
    editor: &mut Editor<impl Env>,
    state: &PointDragState,
    event: PointerEvent,
) {
    let target = [event.at[0] - state.offset[0], event.at[1] - state.offset[1]];
    let shift = event.modifiers.shift;
    let start_element = &state.start.elements[state.position];
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    edit::move_linear_point(
        start_element,
        &mut file.elements[state.position],
        state.index,
        target,
        shift,
        &mut editor.env,
    );
}

fn apply_box_select_move(
    editor: &mut Editor<impl Env>,
    state: &mut BoxSelectState,
    event: PointerEvent,
) {
    let rect = [state.origin[0], state.origin[1], event.at[0], event.at[1]];
    state.current = Some(normalize_bounds(rect));
    let mut result = selection::box_select(&mut editor.geometry, &editor.file, rect);
    if event.modifiers.shift {
        for id in state.selection_before.iter() {
            result.insert(id.to_owned());
        }
    }
    editor.selection = result;
}

fn normalize_bounds(rect: Bounds) -> Bounds {
    let [x1, y1, x2, y2] = rect;
    [x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]
}

/// No move happened at all: narrows or removes on Shift (`onPointerUpFromPointerDownHandler`),
/// then, when the click only grazed the selected element's padded bounding box rather than its
/// actual shape or a bound text, clears the selection entirely (`hitElementBoundingBoxOnly`).
fn finish_click(editor: &mut Editor<impl Env>, click: ClickState, event: PointerEvent) {
    let Some(pos) = click.hit else {
        if click.in_box {
            editor.selection = Selection::new();
        }
        return;
    };
    if !click.added_now {
        apply_click_narrow_or_remove(editor, pos, event.modifiers.shift);
    }
    if hit_bounding_box_only(editor, pos, click.origin, click.zoom) {
        editor.selection = Selection::new();
    }
}

fn apply_click_narrow_or_remove(editor: &mut Editor<impl Env>, pos: usize, shift: bool) {
    let Some(id) = editor.file.elements[pos].id().map(str::to_owned) else {
        return;
    };
    if !shift {
        editor.selection = selection::select_groups(&editor.file, &Selection::from_ids([id]));
        return;
    }
    let outermost = editor.file.elements[pos]
        .group_ids()
        .last()
        .map(|s| s.to_string());
    match outermost {
        Some(group) => {
            let to_remove: Vec<String> = editor
                .file
                .elements
                .iter()
                .filter(|e| e.group_ids().iter().any(|g| *g == group))
                .filter_map(Element::id)
                .map(str::to_owned)
                .collect();
            for rid in to_remove {
                editor.selection.remove(&rid);
            }
        }
        None => {
            editor.selection.remove(&id);
        }
    }
}

/// `hitElementBoundingBoxOnly`: `point` falls inside the element's unrotated bounds but neither
/// on its own outline nor inside its bound text.
fn hit_bounding_box_only(
    editor: &mut Editor<impl Env>,
    pos: usize,
    point: [f64; 2],
    zoom: f64,
) -> bool {
    let element = editor.file.elements[pos].clone();
    let threshold = collision::hit_threshold(&element, zoom);
    if collision::hit_element_itself(&mut editor.geometry, &element, point, threshold) {
        return false;
    }
    let index_by_id: HashMap<&str, usize> = editor
        .file
        .elements
        .iter()
        .enumerate()
        .filter_map(|(index, e)| e.id().map(|id| (id, index)))
        .collect();
    if selection::hit_element_bound_text(
        &mut editor.geometry,
        &editor.file.elements,
        &index_by_id,
        &element,
        point,
    ) {
        return false;
    }
    selection::hit_element_bounding_box(&mut editor.geometry, &element, point, 0.0)
}

/// Updates the idle cursor for `event`'s position: a resize cursor on a handle, `Pointer` on a
/// single selected line or non-elbow arrow's own point, `Move` on an element or inside the
/// multi-selection box, `Default` otherwise; a creation tool always shows `Crosshair`.
fn update_cursor(editor: &mut Editor<impl Env>, event: PointerEvent) {
    editor.cursor = compute_cursor(editor, event);
}

fn compute_cursor(editor: &mut Editor<impl Env>, event: PointerEvent) -> Cursor {
    if editor.tool != Tool::Selection {
        return if editor.tool == Tool::Hand {
            Cursor::Default
        } else {
            Cursor::Crosshair
        };
    }

    if let Some(handle) = transform::handle_at(
        &mut editor.geometry,
        &editor.file,
        &editor.selection,
        event.at,
        event.zoom,
    ) {
        return match handle {
            HandleKind::Nw | HandleKind::Se => Cursor::ResizeNwse,
            HandleKind::Ne | HandleKind::Sw => Cursor::ResizeNesw,
            HandleKind::N | HandleKind::S => Cursor::ResizeNs,
            HandleKind::E | HandleKind::W => Cursor::ResizeEw,
        };
    }

    if hit_point(editor, event.at, event.zoom).is_some() {
        return Cursor::Pointer;
    }

    let hit = selection::element_at(
        &mut editor.geometry,
        &editor.file,
        event.at,
        event.zoom,
        &editor.selection,
    );
    let in_box = selection::hits_selection_box(
        &mut editor.geometry,
        &editor.file,
        &editor.selection,
        event.at,
        event.zoom,
    );
    if hit.is_some() || in_box {
        Cursor::Move
    } else {
        Cursor::Default
    }
}
