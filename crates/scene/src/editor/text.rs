//! The text tool and text editing: `Tool::Text`'s pointer-down (`handleTextOnPointerDown` →
//! `startTextEditing`), the selection tool's double-click (`handleCanvasDoubleClick`), and
//! writing the edited text back to the scene (`handleTextWysiwyg`'s `onSubmit` closure and
//! `wysiwyg/textWysiwyg.tsx`'s own `onSubmit`), all in `packages/excalidraw/components/App.tsx`
//! at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! Text editing is a field of the editor, not a pointer gesture (spec deviation 5): nothing is
//! written back to the file until [`commit_text`]. A brand new text or label that ends up empty
//! is therefore never created at all, rather than being inserted empty by `startTextEditing`
//! and soft-deleted again by `onSubmit`; an existing one that ends up empty is soft-deleted the
//! same way [`edit::delete_selection`] deletes a selection. Creating a new container label goes
//! through `batch::add::bind_label` (the same primitive the AI batch interface uses), not a
//! second copy of `startTextEditing`'s own container-binding branch: napkin has no sticky notes
//! and does not grow a container to fit its label (spec deviations 1, M5), so that branch's
//! remaining logic is just what `bind_label` already does.
//!
//! Left out of this port, matching the rest of `scene`'s scope: frames, sticky notes, elbow
//! arrow endpoint labels (an arrow is never a text container here), the autoshape tool, grid
//! snapping, and `startTextEditing`'s `TEXT_TO_CENTER_SNAP_THRESHOLD` (30 scene units) gate on
//! binding to a container. Clicking or double-clicking anywhere inside a rectangle, diamond or
//! ellipse's bounding box always edits or creates its label here; the JS only does that within
//! 30 units of the container's center, and creates unbound free text (inheriting the
//! container's angle) for a miss further out.

use std::collections::HashSet;
use std::sync::Arc;

use crate::batch::add::{LabelSpec, bind_label};
use crate::collision;
use crate::edit;
use crate::element::Element;
use crate::env::Env;
use crate::file::SceneFile;
use crate::fractional_index;
use crate::geometry::GeometryCache;
use crate::new_element::{ElementProps, TextProps, bump_version, new_text_element};
use crate::selection::{self, Selection};
use crate::text::{self, TextMeasure};
use crate::transform;

use super::{Editor, PointerEvent, Tool, clone_scene};

/// The text element being edited, until [`commit_text`] writes it back
/// (`state.editingTextElement` plus the font/color/opacity fields `textWysiwyg` reads off it
/// and off `appState`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextEditing {
    /// The text element being edited; `None` while typing a new one.
    pub element_id: Option<String>,
    /// The container a new label will be bound to.
    pub container_id: Option<String>,
    /// Current text shown in the editor.
    pub text: String,
    /// Top-left of the editor box in scene coordinates.
    pub origin: [f64; 2],
    /// Width the editor box starts at (the element's width, or 0 for new text).
    pub width: f64,
    pub font_family: f64,
    pub font_size: f64,
    pub line_height: f64,
    pub text_align: String,
    pub stroke_color: String,
    pub opacity: f64,
    pub angle: f64,
}

/// Topmost non-deleted, non-locked element hit at `point` (`getElementAtPosition` with
/// `includeBoundTextElement: true`): unlike [`selection::element_at`], a container-bound text
/// label counts as its own hittable element here rather than being folded into its container.
fn topmost_hit(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    zoom: f64,
) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, element)| {
            if element.is_deleted() || element.is_locked() {
                return None;
            }
            let threshold = collision::hit_threshold(element, zoom);
            collision::hit_element_itself(geometry, element, point, threshold).then_some(index)
        })
}

/// Topmost non-deleted rectangle, diamond or ellipse whose (rotation-aware) bounding box
/// contains `point` (`getTextBindableContainerAtPosition`'s hit-test loop, napkin's three
/// container kinds only; unlike the JS, this does not skip a locked one, matching it exactly).
fn container_at(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, element)| {
            if element.is_deleted()
                || !matches!(
                    element,
                    Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
                )
            {
                return None;
            }
            selection::hit_element_bounding_box(geometry, element, point, 0.0).then_some(index)
        })
}

/// `element`'s bound text label, if it has one and it is still live (`getBoundTextElement`).
fn bound_label_of(elements: &[Element], element: &Element) -> Option<usize> {
    let (label_id, _) = element
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")?;
    elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(label_id))
}

/// What a pointer-down or double-click at a point should edit.
enum Target {
    ExistingText(usize),
    NewLabel(usize),
    NewFreeText,
}

/// `handleTextOnPointerDown`'s target resolution, napkin's container/free-text subset only
/// (no arrow endpoint labels): the topmost hit if it is text, else that hit's existing label if
/// it is a container that already has one (`hasBoundTextElement`), else a container found by
/// bounding box (a new label), else new free text at the pointer.
fn text_tool_target(editor: &mut Editor<impl Env>, point: [f64; 2], zoom: f64) -> Target {
    if let Some(index) = topmost_hit(&mut editor.geometry, &editor.file.elements, point, zoom) {
        if matches!(editor.file.elements[index], Element::Text(_)) {
            return Target::ExistingText(index);
        }
        if let Some(label) = bound_label_of(&editor.file.elements, &editor.file.elements[index]) {
            return Target::ExistingText(label);
        }
    }
    match container_at(&mut editor.geometry, &editor.file.elements, point) {
        Some(index) => Target::NewLabel(index),
        None => Target::NewFreeText,
    }
}

/// `handleCanvasDoubleClick`'s target resolution: a container found at the point takes
/// priority (its existing label, or a new one) over a free text hit, matching
/// `getTextBindableContainerAtPosition` running before the free-text fallback in the JS.
fn double_click_target(editor: &mut Editor<impl Env>, point: [f64; 2], zoom: f64) -> Target {
    if let Some(container) = container_at(&mut editor.geometry, &editor.file.elements, point) {
        return match bound_label_of(&editor.file.elements, &editor.file.elements[container]) {
            Some(label) => Target::ExistingText(label),
            None => Target::NewLabel(container),
        };
    }
    match topmost_hit(&mut editor.geometry, &editor.file.elements, point, zoom) {
        Some(index) if matches!(editor.file.elements[index], Element::Text(_)) => {
            Target::ExistingText(index)
        }
        _ => Target::NewFreeText,
    }
}

/// The [`TextEditing`] a resolved [`Target`] starts with: an existing text's own current
/// values, a new label's from its container and the current [`super::ItemStyle`], or a new
/// free text's from `point` and the current style (`startTextEditing`'s field assembly).
fn editing_for(editor: &Editor<impl Env>, target: Target, point: [f64; 2]) -> TextEditing {
    match target {
        Target::ExistingText(index) => {
            let Element::Text(t) = &editor.file.elements[index] else {
                unreachable!(
                    "text_tool_target/double_click_target only resolve text positions here"
                )
            };
            TextEditing {
                element_id: Some(t.base.id.clone()),
                container_id: t.container_id.value().cloned(),
                text: t.text.clone(),
                origin: [t.base.x, t.base.y],
                width: t.base.width,
                font_family: t.font_family,
                font_size: t.font_size,
                line_height: t
                    .line_height
                    .unwrap_or_else(|| text::line_height(t.font_family)),
                text_align: t.text_align.clone(),
                stroke_color: t.base.stroke_color.clone(),
                opacity: t.base.opacity,
                angle: t.base.angle,
            }
        }
        Target::NewLabel(container_index) => {
            let container = &editor.file.elements[container_index];
            let placement = container
                .placement()
                .expect("a rectangle/diamond/ellipse container always has a placement");
            let style = &editor.style;
            TextEditing {
                element_id: None,
                container_id: Some(
                    container
                        .id()
                        .expect("a container always has an id")
                        .to_owned(),
                ),
                text: String::new(),
                origin: [
                    placement.x + placement.width / 2.0,
                    placement.y + placement.height / 2.0,
                ],
                width: 0.0,
                font_family: style.font_family,
                font_size: style.font_size,
                line_height: text::line_height(style.font_family),
                text_align: "center".to_owned(),
                stroke_color: style.stroke_color.clone(),
                opacity: style.opacity,
                angle: placement.angle,
            }
        }
        Target::NewFreeText => {
            let style = &editor.style;
            let line_height = text::line_height(style.font_family);
            // `startTextEditing`'s free-text position: the click point itself for x, and y
            // shifted up by half a line's pixel height so the first line is centered on it
            // (`getLineHeightInPx(fontSize, lineHeight) / 2`, since no grid is in effect).
            TextEditing {
                element_id: None,
                container_id: None,
                text: String::new(),
                origin: [point[0], point[1] - style.font_size * line_height / 2.0],
                width: 0.0,
                font_family: style.font_family,
                font_size: style.font_size,
                line_height,
                text_align: "left".to_owned(),
                stroke_color: style.stroke_color.clone(),
                opacity: style.opacity,
                angle: 0.0,
            }
        }
    }
}

/// `Tool::Text`'s pointer-down: starts editing at `event`'s position, or does nothing while
/// already editing (`if (this.state.editingTextElement) return;`) or under another tool.
/// Deselects everything first (`handleTextWysiwyg`'s `this.deselectElements()`): the eventual
/// selection is decided by [`commit_text`] instead.
pub(super) fn pointer_down(editor: &mut Editor<impl Env>, event: PointerEvent) {
    if editor.tool != Tool::Text || editor.text_editing.is_some() {
        return;
    }
    let target = text_tool_target(editor, event.at, event.zoom);
    let editing = editing_for(editor, target, event.at);
    editor.selection = Selection::new();
    editor.text_editing = Some(editing);
}

/// The selection tool's double click: starts editing the text or container label under the
/// pointer, or a new one, the same way [`pointer_down`] does for the text tool. A no-op
/// (returning `false`) under any other tool or mid-gesture (`activeTool.type !==
/// preferredSelectionTool.type`, `state.multiElement`, `state.editingTextElement`, all folded
/// into [`Editor::is_idle`] once text editing counts toward it).
pub(super) fn double_click(editor: &mut Editor<impl Env>, event: PointerEvent) -> bool {
    if editor.tool != Tool::Selection || !editor.is_idle() {
        return false;
    }
    let target = double_click_target(editor, event.at, event.zoom);
    let editing = editing_for(editor, target, event.at);
    editor.selection = Selection::new();
    editor.text_editing = Some(editing);
    true
}

/// Rewrites an existing text's content in place (`textWysiwyg`'s `onSubmit`, kept-text branch):
/// re-measures at its current font, repositions a container's label with
/// [`transform::bound_text_position`], and leaves a standalone text's top-left exactly where it
/// was. Unlike `properties.rs`'s `redraw_text` (the property panel's font-size/family change),
/// this does not recentre a standalone text at all: `getAdjustedDimensions`' anchor-preserving
/// math keeps a left/top-aligned, unrotated text's top-left fixed on a content edit too (the
/// only alignment/angle napkin's own UI ever creates), so the simpler fixed-top-left rule
/// matches the JS for everything napkin can produce; it can disagree with the JS for a loaded
/// file's differently aligned or rotated text.
fn update_existing_text(
    file: &mut SceneFile,
    id: &str,
    text: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) {
    let position = file
        .elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(id))
        .expect("the text being edited still exists");
    let before = file.elements[position].clone();

    let normalized = text::normalize_text(text);
    let Element::Text(t) = &mut file.elements[position] else {
        unreachable!("commit_text only edits text elements")
    };
    let line_height = t
        .line_height
        .unwrap_or_else(|| text::line_height(t.font_family));
    let [width, height] = text::measure_text(
        &normalized,
        t.font_family,
        t.font_size,
        line_height,
        measure,
    );
    t.text = normalized.clone();
    t.original_text = Some(normalized);
    t.base.width = width;
    t.base.height = height;
    let container_id = t.container_id.value().cloned();

    if let Some(container_id) = container_id
        && let Some(container) = file
            .elements
            .iter()
            .find(|e| !e.is_deleted() && e.id() == Some(container_id.as_str()))
            .cloned()
    {
        let Element::Text(t) = &file.elements[position] else {
            unreachable!("checked above")
        };
        if let Some([x, y]) = transform::bound_text_position(&container, t) {
            let Element::Text(t) = &mut file.elements[position] else {
                unreachable!("checked above")
            };
            t.base.x = x;
            t.base.y = y;
        }
    }

    if file.elements[position] != before {
        bump_version(&mut file.elements[position], env);
    }
}

/// A new label on `container_id`, via the same `bindTextToContainer`/`redrawTextBoundingBox`
/// primitive the AI batch interface uses (`batch::add::bind_label`): centered, not wrapped,
/// left overflowing when it does not fit (spec deviation 1). Returns the label's id.
fn create_new_label(
    file: &mut SceneFile,
    container_id: &str,
    editing: &TextEditing,
    text: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> String {
    let position = file
        .elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(container_id))
        .expect("the label's container still exists");
    let label = LabelSpec {
        text: text.to_owned(),
        font_size: Some(editing.font_size),
        font_family: Some(editing.font_family),
        text_align: None,
        vertical_align: None,
    };
    let mut warnings = Vec::new();
    let label_position = bind_label(
        file,
        position,
        &label,
        container_id,
        measure,
        env,
        &mut warnings,
    );
    let label_id = file.elements[label_position]
        .id()
        .expect("a freshly created label has an id")
        .to_owned();
    let moved: HashSet<String> = [label_id.clone()].into_iter().collect();
    fractional_index::sync_moved_indices(&mut file.elements, &moved, env);
    label_id
}

/// A new standalone text at `editing.origin` (`newTextElement`, left-aligned, top-anchored, so
/// `editing.origin` is already its final top-left). Returns the text's id.
fn create_new_free_text(
    file: &mut SceneFile,
    editing: &TextEditing,
    text: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> String {
    let props = ElementProps {
        x: editing.origin[0],
        y: editing.origin[1],
        angle: editing.angle,
        stroke_color: editing.stroke_color.clone(),
        opacity: editing.opacity,
        ..ElementProps::default()
    };
    let text_props = TextProps {
        text: text.to_owned(),
        font_size: Some(editing.font_size),
        font_family: Some(editing.font_family),
        text_align: Some("left".to_owned()),
        vertical_align: Some("top".to_owned()),
        container_id: None,
        line_height: Some(editing.line_height),
    };
    let element = new_text_element(props, text_props, measure, env);
    let id = element
        .id()
        .expect("a freshly created text element has an id")
        .to_owned();
    edit::append_element(file, element, env);
    id
}

/// Writes the edited text back as one history step (`textWysiwyg`'s `onSubmit` and
/// `handleTextWysiwyg`'s submit branch), ends editing and returns the tool to Selection. A
/// no-op (returning `false`) when nothing was being edited.
///
/// `text.trim()` being empty (`!nextOriginalText.trim()`) means: a new text or label is never
/// created at all; an existing one is soft-deleted like [`edit::delete_selection`] would
/// (clearing it from its container's `boundElements` too). Otherwise a new text or label is
/// created, or an existing one's content is rewritten in place. Selection afterwards
/// (`element.containerId || (!isDeleted ? element.id : null)`, applied unconditionally rather
/// than only on a keyboard submit, napkin's own submit path having no such distinction):
/// the container when the (possibly just-created) text is bound to one, else the text itself
/// when it survives, else nothing.
pub(super) fn commit_text(
    editor: &mut Editor<impl Env>,
    text: &str,
    measure: &mut dyn TextMeasure,
) -> bool {
    let Some(editing) = editor.text_editing.take() else {
        return false;
    };
    editor.tool = Tool::Selection;
    let deleted = text.trim().is_empty();

    let before = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();

    let surviving_id: Option<String> = match (&editing.element_id, deleted) {
        (Some(id), true) => {
            let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
            edit::delete_selection(file, &Selection::from_ids([id.clone()]), &mut editor.env);
            Some(id.clone())
        }
        (Some(id), false) => {
            let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
            update_existing_text(file, id, text, measure, &mut editor.env);
            Some(id.clone())
        }
        (None, true) => None,
        (None, false) => {
            let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
            let id = match &editing.container_id {
                Some(container_id) => {
                    create_new_label(file, container_id, &editing, text, measure, &mut editor.env)
                }
                None => create_new_free_text(file, &editing, text, measure, &mut editor.env),
            };
            Some(id)
        }
    };

    editor.selection = match surviving_id {
        None => Selection::new(),
        Some(id) => {
            let bound_container = editor
                .file
                .elements
                .iter()
                .find(|e| e.id() == Some(id.as_str()))
                .and_then(Element::container_id)
                .map(str::to_owned);
            match bound_container {
                Some(container_id) => Selection::from_ids([container_id]),
                None if deleted => Selection::new(),
                None => Selection::from_ids([id]),
            }
        }
    };

    editor.finish_edit(&before, &selection_before)
}
