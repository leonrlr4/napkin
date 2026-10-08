//! Container labels: wrapping a bound text to its container, growing the container when the
//! text no longer fits, and placing the text inside it. Ported from
//! `packages/element/src/textElement.ts` (`redrawTextBoundingBox`, `handleBindTextResize`,
//! `computeBoundTextPosition`, `getContainerCoords`, `computeContainerDimensionForBoundText`,
//! `getBoundTextMaxWidth`, `getBoundTextMaxHeight`), `textMeasurements.ts`
//! (`getApproxMinLineWidth`, `getApproxMinLineHeight`, `getMinTextElementWidth`) and
//! `sizeHelpers.ts` (`getPositionAfterHeightChange`) at the pinned commit.
//!
//! Only rectangle, diamond and ellipse containers are handled. An arrow container's label
//! follows `LinearElementEditor` rules instead, a `Raw` container is never touched, and the
//! sticky note branches and `originalContainerCache` do not exist here.
//!
//! `getApproxMinLineWidth` reads a per-character width cache that Excalidraw fills as a side
//! effect of canvas measuring. napkin keeps no such cache, so the function always takes the
//! branch that measures `DUMMY_TEXT` one character per line.

use crate::element::{Element, TextElement};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::rotate_point;
use crate::new_element::bump_version;
use crate::text::{TextMeasure, line_height, measure_text, normalize_text};
use crate::text_wrap::wrap_text;
use crate::transform::HandleKind;

/// `BOUND_TEXT_PADDING` (`packages/common/src/constants.ts`).
pub const BOUND_TEXT_PADDING: f64 = 5.0;

/// `DUMMY_TEXT`.
const DUMMY_TEXT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub(crate) fn is_container(element: &Element) -> bool {
    matches!(
        element,
        Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
    )
}

/// `getBoundTextMaxWidth`/`getBoundTextMaxHeight` for a rectangle, diamond or ellipse
/// container; `None` for any other element (an arrow container's label has no such limit
/// here, see [`bound_text_position`]'s doc comment).
pub fn bound_text_max_size(container: &Element) -> Option<[f64; 2]> {
    if !is_container(container) {
        return None;
    }
    let placement = container.placement()?;
    let inscribed = |extent: f64| match container {
        Element::Diamond(_) => rough::js::math_round(extent / 2.0) - BOUND_TEXT_PADDING * 2.0,
        Element::Ellipse(_) => {
            rough::js::math_round(extent / 2.0 * std::f64::consts::SQRT_2)
                - BOUND_TEXT_PADDING * 2.0
        }
        _ => extent - BOUND_TEXT_PADDING * 2.0,
    };
    Some([inscribed(placement.width), inscribed(placement.height)])
}

/// `computeBoundTextPosition` for a rectangle, diamond or ellipse container; `None` otherwise
/// (an arrow container's label follows `LinearElementEditor.getBoundTextElementPosition`
/// instead, out of scope here).
pub fn bound_text_position(container: &Element, text: &TextElement) -> Option<[f64; 2]> {
    let placement = container.placement()?;
    let [max_width, max_height] = bound_text_max_size(container)?;

    // `getContainerCoords`.
    let (offset_x, offset_y) = match container {
        Element::Diamond(_) => (placement.width / 4.0, placement.height / 4.0),
        Element::Ellipse(_) => {
            let k = 1.0 - std::f64::consts::FRAC_1_SQRT_2;
            (placement.width / 2.0 * k, placement.height / 2.0 * k)
        }
        _ => (0.0, 0.0),
    };
    let container_x = placement.x + BOUND_TEXT_PADDING + offset_x;
    let container_y = placement.y + BOUND_TEXT_PADDING + offset_y;

    let y = match text.vertical_align.as_str() {
        "top" => container_y,
        "bottom" => container_y + (max_height - text.base.height),
        _ => container_y + (max_height / 2.0 - text.base.height / 2.0),
    };
    let x = match text.text_align.as_str() {
        "left" => container_x,
        "right" => container_x + (max_width - text.base.width),
        _ => container_x + (max_width / 2.0 - text.base.width / 2.0),
    };

    if placement.angle != 0.0 {
        let content_center = [
            container_x + max_width / 2.0,
            container_y + max_height / 2.0,
        ];
        let text_center = [x + text.base.width / 2.0, y + text.base.height / 2.0];
        let [rx, ry] = rotate_point(text_center, content_center, placement.angle);
        return Some([rx - text.base.width / 2.0, ry - text.base.height / 2.0]);
    }
    Some([x, y])
}

/// `computeContainerDimensionForBoundText` for napkin's three container kinds (`kind` is the
/// element type string).
pub fn container_dimension_for_bound_text(dimension: f64, kind: &str) -> f64 {
    let dimension = dimension.ceil();
    let padding = BOUND_TEXT_PADDING * 2.0;
    match kind {
        "ellipse" => rough::js::math_round((dimension + padding) / std::f64::consts::SQRT_2 * 2.0),
        "arrow" => dimension + padding * 8.0,
        "diamond" => 2.0 * (dimension + padding),
        _ => dimension + padding,
    }
}

/// `getApproxMinLineWidth` through its measure-`DUMMY_TEXT` fallback (always taken here, see
/// this module's doc comment) and `getApproxMinLineHeight`: `[width, height]`.
pub fn approx_min_container_size(
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> [f64; 2] {
    let dummy: Vec<String> = DUMMY_TEXT.chars().map(String::from).collect();
    let [width, _] = measure_text(
        &dummy.join("\n"),
        font_family,
        font_size,
        line_height,
        measure,
    );
    [
        width + BOUND_TEXT_PADDING * 2.0,
        font_size * line_height + BOUND_TEXT_PADDING * 2.0,
    ]
}

/// `getMinTextElementWidth`.
pub fn min_text_element_width(
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> f64 {
    measure_text("", font_family, font_size, line_height, measure)[0] + BOUND_TEXT_PADDING * 2.0
}

/// `getPositionAfterHeightChange`: the new `[x, y]` of an element of `height` and `angle`
/// whose height becomes `next_height`, keeping the `anchor` edge (or the center) fixed.
fn position_after_height_change(
    [x, y]: [f64; 2],
    height: f64,
    angle: f64,
    next_height: f64,
    anchor: Anchor,
) -> [f64; 2] {
    let delta = (height - next_height) / 2.0;
    let (sin, cos) = (angle.sin(), angle.cos());
    match anchor {
        Anchor::Center => [x, y + delta],
        Anchor::Bottom => [x - delta * sin, y + delta * (1.0 + cos)],
        Anchor::Top => [x + delta * sin, y + delta * (1.0 - cos)],
    }
}

#[derive(Clone, Copy)]
enum Anchor {
    Top,
    Bottom,
    Center,
}

/// The position of `container`'s live (typed, not deleted) text label.
pub(crate) fn live_bound_text(file: &SceneFile, container: usize) -> Option<usize> {
    let (text_id, _) = file.elements[container]
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")?;
    file.elements
        .iter()
        .position(|e| matches!(e, Element::Text(_)) && !e.is_deleted() && e.id() == Some(text_id))
}

pub(crate) fn text_line_height(text: &TextElement) -> f64 {
    text.line_height
        .unwrap_or_else(|| line_height(text.font_family))
}

/// Stores `next` at `position`, bumping the version when it differs from what was there.
/// Returns whether it did.
fn store(file: &mut SceneFile, position: usize, next: Element, env: &mut impl Env) -> bool {
    if file.elements[position] == next {
        return false;
    }
    file.elements[position] = next;
    bump_version(&mut file.elements[position], env);
    true
}

/// `redrawTextBoundingBox(text, container)`: rewraps `originalText` (to the container's max
/// width, or to the text's own width when it is a standalone text with `autoResize: false`),
/// remeasures, grows a rectangle/diamond/ellipse container that is now too small, and
/// recenters a bound text. `container` is the container's position, if any; an arrow or
/// `Raw` container leaves everything untouched. Bumps every element that changed. Returns
/// whether anything changed.
pub fn redraw_text_bounding_box(
    file: &mut SceneFile,
    text: usize,
    container: Option<usize>,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool {
    let Element::Text(start) = &file.elements[text] else {
        return false;
    };
    let mut label = start.clone();
    let mut container_element = match container {
        Some(position) => {
            if !is_container(&file.elements[position]) {
                return false;
            }
            Some(file.elements[position].clone())
        }
        None => None,
    };

    let auto_resize = label.auto_resize.unwrap_or(true);
    let original = label
        .original_text
        .clone()
        .unwrap_or_else(|| label.text.clone());
    let max_size = container_element.as_ref().and_then(bound_text_max_size);
    if container.is_some() || !auto_resize {
        let max_width = max_size.map_or(label.base.width, |[w, _]| w);
        label.text = wrap_text(
            &original,
            label.font_family,
            label.font_size,
            max_width,
            measure,
        );
    }

    let [width, height] = measure_text(
        &normalize_text(&label.text),
        label.font_family,
        label.font_size,
        text_line_height(&label),
        measure,
    );
    if auto_resize {
        label.base.width = width;
    }
    label.base.height = height;

    if let (Some(element), Some([max_width, max_height])) = (&mut container_element, max_size) {
        let kind = element.kind().to_owned();
        let base = element.base_mut().expect("typed container");
        if height > max_height {
            base.height = container_dimension_for_bound_text(height, &kind);
        }
        if width > max_width {
            base.width = container_dimension_for_bound_text(width, &kind);
        }
        label.base.angle = base.angle;
        if let Some([x, y]) = bound_text_position(element, &label) {
            label.base.x = x;
            label.base.y = y;
        }
    }

    let mut changed = false;
    if let (Some(position), Some(element)) = (container, container_element) {
        changed |= store(file, position, element, env);
    }
    changed |= store(file, text, Element::Text(label), env);
    changed
}

/// `handleBindTextResize(container, handle, keepAspectRatio, fromCenter, flipByY)` after the
/// container at `container` was resized: rewraps its live bound text unless the handle is
/// `N`/`S` without `keep_aspect_ratio`, grows the container (anchored per the JS) when the
/// text no longer fits, and recenters the text. `handle` is `None` for a resize that came
/// from no handle (a batch `update`), which rewraps like a side handle. A no-op for a
/// container without a live typed text label. Returns whether anything changed.
#[expect(clippy::too_many_arguments, reason = "mirrors handleBindTextResize")]
pub fn handle_bind_text_resize(
    file: &mut SceneFile,
    container: usize,
    handle: Option<HandleKind>,
    keep_aspect_ratio: bool,
    from_center: bool,
    flip_by_y: bool,
    measure: &mut dyn TextMeasure,
    env: &mut impl Env,
) -> bool {
    if !is_container(&file.elements[container]) {
        return false;
    }
    let Some(text_position) = live_bound_text(file, container) else {
        return false;
    };
    let Element::Text(start) = &file.elements[text_position] else {
        return false;
    };
    if start.text.is_empty() {
        return false;
    }
    let mut label = start.clone();
    let mut element = file.elements[container].clone();
    let Some([max_width, max_height]) = bound_text_max_size(&element) else {
        return false;
    };

    let (mut text, mut next_width, mut next_height) =
        (label.text.clone(), label.base.width, label.base.height);
    if keep_aspect_ratio || !matches!(handle, Some(HandleKind::N | HandleKind::S)) {
        let original = label
            .original_text
            .clone()
            .unwrap_or_else(|| label.text.clone());
        text = wrap_text(
            &original,
            label.font_family,
            label.font_size,
            max_width,
            measure,
        );
        [next_width, next_height] = measure_text(
            &normalize_text(&text),
            label.font_family,
            label.font_size,
            text_line_height(&label),
            measure,
        );
    }

    if next_height > max_height {
        let kind = element.kind().to_owned();
        let container_height = container_dimension_for_bound_text(next_height, &kind);
        // Crossing the opposite edge swaps the anchor for text-driven growth.
        let from_top = matches!(
            handle,
            Some(HandleKind::N | HandleKind::Ne | HandleKind::Nw)
        );
        let anchor = if from_center {
            Anchor::Center
        } else if from_top != flip_by_y {
            Anchor::Bottom
        } else {
            Anchor::Top
        };
        let base = element.base_mut().expect("typed container");
        let [x, y] = position_after_height_change(
            [base.x, base.y],
            base.height,
            base.angle,
            container_height,
            anchor,
        );
        base.height = container_height;
        base.x = x;
        base.y = y;
    }

    label.text = text;
    label.base.width = next_width;
    label.base.height = next_height;
    if let Some([x, y]) = bound_text_position(&element, &label) {
        label.base.x = x;
        label.base.y = y;
    }

    let mut changed = store(file, container, element, env);
    changed |= store(file, text_position, Element::Text(label), env);
    changed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample::{self, CharWidthMeasure};

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(0);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    /// A container at the origin with `kind`'s `[w, h]` and a centered label `"hello world"`
    /// (unwrapped, 132 wide, one line).
    fn labeled(kind: &str, size: [f64; 2]) -> SceneFile {
        sample::file(vec![
            sample::with(
                sample::generic(kind, "c", [0.0, 0.0, size[0], size[1]]),
                json!({"boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::with(
                sample::text("t", [0.0, 0.0, 132.0, 25.0], "hello world", Some("c")),
                json!({"textAlign": "center", "verticalAlign": "middle"}),
            ),
        ])
    }

    fn rect(file: &SceneFile, index: usize) -> [f64; 4] {
        let p = file.elements[index].placement().expect("placement");
        [p.x, p.y, p.width, p.height]
    }

    fn text_of(file: &SceneFile) -> String {
        file.elements[1].to_value()["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn redraw_wraps_grows_the_container_and_recenters() {
        let mut file = labeled("rectangle", [100.0, 50.0]);
        assert!(redraw_text_bounding_box(
            &mut file,
            1,
            Some(0),
            &mut CharWidthMeasure,
            &mut FixedEnv
        ));
        assert_eq!(text_of(&file), "hello\nworld");
        // Two lines are 50 tall; max height was 50 - 10 = 40, so the container grows to 50 + 10.
        assert_eq!(rect(&file, 0), [0.0, 0.0, 100.0, 60.0]);
        assert_eq!(rect(&file, 1), [20.0, 5.0, 60.0, 50.0]);
        assert_eq!(
            file.elements[1].to_value()["originalText"],
            json!("hello world")
        );
    }

    #[test]
    fn an_ellipse_grows_by_its_own_formula() {
        let mut file = labeled("ellipse", [400.0, 40.0]);
        redraw_text_bounding_box(&mut file, 1, Some(0), &mut CharWidthMeasure, &mut FixedEnv);
        assert_eq!(text_of(&file), "hello world");
        // round((25 + 10) / sqrt(2) * 2) = 49.
        assert_eq!(rect(&file, 0)[3], 49.0);
    }

    #[test]
    fn a_fitting_label_changes_nothing_but_its_position() {
        let mut file = labeled("rectangle", [300.0, 100.0]);
        redraw_text_bounding_box(&mut file, 1, Some(0), &mut CharWidthMeasure, &mut FixedEnv);
        assert_eq!(rect(&file, 0), [0.0, 0.0, 300.0, 100.0]);
        assert_eq!(rect(&file, 1), [84.0, 37.5, 132.0, 25.0]);
    }

    #[test]
    fn side_resize_rewraps_and_grows_downward() {
        let mut file = labeled("rectangle", [100.0, 50.0]);
        assert!(handle_bind_text_resize(
            &mut file,
            0,
            Some(HandleKind::E),
            false,
            false,
            false,
            &mut CharWidthMeasure,
            &mut FixedEnv,
        ));
        assert_eq!(text_of(&file), "hello\nworld");
        assert_eq!(rect(&file, 0), [0.0, 0.0, 100.0, 60.0]);
    }

    #[test]
    fn top_resize_grows_upward_and_keeps_the_text_unwrapped() {
        let mut file = labeled("rectangle", [300.0, 20.0]);
        handle_bind_text_resize(
            &mut file,
            0,
            Some(HandleKind::N),
            false,
            false,
            false,
            &mut CharWidthMeasure,
            &mut FixedEnv,
        );
        assert_eq!(text_of(&file), "hello world");
        // 25 > 20 - 10: grows to 35, anchored at the bottom edge.
        assert_eq!(rect(&file, 0), [0.0, -15.0, 300.0, 35.0]);
    }

    #[test]
    fn standalone_fixed_width_text_wraps_to_its_own_width() {
        let mut file = sample::file(vec![sample::with(
            sample::text("t", [0.0, 0.0, 70.0, 25.0], "hello world", None),
            json!({"autoResize": false}),
        )]);
        redraw_text_bounding_box(&mut file, 0, None, &mut CharWidthMeasure, &mut FixedEnv);
        let v = file.elements[0].to_value();
        assert_eq!(v["text"], json!("hello\nworld"));
        assert_eq!(v["width"], json!(70.0));
        assert_eq!(v["height"], json!(50.0));
    }

    #[test]
    fn min_sizes_measure_the_widest_dummy_character() {
        // Every character is 12 wide here: 12 + 10, and one line 25 + 10.
        assert_eq!(
            approx_min_container_size(5.0, 20.0, 1.25, &mut CharWidthMeasure),
            [22.0, 35.0]
        );
        // measureText("") measures " ": 12 + 10.
        assert_eq!(
            min_text_element_width(5.0, 20.0, 1.25, &mut CharWidthMeasure),
            22.0
        );
    }
}
