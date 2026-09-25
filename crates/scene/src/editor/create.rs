//! Shape, line, arrow and freedraw creation, ported from `packages/excalidraw/components/
//! App.tsx`'s `createGenericElementOnPointerDown` (~10511), `handleLinearElementOnPointerDown`
//! (~10189 to 10482), `handleFreeDrawElementOnPointerDown` (~9966), the multi-point branch of
//! the pointer-move handler (~7934 to 8046), the freedraw and linear branches of the
//! pointer-move-while-dragging handler (~11394 and ~11425), `maybeDragNewGenericElement`
//! (~13521) and the freedraw, linear and generic-shape tails of the pointer-up handler (~11885
//! to 12160); `packages/element/src/dragElements.ts`'s `dragNewElement`;
//! `packages/element/src/sizeHelpers.ts`'s `getPerfectElementSize` and
//! `isInvisiblySmallElement`; and `packages/excalidraw/actions/actionFinalize.tsx`; all at
//! commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! A line or arrow goes through two gestures while it is being created. [`Gesture::Linear`]
//! covers the initial two-point drag: point 1 follows the pointer. Releasing it either
//! finishes the element (a real drag, past `MINIMUM_ARROW_SIZE`) or hands off to
//! [`Gesture::MultiPoint`], which covers clicking down further points one at a time; a line
//! closes into a polygon by clicking back near its start, and any of them finish on
//! `Command::Finalize`, `Command::Escape` or a tool switch. [`Gesture::MultiPoint`] tracks only
//! which point was last confirmed (`None` meaning just point 0, from a fresh two-point element
//! that was released without much of a drag); every point at or after that index but still in
//! the array is an uncommitted point following the pointer, dropped again on finish unless it
//! gets confirmed by a further click.

use std::sync::Arc;

use serde_json::{Map, json};

use crate::collision;
use crate::edit;
use crate::element::{Element, Roundness, StrokeOptions};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry;
use crate::json::Slot;
use crate::new_element::{self, ElementProps, GenericKind, bump_version};
use crate::selection::Selection;

use super::style::{ArrowType, EdgeStyle, ItemStyle};
use super::{Editor, PointerEvent, Tool, clone_scene};

/// `MINIMUM_ARROW_SIZE` (`packages/common/src/constants.ts`).
const MINIMUM_ARROW_SIZE: f64 = 20.0;

/// `INVISIBLY_SMALL_ELEMENT_SIZE` (`packages/element/src/sizeHelpers.ts`).
const INVISIBLY_SMALL_ELEMENT_SIZE: f64 = 0.1;

/// A creation tool's gesture in progress, if any.
pub(super) enum Gesture {
    None,
    /// A rectangle, diamond or ellipse being dragged from pointer-down to pointer-up.
    Shape(ShapeState),
    /// A line or arrow's first two points, before its first release.
    Linear(LinearState),
    /// A line or arrow between clicks, not currently being dragged.
    MultiPoint(MultiPointState),
    /// A freedraw stroke being dragged.
    Freedraw(FreedrawState),
}

pub(super) struct ShapeState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    origin: [f64; 2],
}

pub(super) struct LinearState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    /// Whether any `pointer_move` happened since the press, regardless of distance
    /// (`pointerDownState.drag.hasOccurred`, set unconditionally by a linear element's own
    /// move handler).
    moved: bool,
    /// The zoom of the latest event seen, for the release-distance and (via
    /// [`MultiPointState`]) commit-zone and loop checks a `Command` carries no zoom of its own.
    zoom: f64,
}

pub(super) struct MultiPointState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    /// The index of the last point the user clicked to confirm; `None` before the first such
    /// click, when only point 0 (the press that started the element) is confirmed. Every point
    /// from here to the end of the array, exclusive of this one, is an uncommitted point
    /// following the pointer.
    confirmed: Option<usize>,
    zoom: f64,
}

pub(super) struct FreedrawState {
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    /// The latest pointer position seen, for a tool switch to finish the stroke the way
    /// `pointer_up` there would.
    last: [f64; 2],
    zoom: f64,
}

pub(super) fn pointer_down(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if matches!(editor.tool, Tool::Selection | Tool::Hand) {
        return;
    }
    if matches!(editor.create_gesture, Gesture::MultiPoint(_)) {
        let Gesture::MultiPoint(state) =
            std::mem::replace(&mut editor.create_gesture, Gesture::None)
        else {
            unreachable!("just matched Gesture::MultiPoint above");
        };
        editor.create_gesture = multi_point_pointer_down(editor, state, event);
        return;
    }
    if !matches!(editor.create_gesture, Gesture::None) {
        // A second press before the first one's release: none of napkin's simulated event
        // sequences do this, so it is ignored rather than starting a second gesture.
        return;
    }
    match editor.tool {
        Tool::Rectangle | Tool::Diamond | Tool::Ellipse => start_shape(editor, event),
        Tool::Arrow | Tool::Line => start_linear(editor, event),
        Tool::Freedraw => start_freedraw(editor, event),
        Tool::Selection | Tool::Hand => {}
    }
}

pub(super) fn pointer_move(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if matches!(editor.tool, Tool::Selection | Tool::Hand) {
        return;
    }
    editor.create_gesture = match std::mem::replace(&mut editor.create_gesture, Gesture::None) {
        Gesture::None => Gesture::None,
        Gesture::Shape(state) => {
            apply_shape_move(editor, &state, event);
            Gesture::Shape(state)
        }
        Gesture::Linear(mut state) => {
            apply_linear_drag_move(editor, &state, event);
            state.moved = true;
            state.zoom = event.zoom;
            Gesture::Linear(state)
        }
        Gesture::MultiPoint(state) => multi_point_move(editor, state, event),
        Gesture::Freedraw(mut state) => {
            apply_freedraw_move(editor, &mut state, event);
            Gesture::Freedraw(state)
        }
    };
}

pub(super) fn pointer_up(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if matches!(editor.tool, Tool::Selection | Tool::Hand) {
        return;
    }
    if matches!(editor.create_gesture, Gesture::MultiPoint(_)) {
        let Gesture::MultiPoint(state) =
            std::mem::replace(&mut editor.create_gesture, Gesture::None)
        else {
            unreachable!("just matched Gesture::MultiPoint above");
        };
        editor.create_gesture = confirm_multi_point(editor, state, event);
        return;
    }
    finish_gesture(editor, Some(event));
}

/// Ends whatever creation gesture is in progress, the way a release at the latest position
/// would (`event` is `None` for a tool switch, which has no release of its own): a shape,
/// initial two-point drag or freedraw stroke finishes exactly as [`pointer_up`] would (their
/// geometry already reflects the last `pointer_move`, since neither reads the release event
/// itself in Excalidraw); a multi-point line or arrow finishes the way `Command::Finalize`
/// (Enter) would, dropping its uncommitted point.
pub(super) fn finish_gesture(editor: &mut Editor<impl Env>, event: Option<PointerEvent>) {
    match std::mem::replace(&mut editor.create_gesture, Gesture::None) {
        Gesture::None => {}
        Gesture::Shape(state) => finish_shape(editor, state),
        Gesture::Linear(state) => finish_linear_drag(editor, state),
        Gesture::MultiPoint(state) => finish_multi_point(editor, state),
        Gesture::Freedraw(state) => {
            let at = event.map_or(state.last, |e| e.at);
            finish_freedraw(editor, state, at);
        }
    }
}

/// `Command::Escape`: `None` when no creation gesture is active (Task 7's escape, clearing the
/// selection, applies instead). A shape, initial two-point drag or freedraw stroke being
/// actively dragged is discarded outright, leaving no history entry; a multi-point line or
/// arrow finishes the way `Command::Finalize` does. Always `Some(true)` otherwise: napkin has
/// no in-progress edit that an active creation gesture doesn't fully cover.
pub(super) fn escape(editor: &mut Editor<impl Env>) -> Option<bool> {
    match std::mem::replace(&mut editor.create_gesture, Gesture::None) {
        Gesture::None => None,
        Gesture::Shape(ShapeState {
            start,
            selection_before,
            position,
            ..
        })
        | Gesture::Linear(LinearState {
            start,
            selection_before,
            position,
            ..
        })
        | Gesture::Freedraw(FreedrawState {
            start,
            selection_before,
            position,
            ..
        }) => {
            discard(editor, &start, &selection_before, position);
            Some(true)
        }
        Gesture::MultiPoint(state) => {
            finish_multi_point(editor, state);
            Some(true)
        }
    }
}

/// `Command::Finalize` (Enter): only a multi-point line or arrow reacts, matching
/// `actionFinalize`'s own `keyTest`.
pub(super) fn finalize_command(editor: &mut Editor<impl Env>) -> bool {
    match std::mem::replace(&mut editor.create_gesture, Gesture::None) {
        Gesture::MultiPoint(state) => {
            finish_multi_point(editor, state);
            true
        }
        other => {
            editor.create_gesture = other;
            false
        }
    }
}

fn start_shape(editor: &mut Editor<impl Env>, event: PointerEvent) {
    let kind = match editor.tool {
        Tool::Rectangle => GenericKind::Rectangle,
        Tool::Diamond => GenericKind::Diamond,
        Tool::Ellipse => GenericKind::Ellipse,
        _ => return,
    };
    let start = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();
    let roundness = generic_roundness(&editor.style, kind);
    let stroke_width = editor.style.stroke_width.value(false);
    let props = item_props(&editor.style, event.at, 0.0, 0.0, roundness, stroke_width);
    let element = new_element::new_generic_element(kind, props, &mut editor.env);
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let position = edit::append_element(file, element, &mut editor.env);
    editor.create_gesture = Gesture::Shape(ShapeState {
        start,
        selection_before,
        position,
        origin: event.at,
    });
}

fn apply_shape_move(editor: &mut Editor<impl Env>, state: &ShapeState, event: PointerEvent) {
    let (x, y, width, height) = drag_new_shape_geometry(
        state.origin,
        event.at,
        event.modifiers.shift,
        event.modifiers.alt,
    );
    if width == 0.0 || height == 0.0 {
        return;
    }
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let element = &mut file.elements[state.position];
    let before = element.clone();
    if let Some(base) = element.base_mut() {
        base.x = x;
        base.y = y;
        base.width = width;
        base.height = height;
    }
    if *element != before {
        bump_version(element, &mut editor.env);
    }
}

/// The generic-shape completion (`isInvisiblySmallElement` branch of
/// `onPointerUpFromPointerDownHandler`): an invisible (0x0) shape is removed and the tool stays
/// put, matching that branch's early return; a real one is selected and the tool reverts to
/// `Selection`.
fn finish_shape(editor: &mut Editor<impl Env>, state: ShapeState) {
    let placement = editor.file.elements[state.position]
        .placement()
        .expect("a freshly created shape is always typed");
    let invisible = placement.width == 0.0 && placement.height == 0.0;
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    if invisible {
        file.elements.remove(state.position);
    } else {
        if let Some(id) = file.elements[state.position].id().map(str::to_owned) {
            editor.selection = Selection::from_ids([id]);
        }
        editor.tool = Tool::Selection;
    }
    editor.finish_edit(&state.start, &state.selection_before);
}

fn start_linear(editor: &mut Editor<impl Env>, event: PointerEvent) {
    let start = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();
    let stroke_width = editor.style.stroke_width.value(false);
    let element = if editor.tool == Tool::Line {
        let roundness = (editor.style.edges == EdgeStyle::Round).then(round_proportional);
        let props = item_props(&editor.style, event.at, 0.0, 0.0, roundness, stroke_width);
        new_element::new_line_element(props, vec![[0.0, 0.0], [0.0, 0.0]], &mut editor.env)
    } else {
        let roundness = (editor.style.arrow_type == ArrowType::Round).then(round_proportional);
        let props = item_props(&editor.style, event.at, 0.0, 0.0, roundness, stroke_width);
        new_element::new_arrow_element(
            props,
            vec![[0.0, 0.0], [0.0, 0.0]],
            editor.style.start_arrowhead.clone(),
            editor.style.end_arrowhead.clone(),
            &mut editor.env,
        )
    };
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let position = edit::append_element(file, element, &mut editor.env);
    editor.create_gesture = Gesture::Linear(LinearState {
        start,
        selection_before,
        position,
        moved: false,
        zoom: event.zoom,
    });
}

/// Moves point 1 (the only uncommitted point of a fresh two-point line or arrow) to the
/// pointer, Shift locking its angle around point 0 (`edit::move_linear_point`'s own pivot
/// choice for a non-zero index).
fn apply_linear_drag_move(editor: &mut Editor<impl Env>, state: &LinearState, event: PointerEvent) {
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let before = file.elements[state.position].clone();
    edit::move_linear_point(
        &before,
        &mut file.elements[state.position],
        1,
        event.at,
        event.modifiers.shift,
        &mut editor.env,
    );
}

/// Point 1's current scene-relative position (point 0 is always the press origin), for the
/// release-distance check below.
fn linear_points(element: &Element) -> Vec<[f64; 2]> {
    match element {
        Element::Line(l) | Element::Arrow(l) => l.points.clone(),
        Element::Freedraw(f) => f.points.clone(),
        _ => Vec::new(),
    }
}

/// Releasing a fresh two-point line or arrow: too little movement (no move at all, or the
/// release less than `MINIMUM_ARROW_SIZE` screen pixels from the press) hands off to
/// [`Gesture::MultiPoint`] instead of finishing.
fn finish_linear_drag(editor: &mut Editor<impl Env>, state: LinearState) {
    let points = linear_points(&editor.file.elements[state.position]);
    let follow = points[1];
    let distance = rough::js::hypot(follow[0], follow[1]) * state.zoom;
    if !state.moved || distance < MINIMUM_ARROW_SIZE {
        editor.create_gesture = Gesture::MultiPoint(MultiPointState {
            start: state.start,
            selection_before: state.selection_before,
            position: state.position,
            confirmed: None,
            zoom: state.zoom,
        });
        return;
    }
    finalize_linear_or_freedraw(
        editor,
        state.start,
        state.selection_before,
        state.position,
        state.zoom,
        false,
    );
}

fn multi_point_pointer_down(
    editor: &mut Editor<impl Env>,
    mut state: MultiPointState,
    event: PointerEvent,
) -> Gesture {
    state.zoom = event.zoom;
    let element = &editor.file.elements[state.position];
    let points = linear_points(element);
    let is_line = matches!(element, Element::Line(_));
    let placement = element
        .placement()
        .expect("a line or arrow is always typed");
    let local = [event.at[0] - placement.x, event.at[1] - placement.y];

    if is_line && collision::is_path_a_loop(&points, state.zoom) {
        // Confirm the point about to close the loop first, so the finish below (which drops
        // any point past the last confirmed one) keeps it instead.
        state.confirmed = Some(points.len() - 1);
        finish_multi_point(editor, state);
        return Gesture::None;
    }

    let in_commit_zone = state.confirmed.is_some_and(|confirmed| {
        rough::js::hypot(
            local[0] - points[confirmed][0],
            local[1] - points[confirmed][1],
        ) < collision::LINE_CONFIRM_THRESHOLD
    });
    if in_commit_zone {
        finish_multi_point(editor, state);
        return Gesture::None;
    }

    Gesture::MultiPoint(state)
}

fn multi_point_move(
    editor: &mut Editor<impl Env>,
    mut state: MultiPointState,
    event: PointerEvent,
) -> Gesture {
    state.zoom = event.zoom;
    let position = state.position;
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let placement = file.elements[position]
        .placement()
        .expect("a line or arrow is always typed");
    let local = [event.at[0] - placement.x, event.at[1] - placement.y];
    let points = linear_points(&file.elements[position]);
    let confirmed_count = state.confirmed.map_or(1, |i| i + 1);
    let last_confirmed = points[confirmed_count - 1];
    let near_confirmed =
        rough::js::hypot(local[0] - last_confirmed[0], local[1] - last_confirmed[1])
            < collision::LINE_CONFIRM_THRESHOLD;

    if points.len() == confirmed_count {
        if !near_confirmed {
            push_linear_point(&mut file.elements[position], local, &mut editor.env);
        }
    } else if points.len() > 2 && near_confirmed {
        pop_linear_point(&mut file.elements[position], &mut editor.env);
    } else {
        let before = file.elements[position].clone();
        let last_index = points.len() - 1;
        edit::move_linear_point(
            &before,
            &mut file.elements[position],
            last_index,
            event.at,
            event.modifiers.shift,
            &mut editor.env,
        );
    }
    Gesture::MultiPoint(state)
}

fn confirm_multi_point(
    editor: &mut Editor<impl Env>,
    mut state: MultiPointState,
    event: PointerEvent,
) -> Gesture {
    state.zoom = event.zoom;
    let points_len = linear_points(&editor.file.elements[state.position]).len();
    state.confirmed = Some(points_len - 1);
    Gesture::MultiPoint(state)
}

/// Drops every point past the last confirmed one, then hands off to the completion shared with
/// a direct two-point drag and a freedraw stroke.
fn finish_multi_point(editor: &mut Editor<impl Env>, state: MultiPointState) {
    let position = state.position;
    {
        let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
        let before = file.elements[position].clone();
        let keep = state.confirmed.map_or(1, |i| i + 1);
        if let Element::Line(l) | Element::Arrow(l) = &mut file.elements[position] {
            l.points.truncate(keep);
            let [width, height] = geometry::size_from_points(&l.points);
            l.base.width = width;
            l.base.height = height;
        }
        if file.elements[position] != before {
            bump_version(&mut file.elements[position], &mut editor.env);
        }
    }
    finalize_linear_or_freedraw(
        editor,
        state.start,
        state.selection_before,
        position,
        state.zoom,
        false,
    );
}

fn start_freedraw(editor: &mut Editor<impl Env>, event: PointerEvent) {
    let start = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();
    let stroke_width = editor.style.stroke_width.value(true);
    let props = item_props(&editor.style, event.at, 0.0, 0.0, None, stroke_width);
    let stroke_options = StrokeOptions {
        variability: Slot::Value(editor.style.stroke_variability.clone()),
        streamline: Slot::Value(new_element::DEFAULT_STROKE_STREAMLINE),
        extra: Map::new(),
    };
    let element = new_element::new_freedraw_element(
        props,
        vec![[0.0, 0.0]],
        Vec::new(),
        true,
        Some(stroke_options),
        &mut editor.env,
    );
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let position = edit::append_element(file, element, &mut editor.env);
    editor.create_gesture = Gesture::Freedraw(FreedrawState {
        start,
        selection_before,
        position,
        last: event.at,
        zoom: event.zoom,
    });
}

fn apply_freedraw_move(
    editor: &mut Editor<impl Env>,
    state: &mut FreedrawState,
    event: PointerEvent,
) {
    state.last = event.at;
    state.zoom = event.zoom;
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let position = state.position;
    let before = file.elements[position].clone();
    if let Element::Freedraw(f) = &mut file.elements[position] {
        let point = [event.at[0] - f.base.x, event.at[1] - f.base.y];
        let discard = f.points.last() == Some(&point);
        if !discard {
            f.points.push(point);
            let [width, height] = geometry::size_from_points(&f.points);
            f.base.width = width;
            f.base.height = height;
        }
    }
    if file.elements[position] != before {
        bump_version(&mut file.elements[position], &mut editor.env);
    }
}

/// Appends the release position as one final point (`at` nudged by `0.0001` on both axes when
/// it would otherwise land exactly on point 0, so a plain click still leaves a visible dot),
/// then hands off to the completion shared with a finished line or arrow.
fn finish_freedraw(editor: &mut Editor<impl Env>, state: FreedrawState, at: [f64; 2]) {
    let position = state.position;
    {
        let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
        let before = file.elements[position].clone();
        if let Element::Freedraw(f) = &mut file.elements[position] {
            let mut point = [at[0] - f.base.x, at[1] - f.base.y];
            if f.points.first() == Some(&point) {
                point[0] += 0.0001;
                point[1] += 0.0001;
            }
            f.points.push(point);
            let [width, height] = geometry::size_from_points(&f.points);
            f.base.width = width;
            f.base.height = height;
        }
        if file.elements[position] != before {
            bump_version(&mut file.elements[position], &mut editor.env);
        }
    }
    finalize_linear_or_freedraw(
        editor,
        state.start,
        state.selection_before,
        position,
        state.zoom,
        true,
    );
}

/// The shared tail of every line, arrow or freedraw completion (`actionFinalize`): an
/// invisible result (`isInvisiblySmallElement`) is removed; otherwise a line or freedraw that
/// closes into a loop gets its last point snapped onto its first, and a line's `polygon` flag
/// set to whether that leaves it with more than 3 points. The tool always reverts to
/// `Selection` and the element gets selected, unless `keep_tool` (freedraw), which does
/// neither — even when the element itself was removed as invisible, since a linear or freedraw
/// gesture always reaches this same return in `actionFinalize` regardless of visibility (unlike
/// a generic shape, which returns before ever reaching its own tool-revert code).
fn finalize_linear_or_freedraw(
    editor: &mut Editor<impl Env>,
    start: Arc<SceneFile>,
    selection_before: Selection,
    position: usize,
    zoom: f64,
    keep_tool: bool,
) {
    let invisible = {
        let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
        let before = file.elements[position].clone();
        let points = linear_points(&file.elements[position]);
        let is_arrow = matches!(file.elements[position], Element::Arrow(_));
        let invisible = points.len() < 2
            || (points.len() == 2
                && is_arrow
                && points_close(points[0], points[1], INVISIBLY_SMALL_ELEMENT_SIZE));

        if !invisible {
            let is_line_or_freedraw = matches!(
                file.elements[position],
                Element::Line(_) | Element::Freedraw(_)
            );
            if is_line_or_freedraw && collision::is_path_a_loop(&points, zoom) {
                set_last_point(&mut file.elements[position], points[0]);
            }
            if let Element::Line(l) = &mut file.elements[position] {
                let is_polygon = l.points.len() > 3 && l.points.first() == l.points.last();
                l.extra.insert("polygon".into(), json!(is_polygon));
            }
        }

        if file.elements[position] != before {
            bump_version(&mut file.elements[position], &mut editor.env);
        }
        invisible
    };

    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    if invisible {
        file.elements.remove(position);
    } else if !keep_tool && let Some(id) = file.elements[position].id().map(str::to_owned) {
        editor.selection = Selection::from_ids([id]);
    }
    if !keep_tool {
        editor.tool = Tool::Selection;
    }
    editor.finish_edit(&start, &selection_before);
}

/// Removes the element a shape, initial linear drag or freedraw stroke had added, leaving the
/// scene exactly as it was before the gesture started (so `finish_edit` records nothing).
fn discard(
    editor: &mut Editor<impl Env>,
    start: &Arc<SceneFile>,
    selection_before: &Selection,
    position: usize,
) {
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    file.elements.remove(position);
    editor.finish_edit(start, selection_before);
}

fn push_linear_point(element: &mut Element, point: [f64; 2], env: &mut impl Env) {
    let before = element.clone();
    if let Element::Line(l) | Element::Arrow(l) = element {
        l.points.push(point);
        let [width, height] = geometry::size_from_points(&l.points);
        l.base.width = width;
        l.base.height = height;
    }
    if *element != before {
        bump_version(element, env);
    }
}

fn pop_linear_point(element: &mut Element, env: &mut impl Env) {
    let before = element.clone();
    if let Element::Line(l) | Element::Arrow(l) = element {
        l.points.pop();
        let [width, height] = geometry::size_from_points(&l.points);
        l.base.width = width;
        l.base.height = height;
    }
    if *element != before {
        bump_version(element, env);
    }
}

fn set_last_point(element: &mut Element, point: [f64; 2]) {
    match element {
        Element::Line(l) | Element::Arrow(l) => {
            if let Some(last) = l.points.last_mut() {
                *last = point;
            }
            let [width, height] = geometry::size_from_points(&l.points);
            l.base.width = width;
            l.base.height = height;
        }
        Element::Freedraw(f) => {
            if let Some(last) = f.points.last_mut() {
                *last = point;
            }
            let [width, height] = geometry::size_from_points(&f.points);
            f.base.width = width;
            f.base.height = height;
        }
        _ => {}
    }
}

/// `pointsEqual` at `tolerance`: both axes strictly closer than it, not a Euclidean distance.
fn points_close(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    (a[0] - b[0]).abs() < tolerance && (a[1] - b[1]).abs() < tolerance
}

/// `Math.sign`: `0` (or `-0`) stays itself, `NaN` propagates, otherwise `±1`.
fn js_sign(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `dragNewElement` plus the generic-element branch of `getPerfectElementSize` (a rectangle,
/// diamond or ellipse never takes the line/arrow/freedraw branch, since those are never
/// dragged through this function): `origin` to `pointer` as `(x, y, width, height)`, Shift
/// (`shouldMaintainAspectRatio`) squaring it and Alt (`shouldResizeFromCenter`) growing it from
/// its center.
fn drag_new_shape_geometry(
    origin: [f64; 2],
    pointer: [f64; 2],
    shift: bool,
    alt: bool,
) -> (f64, f64, f64, f64) {
    let [origin_x, origin_y] = origin;
    let [x, y] = pointer;
    let mut width = (x - origin_x).abs();
    let mut height = (y - origin_y).abs();

    if shift {
        if (y - origin_y).abs() > (x - origin_x).abs() {
            let signed_width = if x < origin_x { -width } else { width };
            let new_width = height;
            height *= js_sign(signed_width);
            width = new_width;
        } else {
            let signed_height = if y < origin_y { -height } else { height };
            height = width * js_sign(signed_height);
        }
        if height < 0.0 {
            height = -height;
        }
    }

    let mut new_x = if x < origin_x {
        origin_x - width
    } else {
        origin_x
    };
    let mut new_y = if y < origin_y {
        origin_y - height
    } else {
        origin_y
    };

    if alt {
        width += width;
        height += height;
        new_x = origin_x - width / 2.0;
        new_y = origin_y - height / 2.0;
    }

    (new_x, new_y, width, height)
}

/// `{type: ROUNDNESS.PROPORTIONAL_RADIUS}`, a diamond, ellipse, line or round-arrow's roundness.
fn round_proportional() -> Roundness {
    Roundness {
        kind: 2.0,
        value: Slot::Missing,
        extra: Map::new(),
    }
}

/// `getCurrentItemRoundness`: a rectangle uses the adaptive radius (`{type: 3}`), a diamond or
/// ellipse the proportional one (`{type: 2}`); `None` when `style.edges` is sharp.
fn generic_roundness(style: &ItemStyle, kind: GenericKind) -> Option<Roundness> {
    if style.edges != EdgeStyle::Round {
        return None;
    }
    let radius_kind = if kind == GenericKind::Rectangle {
        3.0
    } else {
        2.0
    };
    Some(Roundness {
        kind: radius_kind,
        value: Slot::Missing,
        extra: Map::new(),
    })
}

fn item_props(
    style: &ItemStyle,
    origin: [f64; 2],
    width: f64,
    height: f64,
    roundness: Option<Roundness>,
    stroke_width: f64,
) -> ElementProps {
    ElementProps {
        x: origin[0],
        y: origin[1],
        width,
        height,
        angle: 0.0,
        stroke_color: style.stroke_color.clone(),
        background_color: style.background_color.clone(),
        fill_style: style.fill_style.clone(),
        stroke_width,
        stroke_style: style.stroke_style.clone(),
        roughness: style.roughness,
        opacity: style.opacity,
        group_ids: Vec::new(),
        roundness,
        locked: false,
    }
}
