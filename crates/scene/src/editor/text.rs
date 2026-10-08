//! The text tool and text editing: `Tool::Text`'s pointer-down (`handleTextOnPointerDown` →
//! `startTextEditing`), the selection tool's double-click (`handleCanvasDoubleClick`), and
//! writing the edited text back to the scene (`handleTextWysiwyg`'s `onSubmit` closure and
//! `wysiwyg/textWysiwyg.tsx`'s own `onSubmit`), all in `packages/excalidraw/components/App.tsx`
//! at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! Text editing is a field of the editor, not a pointer gesture: nothing is written back to the
//! file until [`commit_text`]. A brand new text or label that ends up empty is therefore never
//! created at all, rather than being inserted empty by `startTextEditing` and soft-deleted again
//! by `onSubmit`; an existing one that ends up empty is soft-deleted the same way
//! [`edit::delete_selection`] deletes a selection. Creating a new container label goes through
//! `batch::add::bind_label` (the same primitive the AI batch interface uses), not a second copy
//! of `startTextEditing`'s own container-binding branch: napkin has no sticky notes, so that
//! branch's remaining logic is just what `bind_label` already does.
//!
//! Left out of this port, matching the rest of `scene`'s scope: frames, sticky notes, elbow
//! arrow endpoint labels (an arrow is never a text container here), the autoshape tool, and
//! grid snapping (`getTextCreationGridPoint` is moot without a grid mode, so the free-text
//! position formula below is exact, not an approximation).
//!
//! A click or double-click within a rectangle/diamond/ellipse's bounding box only binds to it
//! as a label within `TEXT_TO_CENTER_SNAP_THRESHOLD` (30 scene units) of its center
//! (`getTextWysiwygSnappedToCenterPosition`); further out it creates unbound free text at the
//! click point instead, inheriting the container's angle and `groupIds`
//! (`startTextEditing`'s own field assembly does this for *every* miss, bound or not, as long
//! as some container was found under the point at all). A double-click additionally forces a
//! bind regardless of distance when the container already has a label, has a non-transparent
//! background, or the click lands on its own outline (`handleCanvasDoubleClick`'s pre-snap
//! branch, which the text tool's own `handleTextOnPointerDown` does not have — its distance
//! check runs unconditionally once the click was inside the container's bounding box at all).

use std::collections::HashSet;
use std::sync::Arc;

use crate::batch::add::{LabelSpec, bind_label};
use crate::bound_text;
use crate::collision;
use crate::color::is_transparent;
use crate::edit;
use crate::element::Element;
use crate::env::Env;
use crate::file::SceneFile;
use crate::fractional_index;
use crate::geometry::GeometryCache;
use crate::new_element::{ElementProps, TextProps, bump_version, new_text_element};
use crate::selection::Selection;
use crate::text::{self, TextMeasure};

use super::{Editor, PointerEvent, Tool, clone_scene};

/// `TEXT_TO_CENTER_SNAP_THRESHOLD` (`packages/common/src/constants.ts`), in scene units.
const TEXT_TO_CENTER_SNAP_THRESHOLD: f64 = 30.0;

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
    /// A container's `groupIds`, inherited by a free text created near it that missed the
    /// center-snap threshold (`startTextEditing`'s `groupIds: container?.groupIds ?? []`);
    /// empty for every other kind of edit.
    pub group_ids: Vec<String>,
    /// Width the editor wraps at, in scene units: a container label's max width
    /// (`getBoundTextMaxWidth`), a fixed-width text's own width, `None` for text that grows
    /// with its content.
    pub wrap_width: Option<f64>,
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

/// `getTextBindableContainerAtPosition`'s hit-test loop: the topmost live element whose
/// (unrotated, `getElementAbsoluteCoords`) box strictly contains `point`, or an arrow whose
/// outline is hit, whatever its type (frames are skipped). That element is the container only
/// if it is a rectangle, diamond or ellipse, so a text drawn over a bigger shape shadows the
/// shape behind it. Locked elements are not skipped, as in the JS.
fn container_at(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    zoom: f64,
) -> Option<usize> {
    let [x, y] = point;
    let mut hit = None;
    for (index, element) in elements.iter().enumerate().rev() {
        if element.is_deleted() {
            continue;
        }
        if element.kind() == "arrow"
            && collision::hit_element_itself(
                geometry,
                element,
                point,
                collision::hit_threshold(element, zoom),
            )
        {
            hit = Some(index);
            break;
        }
        let Some(([x1, y1, x2, y2], _)) = geometry.absolute_coords(element) else {
            continue;
        };
        if x1 < x && x < x2 && y1 < y && y < y2 {
            if matches!(element.kind(), "frame" | "magicframe") {
                continue;
            }
            hit = Some(index);
            break;
        }
    }
    hit.filter(|&index| {
        matches!(
            elements[index],
            Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
        )
    })
}

/// The position of `id`'s live (non-deleted) element, if it still has one.
fn live_position(elements: &[Element], id: &str) -> Option<usize> {
    elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(id))
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

/// A container's label wrap width. A tiny container's max width is negative; no wrap width is
/// then in effect, which agrees with `wrap_text` treating a negative width as no wrapping.
fn container_wrap_width(container: &Element) -> Option<f64> {
    bound_text::bound_text_max_size(container)
        .map(|[w, _]| w)
        .filter(|w| *w > 0.0)
}

/// The width an existing text's editor wraps at: its container's max width for a label, its own
/// width for a standalone `autoResize: false` text, none otherwise.
fn existing_wrap_width(elements: &[Element], text: &crate::element::TextElement) -> Option<f64> {
    if let Some(container_id) = text.container_id.value() {
        let container = elements
            .iter()
            .find(|e| !e.is_deleted() && e.id() == Some(container_id.as_str()))?;
        return container_wrap_width(container);
    }
    (text.auto_resize == Some(false)).then_some(text.base.width)
}

/// What a pointer-down or double-click at a point should edit. `NewFreeText`'s `near_container`
/// is the container the click missed the center-snap threshold of, if any (for the angle and
/// `groupIds` a free text created there inherits); `None` for a click with no container at all.
enum Target {
    ExistingText(usize),
    NewLabel(usize),
    NewFreeText { near_container: Option<usize> },
}

/// `getContainerCenter`, restricted to napkin's non-arrow containers: `container.x +
/// container.width / 2`, `container.y + container.height / 2`. The JS does not rotate this
/// back into scene space for a rotated container, so neither does this, even though every
/// other bound-text placement in `scene` does.
fn container_center(container: &Element) -> [f64; 2] {
    let placement = container
        .placement()
        .expect("a rectangle/diamond/ellipse container always has a placement");
    [
        placement.x + placement.width / 2.0,
        placement.y + placement.height / 2.0,
    ]
}

/// `getTextWysiwygSnappedToCenterPosition`'s own check: whether `point` is within
/// [`TEXT_TO_CENTER_SNAP_THRESHOLD`] scene units of `container`'s center.
fn near_container_center(container: &Element, point: [f64; 2]) -> bool {
    let [cx, cy] = container_center(container);
    rough::js::hypot(point[0] - cx, point[1] - cy) < TEXT_TO_CENTER_SNAP_THRESHOLD
}

/// `handleTextOnPointerDown`'s target resolution, napkin's container/free-text subset only
/// (no arrow endpoint labels): the topmost hit if it is text, else that hit's existing label if
/// it is a container that already has one (`hasBoundTextElement`), else a container found by
/// bounding box, bound only within the center-snap threshold (a new label; further out, new
/// free text near it), else new free text with no container at all.
fn text_tool_target(editor: &mut Editor<impl Env>, point: [f64; 2], zoom: f64) -> Target {
    if let Some(index) = topmost_hit(&mut editor.geometry, &editor.file.elements, point, zoom) {
        if matches!(editor.file.elements[index], Element::Text(_)) {
            return Target::ExistingText(index);
        }
        if let Some(label) = bound_label_of(&editor.file.elements, &editor.file.elements[index]) {
            return Target::ExistingText(label);
        }
    }
    match container_at(&mut editor.geometry, &editor.file.elements, point, zoom) {
        Some(index) if near_container_center(&editor.file.elements[index], point) => {
            Target::NewLabel(index)
        }
        Some(index) => Target::NewFreeText {
            near_container: Some(index),
        },
        None => Target::NewFreeText {
            near_container: None,
        },
    }
}

/// `handleCanvasDoubleClick`'s target resolution: a container found at the point takes
/// priority (its existing label, or a new one) over a free text hit, matching
/// `getTextBindableContainerAtPosition` running before the free-text fallback in the JS. A
/// labelless container binds unconditionally when it has a non-transparent background or the
/// click lands on its own outline (the pre-snap branch that forces `sceneX`/`sceneY` onto its
/// exact center before the shared center-snap check ever runs), else only within the same
/// center-snap threshold [`text_tool_target`] uses.
fn double_click_target(editor: &mut Editor<impl Env>, point: [f64; 2], zoom: f64) -> Target {
    if let Some(index) = container_at(&mut editor.geometry, &editor.file.elements, point, zoom) {
        let container = editor.file.elements[index].clone();
        if let Some(label) = bound_label_of(&editor.file.elements, &container) {
            return Target::ExistingText(label);
        }
        let opaque = !is_transparent(
            &container
                .base()
                .expect("a container always has a base")
                .background_color,
        );
        let threshold = collision::hit_threshold(&container, zoom);
        let hits_outline =
            collision::hit_element_itself(&mut editor.geometry, &container, point, threshold);
        let forced = opaque || hits_outline;
        return if forced || near_container_center(&container, point) {
            Target::NewLabel(index)
        } else {
            Target::NewFreeText {
                near_container: Some(index),
            }
        };
    }
    match topmost_hit(&mut editor.geometry, &editor.file.elements, point, zoom) {
        Some(index) if matches!(editor.file.elements[index], Element::Text(_)) => {
            Target::ExistingText(index)
        }
        _ => Target::NewFreeText {
            near_container: None,
        },
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
            let wrap_width = existing_wrap_width(&editor.file.elements, t);
            // The overlay lays its lines out inside a `wrap_width`-wide box anchored at its
            // left edge, aligned per `text_align`. A bound label is drawn at its own measured
            // width, so the box starts that far left of the label for the lines to sit where
            // the label is drawn (`textWysiwyg` keeps the editor on the label).
            let origin_x = match (wrap_width, t.container_id.value()) {
                (Some(wrap), Some(_)) => {
                    let slack = (wrap - t.base.width).max(0.0);
                    let share = match t.text_align.as_str() {
                        "center" => 0.5,
                        "right" => 1.0,
                        _ => 0.0,
                    };
                    t.base.x - slack * share
                }
                _ => t.base.x,
            };
            TextEditing {
                element_id: Some(t.base.id.clone()),
                container_id: t.container_id.value().cloned(),
                // `originalText`, not the wrapped `text` (`textWysiwyg`'s initial value).
                text: t.original_text.clone().unwrap_or_else(|| t.text.clone()),
                origin: [origin_x, t.base.y],
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
                group_ids: Vec::new(),
                wrap_width,
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
                group_ids: Vec::new(),
                wrap_width: container_wrap_width(container),
            }
        }
        Target::NewFreeText { near_container } => {
            let style = &editor.style;
            let line_height = text::line_height(style.font_family);
            // `startTextEditing`'s own field assembly gives free text the angle and groupIds
            // of whatever container the click found, bound or not (`container?.angle`,
            // `container?.groupIds ?? []`); a click with no container at all gets neither.
            let (angle, group_ids) = match near_container.map(|i| &editor.file.elements[i]) {
                Some(container) => (
                    container.placement().map_or(0.0, |p| p.angle),
                    container
                        .group_ids()
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                ),
                None => (0.0, Vec::new()),
            };
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
                angle,
                group_ids,
                wrap_width: None,
            }
        }
    }
}

/// `startTextEditing`'s container branch: a container too small for even one character of the
/// new label grows to `getApproxMinLineWidth`/`getApproxMinLineHeight` before the editor opens.
/// The grown scene is `editor.file` straight away; the scene from before the growth is kept in
/// `text_edit_base`, so [`commit_text`] records growth and label as one history step and an
/// abandoned edit puts the container back exactly.
fn grow_container_for_new_label(
    editor: &mut Editor<impl Env>,
    container: usize,
    measure: &mut dyn TextMeasure,
) {
    let line_height = text::line_height(editor.style.font_family);
    let [min_width, min_height] = bound_text::approx_min_container_size(
        editor.style.font_family,
        editor.style.font_size,
        line_height,
        measure,
    );
    let placement = editor.file.elements[container]
        .placement()
        .expect("a rectangle/diamond/ellipse container always has a placement");
    let width = placement.width.max(min_width);
    let height = placement.height.max(min_height);
    if width == placement.width && height == placement.height {
        return;
    }
    editor.text_edit_base = Some(Arc::clone(&editor.file));
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
    let element = &mut file.elements[container];
    let base = element
        .base_mut()
        .expect("a rectangle/diamond/ellipse container is typed");
    base.width = width;
    base.height = height;
    bump_version(element, &mut editor.env);
}

/// `Tool::Text`'s pointer-down: starts editing at `event`'s position, or does nothing while
/// already editing (`if (this.state.editingTextElement) return;`) or under another tool.
/// Deselects everything first (`handleTextWysiwyg`'s `this.deselectElements()`): the eventual
/// selection is decided by [`commit_text`] instead.
pub(super) fn pointer_down(
    editor: &mut Editor<impl Env>,
    event: PointerEvent,
    measure: &mut dyn TextMeasure,
) {
    if editor.tool != Tool::Text || editor.text_editing.is_some() {
        return;
    }
    let target = text_tool_target(editor, event.at, event.zoom);
    if let Target::NewLabel(container) = target {
        grow_container_for_new_label(editor, container, measure);
    }
    let editing = editing_for(editor, target, event.at);
    editor.selection = Selection::new();
    editor.text_editing = Some(editing);
}

/// The selection tool's double click: starts editing the text or container label under the
/// pointer, or a new one, the same way [`pointer_down`] does for the text tool. A no-op
/// (returning `false`) under any other tool or mid-gesture (`activeTool.type !==
/// preferredSelectionTool.type`, `state.multiElement`, `state.editingTextElement`, all folded
/// into [`Editor::is_idle`] once text editing counts toward it).
pub(super) fn double_click(
    editor: &mut Editor<impl Env>,
    event: PointerEvent,
    measure: &mut dyn TextMeasure,
) -> bool {
    if editor.tool != Tool::Selection || !editor.is_idle() {
        return false;
    }
    let target = double_click_target(editor, event.at, event.zoom);
    if let Target::NewLabel(container) = target {
        grow_container_for_new_label(editor, container, measure);
    }
    let editing = editing_for(editor, target, event.at);
    editor.selection = Selection::new();
    editor.text_editing = Some(editing);
    true
}

/// Rewrites an existing text's content in place (`textWysiwyg`'s `onSubmit`, kept-text branch):
/// applies `editing`'s current font family/size, stroke color and opacity (`set_property` only
/// ever wrote those to `editing` itself while the edit was open, via `apply_to_text_editing`),
/// stores the text as `originalText`, and measures it at that font. A container's label, and a
/// standalone text with `autoResize: false`, then go through
/// [`bound_text::redraw_text_bounding_box`]: wrapped to the container (or to the text's own
/// width), growing the container and recentred in it. An `autoResize` standalone text is not
/// wrapped and keeps its top-left where it was: `getAdjustedDimensions`' anchor-preserving math
/// does the same for a left/top-aligned, unrotated text (the only alignment/angle napkin's own
/// UI ever creates); it can disagree with the JS for a loaded file's differently aligned or
/// rotated text. A label whose container is `Raw` is measured but not wrapped.
fn update_existing_text(
    file: &mut SceneFile,
    id: &str,
    editing: &TextEditing,
    text: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) {
    let position = live_position(&file.elements, id)
        .expect("commit_text only calls this once it has confirmed the text is still live");
    let before = file.elements[position].clone();

    let normalized = text::normalize_text(text);
    let Element::Text(t) = &mut file.elements[position] else {
        unreachable!("commit_text only edits text elements")
    };
    t.font_family = editing.font_family;
    t.font_size = editing.font_size;
    t.line_height = Some(editing.line_height);
    t.base.stroke_color = editing.stroke_color.clone();
    t.base.opacity = editing.opacity;
    let [width, height] = text::measure_text(
        &normalized,
        t.font_family,
        t.font_size,
        editing.line_height,
        measure,
    );
    t.text = normalized.clone();
    t.original_text = Some(normalized);
    let fixed_width = t.auto_resize == Some(false);
    if !fixed_width {
        t.base.width = width;
    }
    t.base.height = height;
    let container_id = t.container_id.value().cloned();

    match container_id {
        Some(container_id) => {
            if let Some(container) = live_position(&file.elements, &container_id) {
                bound_text::redraw_text_bounding_box(file, position, Some(container), measure, env);
            }
        }
        None if fixed_width => {
            bound_text::redraw_text_bounding_box(file, position, None, measure, env);
        }
        None => {}
    }

    // `redraw_text_bounding_box` bumps what it changed; this covers the edit it was handed.
    if file.elements[position] != before && file.elements[position].version() == before.version() {
        bump_version(&mut file.elements[position], env);
    }
}

/// A new label on `container_id`, via the same `bindTextToContainer`/`redrawTextBoundingBox`
/// primitive the AI batch interface uses (`batch::add::bind_label`): centered, wrapped to
/// the container and growing it when it does not fit, colored and made opaque per
/// `editing`'s stroke color and opacity (`currentItemStrokeColor`/`currentItemOpacity` at
/// `startTextEditing` time) rather than `bind_label`'s AI-batch default of the container's own
/// color. Bumps the container's own version for gaining the `boundElements` entry: unlike
/// `add_elements`, which only ever binds to a container it just created in the same batch
/// (still at version 1, matching the JS baseline), this container is a pre-existing one, so
/// nothing else records that mutation (see `bind_label`'s own doc comment). Returns the label's
/// id.
fn create_new_label(
    file: &mut SceneFile,
    container_id: &str,
    editing: &TextEditing,
    text: &str,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> String {
    let position = live_position(&file.elements, container_id)
        .expect("commit_text only calls this once it has confirmed the container is still live");
    let container_before = file.elements[position].clone();
    let label = LabelSpec {
        text: text.to_owned(),
        font_size: Some(editing.font_size),
        font_family: Some(editing.font_family),
        text_align: None,
        vertical_align: None,
        stroke_color: Some(editing.stroke_color.clone()),
        opacity: Some(editing.opacity),
    };
    let label_position = bind_label(file, position, &label, measure, env);
    if file.elements[position] != container_before
        && file.elements[position].version() == container_before.version()
    {
        bump_version(&mut file.elements[position], env);
    }
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
        group_ids: editing.group_ids.clone(),
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
///
/// The element (or, for a new label, the container) an in-progress edit refers to is checked
/// for existence again here rather than trusted: every editor path that could otherwise delete
/// or replace it while `text_editing` is `Some` already refuses to run (`Editor::is_idle`
/// covers the command surface; `select`/`create`/`erase::pointer_down` each bail on
/// `text_editing.is_some()` directly, since a double-click-started edit leaves `tool` at
/// `Selection` rather than switching to a tool those already gate on), but nothing here should
/// *rely* on every such path staying airtight forever. A non-empty edit whose target vanished
/// becomes a new free text at the edit's own origin instead (`textWysiwyg`'s own
/// `getElement(element.id)` guard has no equivalent fallback — the element it might not find
/// was never deletable out from under it in the first place, since JS inserts it into the
/// scene the moment editing starts); an empty edit of a vanished target is simply dropped,
/// same as an abandoned new text.
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

    // A new label's container may have grown when the editor opened; the step starts from the
    // scene before that.
    let growth_base = editor.text_edit_base.take();
    let before = growth_base
        .clone()
        .unwrap_or_else(|| Arc::clone(&editor.file));
    let selection_before = editor.selection.clone();

    let surviving_id: Option<String> = if let Some(id) = &editing.element_id {
        let live = live_position(&editor.file.elements, id).is_some();
        match (live, deleted) {
            (true, true) => {
                let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
                edit::delete_selection(file, &Selection::from_ids([id.clone()]), &mut editor.env);
                Some(id.clone())
            }
            (true, false) => {
                let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
                update_existing_text(file, id, &editing, text, measure, &mut editor.env);
                Some(id.clone())
            }
            // Erased, or otherwise removed, out from under this edit: nothing left to delete
            // or update.
            (false, true) => None,
            (false, false) => {
                let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
                Some(create_new_free_text(
                    file,
                    &editing,
                    text,
                    measure,
                    &mut editor.env,
                ))
            }
        }
    } else if deleted {
        if let Some(base) = growth_base {
            editor.file = base;
        }
        None
    } else {
        let file = clone_scene(&mut editor.file, &mut editor.scene_clones);
        let container_live = editing
            .container_id
            .as_deref()
            .is_some_and(|id| live_position(&file.elements, id).is_some());
        let id = if container_live {
            create_new_label(
                file,
                editing.container_id.as_deref().expect("checked above"),
                &editing,
                text,
                measure,
                &mut editor.env,
            )
        } else {
            create_new_free_text(file, &editing, text, measure, &mut editor.env)
        };
        Some(id)
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

#[cfg(test)]
mod tests {
    //! `commit_text`'s fallback for an edit target that vanished mid-edit. Every reachable way
    //! to make that happen through the public `Editor` API is already refused while
    //! `text_editing` is `Some` (see `commit_text`'s own doc comment), so exercising the
    //! fallback needs to plant a `TextEditing` whose id was never live in the first place,
    //! which only an internal test (able to reach `Editor`'s private `text_editing` field and
    //! this module's own `commit_text`) can do; `crates/scene/tests/editor_text.rs` covers the
    //! guards themselves through the public API instead.

    use serde_json::json;

    use super::*;
    use crate::env::Env;
    use crate::sample;

    struct TestEnv {
        state: u64,
        now: f64,
    }

    impl TestEnv {
        fn seeded(seed: u64) -> TestEnv {
            TestEnv {
                state: seed.max(1),
                now: 1.0,
            }
        }
    }

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            for byte in bytes {
                self.state ^= self.state << 13;
                self.state ^= self.state >> 7;
                self.state ^= self.state << 17;
                *byte = (self.state >> 32) as u8;
            }
        }

        fn now_ms(&mut self) -> f64 {
            self.now += 1.0;
            self.now
        }
    }

    fn stub_editing(element_id: Option<&str>, container_id: Option<&str>) -> TextEditing {
        TextEditing {
            element_id: element_id.map(str::to_owned),
            container_id: container_id.map(str::to_owned),
            text: "old".to_owned(),
            origin: [10.0, 20.0],
            width: 0.0,
            font_family: 5.0,
            font_size: 20.0,
            line_height: 1.25,
            text_align: "left".to_owned(),
            stroke_color: "#1e1e1e".to_owned(),
            opacity: 100.0,
            angle: 0.0,
            group_ids: Vec::new(),
            wrap_width: None,
        }
    }

    #[test]
    fn a_non_empty_edit_of_a_vanished_element_becomes_a_new_free_text() {
        let mut editor = Editor::new(sample::file(vec![]), TestEnv::seeded(1));
        editor.text_editing = Some(stub_editing(Some("gone"), None));

        assert!(commit_text(
            &mut editor,
            "revived",
            &mut sample::CharWidthMeasure
        ));

        assert!(editor.text_editing.is_none());
        assert_eq!(editor.file.elements.len(), 1);
        let v = editor.file.elements[0].to_value();
        assert_eq!(v["text"], json!("revived"));
        assert_eq!(v["x"], json!(10.0));
        assert_eq!(v["y"], json!(20.0));
    }

    #[test]
    fn an_empty_edit_of_a_vanished_element_is_dropped_without_panicking() {
        let mut editor = Editor::new(sample::file(vec![]), TestEnv::seeded(1));
        editor.text_editing = Some(stub_editing(Some("gone"), None));

        assert!(!commit_text(&mut editor, "", &mut sample::CharWidthMeasure));

        assert!(editor.file.elements.is_empty());
    }

    #[test]
    fn a_new_label_whose_container_vanished_becomes_a_new_free_text() {
        let mut editor = Editor::new(sample::file(vec![]), TestEnv::seeded(1));
        editor.text_editing = Some(stub_editing(None, Some("gone")));

        assert!(commit_text(
            &mut editor,
            "label text",
            &mut sample::CharWidthMeasure
        ));

        assert_eq!(editor.file.elements.len(), 1);
        let v = editor.file.elements[0].to_value();
        assert_eq!(v["text"], json!("label text"));
        assert_eq!(v["containerId"], json!(null));
    }
}
