//! The property panel's visible sections and values, and applying a picked value, ported from
//! `packages/excalidraw/components/Actions.tsx`'s `SelectedShapeActions`,
//! `packages/excalidraw/components/shapeActionPredicates.ts`'s `getShapeActionPredicates`,
//! `packages/element/src/comparisons.ts` (`hasBackground`, `hasFillStyle`, `hasStrokeColor`,
//! `hasStrokeWidth`, `hasStrokeStyle`, `hasRoughness`, `canChangeRoundness`, `toolIsArrow`,
//! `canHaveArrowheads`), `packages/element/src/showSelectedShapeActions.ts`,
//! `packages/element/src/selection.ts`'s `getTargetElements`/`getSelectedElements`,
//! `packages/element/src/typeChecks.ts`'s `isUsingAdaptiveRadius`, and
//! `packages/excalidraw/actions/actionProperties.tsx`'s `changeProperty`, `getFormValue` and
//! the individual `actionChange*` functions, all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! napkin has none of Excalidraw's sticky notes, bucket fill, images, frames or elbow arrows,
//! so this only ports the parts of the JS that apply to napkin's seven typed element kinds
//! (rectangle, diamond, ellipse, line, arrow, text, freedraw); a `Raw` element (spec §5.2)
//! never shows in a section's value and is never touched by [`set_property`].
//!
//! A few JS behaviours are easy to miss because they read as filtered but are not:
//! - `changeProperty`'s callback filters by type for `StrokeColor` (`hasStrokeColor`, though
//!   that predicate happens to cover all seven napkin kinds already), `FillStyle`
//!   (`hasFillStyle`), `ArrowType` (`isArrowElement`), the two arrowheads (`isLinearElement`,
//!   i.e. line *or* arrow, not just arrow) and `FontFamily`/`FontSize` (`isTextElement`). Every
//!   other property (`BackgroundColor`, `StrokeWidth`, `StrokeStyle`, `Roughness`, `Edges`,
//!   `Opacity`) writes to *every* selected typed element, whatever its kind: e.g. picking a
//!   background color while an arrow is selected sets the arrow's unused `backgroundColor`
//!   field too. `Edges` additionally skips an elbow arrow (`isElbowArrow`); napkin never
//!   creates one, but a loaded file can hold one.
//! - `includeBoundTextElement` (a selected container's label joins the elements a property is
//!   applied to) is `true` only for `StrokeColor`, `Opacity`, `FontFamily` and `FontSize`, not
//!   for the rest. This module reads a container's label unconditionally for section
//!   visibility and values (`getTargetElements` always passes `true`), matching that value
//!   independently of which properties propagate to it on write.
//! - `actionChangeSloppiness` also re-rolls the element's `seed`, so two shapes with the same
//!   roughness value still look different after the change.
//! - [`Section::FillStyle`]'s visibility (`predicates.fill`) is not just `hasFillStyle`: it
//!   also requires a non-transparent background, on the tool's current item style or on a
//!   target element (`!isTransparent(...)`, see [`fill_style_visible`]). The panel *value*
//!   itself (`getFormValue`'s own predicate) has no such gate.
//!
//! `canChangeRoundness` (backing [`Section::Edges`]) lists rectangle, diamond, line, iframe,
//! embeddable, stickynote and image, but not ellipse: an ellipse has no sharp/round corners to
//! toggle. Of napkin's kinds that leaves rectangle, diamond and line.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Map;

use crate::bound_text;
use crate::color::is_transparent;
use crate::element::{Element, ElementBase, Roundness};
use crate::env::{Env, random_integer};
use crate::file::SceneFile;
use crate::json::Slot;
use crate::new_element::bump_version;
use crate::text::{self, TextMeasure};

use super::{ArrowType, EdgeStyle, Editor, ItemStyle, StrokeWidth, TextEditing, Tool, clone_scene};

/// One property-panel section (`SelectedShapeActions` in
/// `packages/excalidraw/components/Actions.tsx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    StrokeColor,
    BackgroundColor,
    FillStyle,
    StrokeWidth,
    StrokeStyle,
    Roughness,
    Edges,
    ArrowType,
    Arrowheads,
    FontFamily,
    FontSize,
    Opacity,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Property {
    StrokeColor(String),
    BackgroundColor(String),
    FillStyle(String),
    StrokeWidth(StrokeWidth),
    StrokeStyle(String),
    Roughness(f64),
    Edges(EdgeStyle),
    ArrowType(ArrowType),
    StartArrowhead(Option<String>),
    EndArrowhead(Option<String>),
    FontFamily(f64),
    FontSize(f64),
    Opacity(f64),
}

/// What the panel shows: which sections, and each section's current value; `None` means the
/// selected elements disagree (`getFormValue` returning its default for mixed values).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelState {
    pub sections: Vec<Section>,
    pub stroke_color: Option<String>,
    pub background_color: Option<String>,
    pub fill_style: Option<String>,
    pub stroke_width: Option<StrokeWidth>,
    pub stroke_style: Option<String>,
    pub roughness: Option<f64>,
    pub edges: Option<EdgeStyle>,
    pub arrow_type: Option<ArrowType>,
    pub start_arrowhead: Option<Option<String>>,
    pub end_arrowhead: Option<Option<String>>,
    pub font_family: Option<f64>,
    pub font_size: Option<f64>,
    pub opacity: Option<f64>,
}

// --- section/value type predicates (`comparisons.ts`, narrowed to napkin's typed kinds) ---

/// `hasStrokeColor`: every napkin kind, coincidentally (`hasStrokeColor` in JS also lists
/// stickynote and embeddable, which napkin has no typed struct for).
fn has_stroke_color(kind: &str) -> bool {
    is_typed_kind(kind)
}

fn has_background(kind: &str) -> bool {
    matches!(
        kind,
        "rectangle" | "diamond" | "ellipse" | "line" | "freedraw"
    )
}

/// `hasFillStyle`: `hasBackground(type) && type !== "stickynote"`; napkin has no sticky notes.
/// This alone backs the panel *value* (`getFormValue`'s own predicate, ungated by
/// transparency); [`fill_style_visible`] additionally gates *section visibility*.
fn has_fill_style(kind: &str) -> bool {
    has_background(kind)
}

/// `predicates.fill`: `hasFillStyle(tool) && !isTransparent(currentItemBackgroundColor)`, or
/// some target element with `hasFillStyle(type) && !isTransparent(backgroundColor)`. napkin has
/// no bucket-fill tool, so that half of the JS predicate (which always shows fill style) does
/// not apply here. A transparent fill has nothing to show a hachure/cross-hatch/solid pattern
/// on, so the section only appears once there is a color to fill with.
fn fill_style_visible(
    tool_kind: Option<&str>,
    target: &[&Element],
    style_background: &str,
) -> bool {
    if tool_kind.is_some_and(has_fill_style) && !is_transparent(style_background) {
        return true;
    }
    target.iter().any(|e| {
        has_fill_style(e.kind())
            && e.base()
                .is_some_and(|b| !is_transparent(&b.background_color))
    })
}

fn has_stroke_width(kind: &str) -> bool {
    matches!(
        kind,
        "rectangle" | "diamond" | "ellipse" | "freedraw" | "arrow" | "line"
    )
}

fn has_stroke_style(kind: &str) -> bool {
    matches!(kind, "rectangle" | "diamond" | "ellipse" | "arrow" | "line")
}

/// `hasRoughness`: `hasStrokeStyle(type) || type === "stickynote"`; napkin has no sticky notes.
fn has_roughness(kind: &str) -> bool {
    has_stroke_style(kind)
}

/// `canChangeRoundness`, narrowed to napkin's kinds: rectangle, diamond and line, not ellipse
/// (see this module's doc comment).
fn can_change_roundness(kind: &str) -> bool {
    matches!(kind, "rectangle" | "diamond" | "line")
}

/// `toolIsArrow`/`canHaveArrowheads`: both are `type === "arrow"` in JS.
fn is_arrow(kind: &str) -> bool {
    kind == "arrow"
}

fn is_text(kind: &str) -> bool {
    kind == "text"
}

/// The seven kinds with property panel entries; `false` for an image and for a `Raw`
/// element's kind string (a frame, ...).
fn is_typed_kind(kind: &str) -> bool {
    matches!(
        kind,
        "rectangle" | "diamond" | "ellipse" | "line" | "arrow" | "freedraw" | "text"
    )
}

/// The element kind [`Tool`] creates, for `forToolOrSelection`'s "or the active tool" half;
/// `None` for the tools that create nothing (`Selection`, `Hand`, `Eraser`).
fn tool_kind(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Rectangle => Some("rectangle"),
        Tool::Diamond => Some("diamond"),
        Tool::Ellipse => Some("ellipse"),
        Tool::Arrow => Some("arrow"),
        Tool::Line => Some("line"),
        Tool::Freedraw => Some("freedraw"),
        Tool::Text => Some("text"),
        Tool::Selection | Tool::Hand | Tool::Eraser => None,
    }
}

/// Selected elements, plus (when `include_bound_text`) each selected element's bound text
/// label, as in `getSelectedElements(..., {includeBoundTextElement})`. Ascending file order.
fn selection_positions(
    file: &SceneFile,
    selection: &crate::selection::Selection,
    include_bound_text: bool,
) -> Vec<usize> {
    let base = selection.positions(file);
    if !include_bound_text {
        return base;
    }
    let mut positions: BTreeSet<usize> = base.iter().copied().collect();
    for &position in &base {
        let element = &file.elements[position];
        if let Some((text_id, _)) = element
            .bound_elements()
            .into_iter()
            .find(|&(_, kind)| kind == "text")
            && let Some(text_position) = file
                .elements
                .iter()
                .position(|e| !e.is_deleted() && e.id() == Some(text_id))
        {
            positions.insert(text_position);
        }
    }
    positions.into_iter().collect()
}

/// `reduceToCommonValue`: `None` for an empty collection, for any element whose own value is
/// `None` (an unmatched stroke width, say), or once two elements disagree; otherwise the one
/// shared value.
fn common<T: Clone + PartialEq>(values: Vec<Option<T>>) -> Option<T> {
    if values.is_empty() {
        return None;
    }
    let mut result: Option<T> = None;
    for value in values {
        let value = value?;
        match &result {
            None => result = Some(value),
            Some(r) if *r == value => {}
            Some(_) => return None,
        }
    }
    result
}

pub(super) fn panel<E: Env>(editor: &Editor<E>) -> PanelState {
    let tool_kind = tool_kind(editor.tool);
    let has_selection = !editor.selection.is_empty();
    if !has_selection && tool_kind.is_none() {
        return PanelState::default();
    }

    let positions = selection_positions(&editor.file, &editor.selection, true);
    let target: Vec<&Element> = positions
        .iter()
        .map(|&p| &editor.file.elements[p])
        .collect();

    let shows = |predicate: fn(&str) -> bool| {
        tool_kind.is_some_and(predicate) || target.iter().any(|e| predicate(e.kind()))
    };

    let mut sections = Vec::new();
    if shows(has_stroke_color) {
        sections.push(Section::StrokeColor);
    }
    if shows(has_background) {
        sections.push(Section::BackgroundColor);
    }
    if fill_style_visible(tool_kind, &target, &editor.style.background_color) {
        sections.push(Section::FillStyle);
    }
    if shows(has_stroke_width) {
        sections.push(Section::StrokeWidth);
    }
    if shows(has_stroke_style) {
        sections.push(Section::StrokeStyle);
    }
    if shows(has_roughness) {
        sections.push(Section::Roughness);
    }
    if shows(can_change_roundness) {
        sections.push(Section::Edges);
    }
    if shows(is_arrow) {
        sections.push(Section::ArrowType);
    }
    if shows(is_arrow) {
        sections.push(Section::Arrowheads);
    }
    if shows(is_text) {
        sections.push(Section::FontFamily);
        sections.push(Section::FontSize);
    }
    // `activeToolType !== "autoshape" || hasSelection`: napkin has no autoshape tool, so this
    // is unconditional once the panel itself is visible.
    sections.push(Section::Opacity);

    let style = &editor.style;

    /// `getFormValue`: with nothing selected, the current item style; otherwise the common
    /// value among target elements matching `predicate`, or `None` for no match or a mismatch.
    fn pick<T: Clone + PartialEq>(
        has_selection: bool,
        target: &[&Element],
        predicate: fn(&str) -> bool,
        get: fn(&Element) -> Option<T>,
        style_value: T,
    ) -> Option<T> {
        if !has_selection {
            return Some(style_value);
        }
        common(
            target
                .iter()
                .filter(|e| predicate(e.kind()))
                .map(|e| get(e))
                .collect(),
        )
    }

    PanelState {
        sections,
        stroke_color: pick(
            has_selection,
            &target,
            has_stroke_color,
            stroke_color_value,
            style.stroke_color.clone(),
        ),
        background_color: pick(
            has_selection,
            &target,
            has_background,
            background_color_value,
            style.background_color.clone(),
        ),
        fill_style: pick(
            has_selection,
            &target,
            has_fill_style,
            fill_style_value,
            style.fill_style.clone(),
        ),
        stroke_width: pick(
            has_selection,
            &target,
            has_stroke_width,
            stroke_width_value,
            style.stroke_width,
        ),
        stroke_style: pick(
            has_selection,
            &target,
            has_stroke_style,
            stroke_style_value,
            style.stroke_style.clone(),
        ),
        roughness: pick(
            has_selection,
            &target,
            has_roughness,
            roughness_value,
            style.roughness,
        ),
        edges: pick(
            has_selection,
            &target,
            can_change_roundness,
            edges_value,
            style.edges,
        ),
        arrow_type: pick(
            has_selection,
            &target,
            is_arrow,
            arrow_type_value,
            style.arrow_type,
        ),
        start_arrowhead: pick(
            has_selection,
            &target,
            is_arrow,
            start_arrowhead_value,
            style.start_arrowhead.clone(),
        ),
        end_arrowhead: pick(
            has_selection,
            &target,
            is_arrow,
            end_arrowhead_value,
            style.end_arrowhead.clone(),
        ),
        font_family: pick(
            has_selection,
            &target,
            is_text,
            font_family_value,
            style.font_family,
        ),
        font_size: pick(
            has_selection,
            &target,
            is_text,
            font_size_value,
            style.font_size,
        ),
        opacity: pick(
            has_selection,
            &target,
            is_typed_kind,
            opacity_value,
            style.opacity,
        ),
    }
}

fn stroke_color_value(e: &Element) -> Option<String> {
    e.base().map(|b| b.stroke_color.clone())
}

fn background_color_value(e: &Element) -> Option<String> {
    e.base().map(|b| b.background_color.clone())
}

fn fill_style_value(e: &Element) -> Option<String> {
    e.base().map(|b| b.fill_style.clone())
}

/// `getStrokeWidthKeyForElement`.
fn stroke_width_value(e: &Element) -> Option<StrokeWidth> {
    let base = e.base()?;
    StrokeWidth::from_value(base.stroke_width, e.kind() == "freedraw")
}

fn stroke_style_value(e: &Element) -> Option<String> {
    e.base().map(|b| b.stroke_style.clone())
}

fn roughness_value(e: &Element) -> Option<f64> {
    e.base().map(|b| b.roughness)
}

/// `element.roundness ? "round" : "sharp"`.
fn edges_value(e: &Element) -> Option<EdgeStyle> {
    let base = e.base()?;
    Some(if base.roundness.value().is_some() {
        EdgeStyle::Round
    } else {
        EdgeStyle::Sharp
    })
}

/// `element.elbowed ? elbow : element.roundness ? round : sharp`; napkin's own `ArrowType`
/// only has `Sharp`/`Round` (spec §1.2: no elbow arrows), so an elbow arrow has no value to
/// report here and counts as a mismatch instead.
fn arrow_type_value(e: &Element) -> Option<ArrowType> {
    let Element::Arrow(arrow) = e else {
        return None;
    };
    if arrow.elbowed == Some(true) {
        return None;
    }
    Some(if arrow.base.roundness.value().is_some() {
        ArrowType::Round
    } else {
        ArrowType::Sharp
    })
}

fn start_arrowhead_value(e: &Element) -> Option<Option<String>> {
    let Element::Arrow(arrow) = e else {
        return None;
    };
    Some(arrow.start_arrowhead.value().cloned())
}

fn end_arrowhead_value(e: &Element) -> Option<Option<String>> {
    let Element::Arrow(arrow) = e else {
        return None;
    };
    Some(arrow.end_arrowhead.value().cloned())
}

fn font_family_value(e: &Element) -> Option<f64> {
    let Element::Text(t) = e else {
        return None;
    };
    Some(t.font_family)
}

fn font_size_value(e: &Element) -> Option<f64> {
    let Element::Text(t) = e else {
        return None;
    };
    Some(t.font_size)
}

fn opacity_value(e: &Element) -> Option<f64> {
    e.base().map(|b| b.opacity)
}

pub(super) fn set_property<E: Env>(
    editor: &mut Editor<E>,
    property: Property,
    measure: &mut dyn TextMeasure,
) -> bool {
    update_style(&mut editor.style, &property);
    apply_to_text_editing(&mut editor.text_editing, &property);

    if editor.selection.is_empty() {
        return false;
    }

    // `includeBoundTextElement` in the JS action's own `changeProperty` call (see this
    // module's doc comment): only these four propagate to a selected container's label.
    let include_bound_text = matches!(
        property,
        Property::StrokeColor(_)
            | Property::Opacity(_)
            | Property::FontFamily(_)
            | Property::FontSize(_)
    );
    let targets = selection_positions(&editor.file, &editor.selection, include_bound_text);

    let before = Arc::clone(&editor.file);
    let selection_before = editor.selection.clone();
    let file = clone_scene(&mut editor.file, &mut editor.scene_clones);

    for position in targets {
        apply_property(file, position, &property, measure, &mut editor.env);
    }

    editor.finish_edit(&before, &selection_before)
}

/// The four properties `includeBoundTextElement` already singles out above (stroke color,
/// opacity, font family, font size) also apply live to the text currently being edited, if
/// any: `editingTextElement` is itself part of what `changeProperty`'s `getSelectedElements`
/// call targets in the JS, so a panel change while the wysiwyg editor is open reaches it too,
/// not just a `commit_text` afterwards it has no other way to pick up. Selection is always
/// empty while `text_editing` is `Some` (`text::pointer_down`/`double_click` both clear it), so
/// this runs whether or not the function goes on to touch any selected element.
fn apply_to_text_editing(text_editing: &mut Option<TextEditing>, property: &Property) {
    let Some(editing) = text_editing else {
        return;
    };
    match property {
        Property::StrokeColor(v) => editing.stroke_color = v.clone(),
        Property::Opacity(v) => editing.opacity = *v,
        Property::FontFamily(v) => {
            editing.font_family = *v;
            editing.line_height = text::line_height(*v);
        }
        Property::FontSize(v) => editing.font_size = *v,
        _ => {}
    }
}

fn update_style(style: &mut ItemStyle, property: &Property) {
    match property {
        Property::StrokeColor(v) => style.stroke_color = v.clone(),
        Property::BackgroundColor(v) => style.background_color = v.clone(),
        Property::FillStyle(v) => style.fill_style = v.clone(),
        Property::StrokeWidth(v) => style.stroke_width = *v,
        Property::StrokeStyle(v) => style.stroke_style = v.clone(),
        Property::Roughness(v) => style.roughness = *v,
        Property::Edges(v) => style.edges = *v,
        Property::ArrowType(v) => style.arrow_type = *v,
        Property::StartArrowhead(v) => style.start_arrowhead = v.clone(),
        Property::EndArrowhead(v) => style.end_arrowhead = v.clone(),
        Property::FontFamily(v) => style.font_family = *v,
        Property::FontSize(v) => style.font_size = *v,
        Property::Opacity(v) => style.opacity = *v,
    }
}

fn apply_property(
    file: &mut SceneFile,
    position: usize,
    property: &Property,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) {
    let before = file.elements[position].clone();
    let mut next = before.clone();
    let matched = mutate(&mut next, property, measure, env);
    if !matched {
        return;
    }
    file.elements[position] = next;
    if matches!(property, Property::FontFamily(_) | Property::FontSize(_)) {
        rewrap_label(file, position, measure, env);
    }
    // `redraw_text_bounding_box` bumps the elements it changed itself; this covers the font
    // change it was handed.
    if file.elements[position] != before && file.elements[position].version() == before.version() {
        bump_version(&mut file.elements[position], env);
    }
}

/// `redrawTextBoundingBox(element, container)` after `actionChangeFontFamily`/
/// `actionChangeFontSize` changed the font of the text at `position`: a container's label
/// (or a standalone text with `autoResize: false`) is rewrapped, and the container grows when
/// the new font no longer fits. A standalone `autoResize` text was already remeasured by
/// [`redraw_text`].
fn rewrap_label(
    file: &mut SceneFile,
    position: usize,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) {
    let Element::Text(t) = &file.elements[position] else {
        return;
    };
    match t.container_id.value() {
        Some(container_id) => {
            let container = file
                .elements
                .iter()
                .position(|e| !e.is_deleted() && e.id() == Some(container_id.as_str()));
            if container.is_some() {
                bound_text::redraw_text_bounding_box(file, position, container, measure, env);
            }
        }
        None if t.auto_resize == Some(false) => {
            bound_text::redraw_text_bounding_box(file, position, None, measure, env);
        }
        None => {}
    }
}

/// Runs `f` on a styleable element's `ElementBase`. An image (like `Raw`) has no property
/// panel entry, so a property change leaves it untouched.
fn mutate_base(element: &mut Element, f: impl FnOnce(&mut ElementBase)) -> bool {
    if matches!(element, Element::Image(_)) {
        return false;
    }
    match element.base_mut() {
        Some(base) => {
            f(base);
            true
        }
        None => false,
    }
}

fn proportional_roundness(kind: f64) -> Roundness {
    Roundness {
        kind,
        value: Slot::Missing,
        extra: Map::new(),
    }
}

fn set_arrowhead(next: &mut Element, start: bool, value: &Option<String>) -> bool {
    let slot = value.clone().map_or(Slot::Null, Slot::Value);
    match next {
        Element::Line(l) | Element::Arrow(l) => {
            if start {
                l.start_arrowhead = slot;
            } else {
                l.end_arrowhead = slot;
            }
            true
        }
        _ => false,
    }
}

/// The per-element edit for `property`; `false` means `property` does not apply to this
/// element's kind (it is left untouched, whatever `next` looks like at that point).
fn mutate(
    next: &mut Element,
    property: &Property,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool {
    match property {
        Property::StrokeColor(color) => mutate_base(next, |b| b.stroke_color = color.clone()),
        Property::BackgroundColor(color) => {
            mutate_base(next, |b| b.background_color = color.clone())
        }
        Property::FillStyle(fill) => {
            has_fill_style(next.kind()) && mutate_base(next, |b| b.fill_style = fill.clone())
        }
        Property::StrokeWidth(width) => {
            let freedraw = next.kind() == "freedraw";
            mutate_base(next, |b| b.stroke_width = width.value(freedraw))
        }
        Property::StrokeStyle(style_value) => {
            mutate_base(next, |b| b.stroke_style = style_value.clone())
        }
        Property::Roughness(roughness) => {
            if next.base().is_none() {
                return false;
            }
            // `actionChangeSloppiness` also re-rolls the seed; drawn only once we know there is
            // a typed element to write it to, so a `Raw` element never perturbs the RNG stream.
            let seed = random_integer(env);
            mutate_base(next, |b| {
                b.roughness = *roughness;
                b.seed = seed;
            })
        }
        Property::Edges(edge) => {
            if matches!(next, Element::Arrow(a) if a.elbowed == Some(true)) {
                false
            } else {
                // `isUsingAdaptiveRadius`: only a rectangle does; everything else that can
                // hold a roundness uses the proportional radius.
                let radius_kind = if next.kind() == "rectangle" { 3.0 } else { 2.0 };
                let value =
                    (*edge == EdgeStyle::Round).then(|| proportional_roundness(radius_kind));
                mutate_base(next, |b| {
                    b.roundness = value.map_or(Slot::Null, Slot::Value)
                })
            }
        }
        Property::ArrowType(arrow_type) => {
            if !matches!(next, Element::Arrow(_)) {
                false
            } else {
                let value = (*arrow_type == ArrowType::Round).then(|| proportional_roundness(2.0));
                let matched = mutate_base(next, |b| {
                    b.roundness = value.map_or(Slot::Null, Slot::Value)
                });
                if let Element::Arrow(arrow) = next {
                    // napkin's `ArrowType` never carries "elbow" (spec §1.2: no elbow arrows);
                    // JS always writes `elbowed: value === ARROW_TYPE.elbow`, so this is
                    // always `false` here too.
                    arrow.elbowed = Some(false);
                }
                matched
            }
        }
        Property::StartArrowhead(value) => set_arrowhead(next, true, value),
        Property::EndArrowhead(value) => set_arrowhead(next, false, value),
        Property::FontFamily(family) => {
            if !matches!(next, Element::Text(_)) {
                return false;
            }
            if let Element::Text(t) = next {
                t.font_family = *family;
                t.line_height = Some(text::line_height(*family));
            }
            redraw_text(next, measure);
            true
        }
        Property::FontSize(size) => {
            if !matches!(next, Element::Text(_)) {
                return false;
            }
            if let Element::Text(t) = next {
                t.font_size = *size;
            }
            redraw_text(next, measure);
            true
        }
        Property::Opacity(opacity) => mutate_base(next, |b| b.opacity = *opacity),
    }
}

/// The remeasuring half of `redrawTextBoundingBox` for a font change: remeasures `next`'s
/// (already-mutated) text at its current font, unwrapped. A container's label is then rewrapped
/// and recentred by [`rewrap_label`]; a standalone `autoResize` text keeps its align-appropriate
/// edge fixed horizontally and recentres vertically, as `offsetElementAfterFontResize` does for
/// `actionChangeFontSize`. A standalone text with `autoResize: false` keeps its width and is
/// left to [`rewrap_label`] entirely.
fn redraw_text(next: &mut Element, measure: &mut dyn TextMeasure) {
    let Element::Text(text) = next else { return };
    let bound = text.container_id.value().is_some();
    let auto_resize = text.auto_resize;
    if !bound && auto_resize == Some(false) {
        return;
    }
    let text_align = text.text_align.clone();
    let old_width = text.base.width;
    let old_height = text.base.height;

    let normalized = text::normalize_text(&text.text);
    let line_height = text
        .line_height
        .unwrap_or_else(|| text::line_height(text.font_family));
    let [width, height] = text::measure_text(
        &normalized,
        text.font_family,
        text.font_size,
        line_height,
        measure,
    );
    text.base.width = width;
    text.base.height = height;

    if !bound && auto_resize == Some(true) {
        let dx = match text_align.as_str() {
            "left" => 0.0,
            "right" => old_width - width,
            _ => (old_width - width) / 2.0,
        };
        text.base.x += dx;
        text.base.y += (old_height - height) / 2.0;
    }
}
