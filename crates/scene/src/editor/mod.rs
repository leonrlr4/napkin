//! The stateful editor: pointer gestures and commands over a scene, ported from
//! `packages/excalidraw/components/App.tsx`'s `handleCanvasPointerDown`,
//! `handleSelectionOnPointerDown`, `onPointerMoveFromPointerDownHandler`,
//! `onPointerUpFromPointerDownHandler`, `maybeHandleResize`, `clearSelection` and
//! `clearSelectionIfNotUsingSelection`; `packages/element/src/linearElementEditor.ts`'s
//! `getPointIndexUnderCursor` and `handlePointerMove`; `packages/excalidraw/renderer/
//! interactiveScene.ts`'s `renderSelectionBorder` and its transform-handle painting;
//! `packages/excalidraw/actions/actionDeleteSelected.tsx`, `actionSelectAll.ts` and
//! `actionHistory.tsx`; and, for shape, line, arrow and freedraw creation, the sources listed
//! in `create`'s own doc comment; all at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! The selection tool, the creation tools and the eraser each keep their own gesture
//! (`select_gesture`, `create_gesture`, `erase_gesture`); at most one is active at a time, since
//! [`Editor::set_tool`] finishes all three before switching. `Command::Escape` clears the
//! selection while idle, discards or finishes an active creation gesture (see `create::escape`),
//! and abandons an in-progress eraser stroke (see `erase::escape`). A text edit in progress
//! never reaches this command at all: the overlay's own `TextEdit` widget already treats Escape
//! as a loss of focus and commits it first (see the `app` crate's `text_edit` module).

mod create;
mod erase;
mod properties;
mod select;
mod style;
mod text;

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

use crate::batch::{self, BatchReport, OpError};
use crate::bound_text;
use crate::clipboard::{self, Pasted};
use crate::duplicate::{self, DEFAULT_GRID_SIZE, DuplicateMode};
use crate::edit;
use crate::element::Element;
use crate::env::Env;
use crate::file::SceneFile;
use crate::fractional_index;
use crate::geometry::{Bounds, GeometryCache, rotate_point};
use crate::history::History;
use crate::new_element::{self, ElementProps, TextProps};
use crate::selection::{self, Selection};
use crate::text::TextMeasure;
use crate::transform;
use crate::zindex;

pub use properties::{PanelState, Property, Section};
pub use style::{ArrowType, EdgeStyle, ItemStyle, StrokeWidth};
pub use text::TextEditing;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Selection,
    Hand,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Freedraw,
    /// Starts text editing on pointer-down (`text::pointer_down`); `create::pointer_down` and
    /// friends ignore it like [`Tool::Selection`]/[`Tool::Hand`], but [`Editor::set_tool`]
    /// still clears the selection for it, as for any other creation tool.
    Text,
    /// Drags out and deletes elements one pointer gesture at a time; see `erase`.
    Eraser,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// Scene coordinates.
    pub at: [f64; 2],
    pub modifiers: Modifiers,
    /// The camera zoom when the event happened; CSS-pixel thresholds are divided by it.
    pub zoom: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Delete or Backspace.
    Delete,
    SelectAll,
    Undo,
    Redo,
    /// Spec §7.5: finishes or discards the shape being drawn, else clears the selection.
    Escape,
    /// Enter: finishes a multi-point line or arrow.
    Finalize,
    /// `Ctrl+[`: `moveOneLeft`.
    SendBackward,
    /// `Ctrl+]`: `moveOneRight`.
    BringForward,
    /// `Ctrl+D`: `actionDuplicateSelection`.
    Duplicate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    Move,
    Crosshair,
    Pointer,
    ResizeNwse,
    ResizeNesw,
    ResizeNs,
    ResizeEw,
}

/// What the app should draw on top of the scene for the current selection and gesture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    /// One solid outline per selected element: the corners of its absolute-coords box padded
    /// by `4 / zoom`, rotated with the element (`renderSelectionBorder`). Empty when the
    /// selection is a single two-point line or arrow.
    pub outlines: Vec<[[f64; 2]; 4]>,
    /// Two or more selected elements: their common bounds padded by `4 / zoom`, drawn dashed.
    pub selection_box: Option<Bounds>,
    /// Corner handle squares (`transform::selection_handles`).
    pub handles: Vec<Bounds>,
    /// A single selected line or non-elbow arrow: its points in scene coordinates.
    pub points: Vec<[f64; 2]>,
    /// The rubber band while box selecting, normalized.
    pub box_selection: Option<Bounds>,
}

pub struct Editor<E: Env> {
    file: Arc<SceneFile>,
    env: E,
    tool: Tool,
    selection: Selection,
    select_gesture: select::Gesture,
    create_gesture: create::Gesture,
    erase_gesture: erase::Gesture,
    text_editing: Option<TextEditing>,
    /// The scene from before a new label's container grew to its minimum size at the start of
    /// the edit; `None` when it did not grow.
    text_edit_base: Option<Arc<SceneFile>>,
    style: ItemStyle,
    history: History,
    geometry: GeometryCache,
    revision: u64,
    scene_clones: u64,
    cursor: Cursor,
}

impl<E: Env> Editor<E> {
    /// Applies [`edit::repair_on_load`]. `revision` starts at 0.
    pub fn new(mut file: SceneFile, mut env: E) -> Editor<E> {
        edit::repair_on_load(&mut file, &mut env);
        Editor {
            file: Arc::new(file),
            env,
            tool: Tool::default(),
            selection: Selection::new(),
            select_gesture: select::Gesture::None,
            create_gesture: create::Gesture::None,
            erase_gesture: erase::Gesture::None,
            text_editing: None,
            text_edit_base: None,
            style: ItemStyle::default(),
            history: History::default(),
            geometry: GeometryCache::default(),
            revision: 0,
            scene_clones: 0,
            cursor: Cursor::default(),
        }
    }

    pub fn file(&self) -> &Arc<SceneFile> {
        &self.file
    }

    /// Replaces the scene after an external change: repairs it, clears the selection, the
    /// history, any gesture and any text edit in progress. Does not change [`Editor::revision`].
    pub fn replace_file(&mut self, mut file: SceneFile) {
        edit::repair_on_load(&mut file, &mut self.env);
        self.file = Arc::new(file);
        self.selection = Selection::new();
        self.history.clear();
        self.select_gesture = select::Gesture::None;
        self.create_gesture = create::Gesture::None;
        self.erase_gesture = erase::Gesture::None;
        self.text_editing = None;
        self.text_edit_base = None;
        self.geometry.clear();
    }

    /// Increases every time elements change, including undo and redo.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many edits had to copy the whole scene because another `Arc` shared it.
    pub fn scene_clones(&self) -> u64 {
        self.scene_clones
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Any tool other than [`Tool::Selection`] and [`Tool::Hand`] clears the selection
    /// (`clearSelectionIfNotUsingSelection`). First ends whatever gesture is in progress the
    /// way releasing the pointer at its last position would: a selection-tool drag, resize or
    /// point-drag records one history step, a box selection just stops (its last result is
    /// already the current selection); a shape, initial linear drag or freedraw stroke
    /// finishes exactly as `pointer_up` would, and a multi-point line or arrow finishes the
    /// way `Command::Finalize` (Enter) would (`create::finish_gesture`'s own doc comment).
    /// Without this, switching tools mid-gesture would leave the mutation unrecorded and the
    /// editor permanently "not idle", since `pointer_move` and `pointer_up` both do nothing
    /// once the tool no longer matches the gesture in progress.
    ///
    /// This deliberately never touches `text_editing`: an in-progress text edit survives a tool
    /// switch (it ends only through `Editor::commit_text`), so `create`/`erase`'s own
    /// `pointer_down` still check for it directly rather than relying on the tool alone.
    pub fn set_tool(&mut self, tool: Tool) {
        select::finish_gesture(self, None);
        create::finish_gesture(self, None);
        erase::finish_gesture(self, None);
        if !matches!(tool, Tool::Selection | Tool::Hand) {
            self.selection = Selection::new();
            self.cursor = Cursor::Crosshair;
        }
        self.tool = tool;
    }

    /// Finishes any in-progress pointer gesture or multi-point line without changing the
    /// tool, the same code path [`Editor::set_tool`] runs before switching: a selection-tool
    /// drag, resize or point-drag records one history step; a shape, initial linear drag or
    /// freedraw stroke finishes as a release at its last position would; a multi-point line
    /// or arrow finishes the way `Command::Finalize` (Enter) would, dropping its uncommitted
    /// cursor-following point. A no-op when [`Editor::is_idle`].
    ///
    /// A multi-point line's follow point already sits in `file` (each `pointer_move` writes
    /// it so the overlay tracks the cursor) but is not reflected in `revision` until the
    /// gesture finishes, so closing the window or losing focus mid-gesture without calling
    /// this first would save that uncommitted point as if it were confirmed, or lose it
    /// entirely if nothing else changed to bump the revision.
    pub fn finish_pending_gesture(&mut self) {
        select::finish_gesture(self, None);
        create::finish_gesture(self, None);
        erase::finish_gesture(self, None);
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The style values (`currentItem*`) a newly created element takes.
    pub fn style(&self) -> &ItemStyle {
        &self.style
    }

    /// What the property panel shows for the current selection and tool.
    pub fn panel(&self) -> PanelState {
        properties::panel(self)
    }

    /// Applies `property` to the current selection (and to `style()`), as one history step.
    /// Returns whether any element changed.
    pub fn set_property(&mut self, property: Property, measure: &mut dyn TextMeasure) -> bool {
        properties::set_property(self, property, measure)
    }

    /// No pointer gesture, no multi-point line, and no text edit in progress.
    pub fn is_idle(&self) -> bool {
        matches!(self.select_gesture, select::Gesture::None)
            && matches!(self.create_gesture, create::Gesture::None)
            && matches!(self.erase_gesture, erase::Gesture::None)
            && self.text_editing.is_none()
    }

    /// The text element currently being edited, if any (`state.editingTextElement`).
    pub fn text_editing(&self) -> Option<&TextEditing> {
        self.text_editing.as_ref()
    }

    /// Ids of the elements the current eraser stroke will delete, drawn faded
    /// (`ELEMENT_READY_TO_ERASE_OPACITY`) while it is in progress. Empty outside an eraser
    /// stroke.
    pub fn pending_erasure(&self) -> &HashSet<String> {
        erase::pending(&self.erase_gesture)
    }

    /// `measure` is only consulted when the text tool starts a label on a container.
    pub fn pointer_down(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure) {
        select::pointer_down(self, event);
        create::pointer_down(self, event);
        erase::pointer_down(self, event);
        text::pointer_down(self, event, measure);
    }

    /// `measure` is only consulted when the move resizes a container's label or a standalone
    /// text.
    pub fn pointer_move(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure) {
        select::pointer_move(self, event, measure);
        create::pointer_move(self, event);
        erase::pointer_move(self, event);
    }

    pub fn pointer_up(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure) {
        select::pointer_up(self, event, measure);
        create::pointer_up(self, event);
        erase::pointer_up(self, event);
    }

    /// Whether the command did anything. Commands are ignored while a pointer gesture is in
    /// progress.
    pub fn command(&mut self, command: Command) -> bool {
        match command {
            Command::Delete => {
                if !self.is_idle() || self.selection.is_empty() {
                    return false;
                }
                let before = Arc::clone(&self.file);
                let selection_before = self.selection.clone();
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                let next = edit::delete_selection(file, &selection_before, &mut self.env);
                self.selection = next;
                self.finish_edit(&before, &selection_before)
            }
            Command::SelectAll => {
                if !self.is_idle() {
                    return false;
                }
                let next = selection::select_all(&self.file);
                let changed = next != self.selection;
                self.selection = next;
                changed
            }
            Command::Undo => {
                if !self.is_idle() || !self.history.can_undo() {
                    return false;
                }
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                let selection = self.history.undo(file).expect("can_undo checked above");
                self.selection = selection;
                self.revision += 1;
                self.prune_selection();
                true
            }
            Command::Redo => {
                if !self.is_idle() || !self.history.can_redo() {
                    return false;
                }
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                let selection = self.history.redo(file).expect("can_redo checked above");
                self.selection = selection;
                self.revision += 1;
                self.prune_selection();
                true
            }
            Command::Escape => {
                if let Some(handled) = create::escape(self) {
                    return handled;
                }
                if let Some(handled) = erase::escape(self) {
                    return handled;
                }
                if !self.is_idle() || self.selection.is_empty() {
                    return false;
                }
                self.selection = Selection::new();
                true
            }
            Command::Finalize => create::finalize_command(self),
            Command::SendBackward | Command::BringForward => {
                if !self.is_idle() || self.selection.is_empty() {
                    return false;
                }
                let direction = if command == Command::BringForward {
                    zindex::Direction::Right
                } else {
                    zindex::Direction::Left
                };
                let Some(next_elements) = zindex::move_one(
                    &self.file.elements,
                    &self.selection,
                    direction,
                    &mut self.env,
                ) else {
                    return false;
                };
                let before = Arc::clone(&self.file);
                let selection_before = self.selection.clone();
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                file.elements = next_elements;
                self.finish_edit(&before, &selection_before)
            }
            Command::Duplicate => {
                if !self.is_idle() || self.selection.is_empty() {
                    return false;
                }
                let before = Arc::clone(&self.file);
                let selection_before = self.selection.clone();
                let offset = [DEFAULT_GRID_SIZE / 2.0, DEFAULT_GRID_SIZE / 2.0];
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                let duplicated = duplicate::duplicate_elements(
                    &file.elements,
                    &selection_before,
                    DuplicateMode::InPlace { offset },
                    &mut self.env,
                );
                file.elements = duplicated.elements;
                let moved: HashSet<String> = duplicated.new_ids.iter().cloned().collect();
                fractional_index::sync_moved_indices(&mut file.elements, &moved, &mut self.env);
                self.selection = Selection::from_ids(duplicated.new_ids);
                self.finish_edit(&before, &selection_before)
            }
        }
    }

    /// Applies one batch as one history step (AI spec §4.1), keeping the selection (minus
    /// anything the batch deleted). Refuses while a gesture is in progress: the caller waits
    /// for `is_idle`. Bumps `revision` on success.
    ///
    /// Unspecified fields fall back to [`ItemStyle::default`], not [`Editor::style`]: an AI
    /// batch is deterministic the same way `convertToExcalidrawElements` is, so a rectangle it
    /// adds without a `strokeColor` always gets `#1e1e1e`, whatever the user last picked in the
    /// property panel.
    pub fn apply_batch(
        &mut self,
        batch: &Value,
        measure: &mut dyn TextMeasure,
    ) -> Result<BatchReport, Vec<OpError>> {
        if !self.is_idle() {
            return Err(vec![OpError {
                op: None,
                field: None,
                message: "napkin is in the middle of a drawing gesture; try again".into(),
            }]);
        }
        let (next, report) = batch::apply_batch(
            &self.file,
            batch,
            &ItemStyle::default(),
            measure,
            &mut self.env,
        )?;
        let before = std::mem::replace(&mut self.file, Arc::new(next));
        let selection = self.selection.clone();
        self.finish_edit(&before, &selection);
        Ok(report)
    }

    /// The clipboard JSON for the selection (`actionCopy`); `None` when nothing is selected.
    pub fn copy_selection(&self) -> Option<String> {
        clipboard::serialize(&self.file, &self.selection)
    }

    /// Pastes clipboard `text` at `at`, as one history step, and selects what was pasted.
    /// Excalidraw data (`addElementsFromPasteOrLibrary`) is repaired the way loading a file
    /// repairs it (`edit::repair_on_load`'s duplicate-id and index fixes), its deleted elements
    /// dropped, and the rest centered on `at`; anything else becomes one new text element
    /// (`addTextFromPaste`), also centered on `at`. Returns whether anything was pasted. Refused
    /// (returning `false`) while a gesture, a multi-point line or a text edit is in progress
    /// (`Editor::is_idle`): applying it mid-drag would either merge into the drag's own eventual
    /// history entry or interrupt it losing the selection, and applying it mid-edit would insert
    /// elements the edit's own commit does not expect to find.
    pub fn paste(&mut self, text: &str, at: [f64; 2], measure: &mut dyn TextMeasure) -> bool {
        if !self.is_idle() {
            return false;
        }
        let Some(parsed) = clipboard::parse(text) else {
            return false;
        };
        let before = Arc::clone(&self.file);
        let selection_before = self.selection.clone();

        let new_ids = match parsed {
            Pasted::Elements(raw_elements) => {
                let mut temp = SceneFile::new();
                temp.elements = raw_elements;
                edit::repair_on_load(&mut temp, &mut self.env);
                let live: Vec<Element> = temp
                    .elements
                    .into_iter()
                    .filter(|e| !e.is_deleted())
                    .collect();
                if live.is_empty() {
                    return false;
                }
                let shift = self
                    .geometry
                    .common_bounds(live.iter())
                    .map(|[x1, y1, x2, y2]| [at[0] - (x1 + x2) / 2.0, at[1] - (y1 + y2) / 2.0])
                    .unwrap_or([0.0, 0.0]);
                let shifted: Vec<Element> = live
                    .into_iter()
                    .map(|mut element| {
                        if let Some(p) = element.placement() {
                            element.set_position(p.x + shift[0], p.y + shift[1]);
                        }
                        element
                    })
                    .collect();
                let duplicated = duplicate::duplicate_elements(
                    &shifted,
                    &Selection::new(),
                    DuplicateMode::Everything,
                    &mut self.env,
                );
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                file.elements.extend(duplicated.elements);
                let moved: HashSet<String> = duplicated.new_ids.iter().cloned().collect();
                fractional_index::sync_moved_indices(&mut file.elements, &moved, &mut self.env);
                // `addElementsFromPasteOrLibrary`: a pasted label is rewrapped with this
                // machine's measurer, since the copied line breaks came from another one.
                for id in &duplicated.new_ids {
                    let Some(text) = file
                        .elements
                        .iter()
                        .position(|e| e.id() == Some(id.as_str()))
                    else {
                        continue;
                    };
                    let Element::Text(t) = &file.elements[text] else {
                        continue;
                    };
                    let Some(container_id) = t.container_id.value().cloned() else {
                        continue;
                    };
                    let container = file
                        .elements
                        .iter()
                        .position(|e| !e.is_deleted() && e.id() == Some(container_id.as_str()));
                    bound_text::redraw_text_bounding_box(
                        file,
                        text,
                        container,
                        measure,
                        &mut self.env,
                    );
                }
                duplicated.new_ids
            }
            Pasted::Text(text) => {
                let props = ElementProps {
                    x: at[0],
                    y: at[1],
                    width: 0.0,
                    height: 0.0,
                    angle: 0.0,
                    stroke_color: self.style.stroke_color.clone(),
                    background_color: self.style.background_color.clone(),
                    fill_style: self.style.fill_style.clone(),
                    stroke_width: self.style.stroke_width.value(false),
                    stroke_style: self.style.stroke_style.clone(),
                    roughness: self.style.roughness,
                    opacity: self.style.opacity,
                    group_ids: Vec::new(),
                    roundness: None,
                    locked: false,
                };
                let text_props = TextProps {
                    text,
                    font_size: Some(self.style.font_size),
                    font_family: Some(self.style.font_family),
                    text_align: Some("left".into()),
                    vertical_align: Some("top".into()),
                    container_id: None,
                    line_height: None,
                };
                let mut element =
                    new_element::new_text_element(props, text_props, measure, &mut self.env);
                if let Some(p) = element.placement() {
                    element.set_position(at[0] - p.width / 2.0, at[1] - p.height / 2.0);
                }
                let id = element.id().expect("new text element has an id").to_owned();
                let file = clone_scene(&mut self.file, &mut self.scene_clones);
                edit::append_element(file, element, &mut self.env);
                vec![id]
            }
        };

        self.selection = Selection::from_ids(new_ids);
        self.finish_edit(&before, &selection_before)
    }

    /// Writes the text being edited back as one history step and ends editing; see
    /// `text::commit_text`. Ignored (returning `false`) when nothing is being edited.
    pub fn commit_text(&mut self, text: &str, measure: &mut dyn TextMeasure) -> bool {
        text::commit_text(self, text, measure)
    }

    /// The selection tool's double click: starts editing the text or container label under the
    /// pointer, or a new one; see `text::double_click`. Returns whether editing started.
    pub fn double_click(&mut self, event: PointerEvent, measure: &mut dyn TextMeasure) -> bool {
        text::double_click(self, event, measure)
    }

    /// For the latest pointer position.
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn overlay(&mut self, zoom: f64) -> Overlay {
        let positions = self.selection.positions(&self.file);
        let pad = 4.0 / zoom;

        let single = match positions.as_slice() {
            [position] => Some(*position),
            _ => None,
        };

        let mut outlines = Vec::new();
        let skip_outlines =
            single.is_some_and(|p| select::is_two_point_linear(&self.file.elements[p]));
        if !skip_outlines {
            for &position in &positions {
                let element = &self.file.elements[position];
                if let Some((bounds, center)) = self.geometry.absolute_coords(element) {
                    let angle = element.placement().map_or(0.0, |p| p.angle);
                    outlines.push(padded_rotated_corners(bounds, center, angle, pad));
                }
            }
        }

        let selection_box = if positions.len() >= 2 {
            selection::selected_bounds(&mut self.geometry, &self.file, &self.selection)
                .map(|[x1, y1, x2, y2]| [x1 - pad, y1 - pad, x2 + pad, y2 + pad])
        } else {
            None
        };

        let handles =
            transform::selection_handles(&mut self.geometry, &self.file, &self.selection, zoom)
                .into_iter()
                .map(|(_, bounds)| bounds)
                .collect();

        let points = match single {
            Some(position) if select::is_plain_linear(&self.file.elements[position]) => {
                let element = &self.file.elements[position];
                select::scene_points(&mut self.geometry, element).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        let box_selection = select::box_selection_overlay(&self.select_gesture);

        Overlay {
            outlines,
            selection_box,
            handles,
            points,
            box_selection,
        }
    }

    /// Records `before` -> the current scene as one history entry (a no-op, returning `false`,
    /// when nothing actually changed), bumps `revision` when it did, and drops any selected id
    /// that no longer names a live element.
    fn finish_edit(&mut self, before: &SceneFile, selection_before: &Selection) -> bool {
        let changed = self
            .history
            .record(before, &self.file, selection_before, &self.selection);
        if changed {
            self.revision += 1;
        }
        self.prune_selection();
        changed
    }

    /// Drops every selected id that no longer names a live (non-deleted, present) element.
    fn prune_selection(&mut self) {
        let kept: Vec<String> = self
            .selection
            .iter()
            .filter(|&id| {
                self.file
                    .elements
                    .iter()
                    .any(|e| !e.is_deleted() && e.id() == Some(id))
            })
            .map(str::to_owned)
            .collect();
        self.selection = Selection::from_ids(kept);
    }
}

/// The entry point for anything that mutates scene elements: clones the scene once when
/// another `Arc` still shares it (counted in `scene_clones`), then hands back mutable access to
/// it alone. A free function, not a method, so callers can still borrow other `Editor` fields
/// (its `Env`, its `GeometryCache`, ...) at the same time.
fn clone_scene<'a>(file: &'a mut Arc<SceneFile>, scene_clones: &mut u64) -> &'a mut SceneFile {
    if Arc::strong_count(&*file) > 1 {
        *scene_clones += 1;
    }
    Arc::make_mut(file)
}

/// The padded, rotated corners of `bounds` in `[nw, ne, se, sw]` order (`renderSelectionBorder`).
fn padded_rotated_corners(bounds: Bounds, center: [f64; 2], angle: f64, pad: f64) -> [[f64; 2]; 4] {
    let [x1, y1, x2, y2] = bounds;
    let (x1, y1, x2, y2) = (x1 - pad, y1 - pad, x2 + pad, y2 + pad);
    let corners = [[x1, y1], [x2, y1], [x2, y2], [x1, y2]];
    if angle == 0.0 {
        corners
    } else {
        corners.map(|p| rotate_point(p, center, angle))
    }
}
