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
//! The selection tool and the creation tools each keep their own gesture (`select_gesture`,
//! `create_gesture`); at most one is active at a time, since [`Editor::set_tool`] finishes both
//! before switching. `Command::Escape` clears the selection while idle, discards or finishes an
//! active creation gesture (see `create::escape`), and otherwise does nothing: napkin has no
//! other in-progress edit to cancel.

mod create;
mod select;
mod style;

use std::sync::Arc;

use crate::edit;
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{Bounds, GeometryCache, rotate_point};
use crate::history::History;
use crate::selection::{self, Selection};
use crate::transform;

pub use style::{ArrowType, EdgeStyle, ItemStyle, StrokeWidth};

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
    /// history and any gesture. Does not change [`Editor::revision`].
    pub fn replace_file(&mut self, mut file: SceneFile) {
        edit::repair_on_load(&mut file, &mut self.env);
        self.file = Arc::new(file);
        self.selection = Selection::new();
        self.history.clear();
        self.select_gesture = select::Gesture::None;
        self.create_gesture = create::Gesture::None;
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
    pub fn set_tool(&mut self, tool: Tool) {
        select::finish_gesture(self, None);
        create::finish_gesture(self, None);
        if !matches!(tool, Tool::Selection | Tool::Hand) {
            self.selection = Selection::new();
            self.cursor = Cursor::Crosshair;
        }
        self.tool = tool;
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The style values (`currentItem*`) a newly created element takes.
    pub fn style(&self) -> &ItemStyle {
        &self.style
    }

    /// No pointer gesture and no multi-point line in progress.
    pub fn is_idle(&self) -> bool {
        matches!(self.select_gesture, select::Gesture::None)
            && matches!(self.create_gesture, create::Gesture::None)
    }

    pub fn pointer_down(&mut self, event: PointerEvent) {
        select::pointer_down(self, event);
        create::pointer_down(self, event);
    }

    pub fn pointer_move(&mut self, event: PointerEvent) {
        select::pointer_move(self, event);
        create::pointer_move(self, event);
    }

    pub fn pointer_up(&mut self, event: PointerEvent) {
        select::pointer_up(self, event);
        create::pointer_up(self, event);
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
                if !self.is_idle() || self.selection.is_empty() {
                    return false;
                }
                self.selection = Selection::new();
                true
            }
            Command::Finalize => create::finalize_command(self),
        }
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
