//! Port of `generateRoughOptions` and the helpers it depends on:
//! `packages/element/src/shape.ts` (`getDashArrayDashed`, `getDashArrayDotted`,
//! `adjustRoughness`, `generateRoughOptions`), `packages/element/src/utils.ts`
//! (`isPathALoop`), `packages/element/src/comparisons.ts` (`canChangeRoundness`) and
//! `packages/element/src/typeChecks.ts` (`isLinearElement`), all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.

use rough::Options;

use crate::color::{apply_dark_mode_filter, is_transparent};
use crate::element::{Element, ElementBase};
use crate::json::Slot;

/// `ROUGHNESS.cartoonist` (`packages/common/src/constants.ts`).
const ROUGHNESS_CARTOONIST: f64 = 2.0;

/// `LINE_CONFIRM_THRESHOLD` (px, `packages/common/src/constants.ts`).
const LINE_CONFIRM_THRESHOLD: f64 = 8.0;

/// `getDashArrayDashed`.
fn dash_array_dashed(stroke_width: f64) -> Vec<f64> {
    vec![8.0, 8.0 + stroke_width]
}

/// `getDashArrayDotted`. `pub(super)`: `arrowhead.rs`'s `getArrowheadLineOptions` reuses it
/// for dotted arrow caps.
pub(super) fn dash_array_dotted(stroke_width: f64) -> Vec<f64> {
    vec![1.5, 6.0 + stroke_width]
}

/// `applyDarkModeFilter(color, enable)`: JS's version takes the `enable` flag itself and
/// returns `color` unmodified when it is false. `crate::color::apply_dark_mode_filter` has
/// no such parameter (it always filters), so this wrapper does the `enable` check instead.
pub(crate) fn dark(color: &str, dark_mode: bool) -> String {
    if dark_mode {
        apply_dark_mode_filter(color)
    } else {
        color.to_owned()
    }
}

/// `isPathALoop`, with `zoomValue` fixed at 1 (napkin has no zoom-aware caller for this).
pub(crate) fn is_path_a_loop(points: &[[f64; 2]]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let distance = rough::js::hypot(last[0] - first[0], last[1] - first[1]);
    distance <= LINE_CONFIRM_THRESHOLD
}

/// `canChangeRoundness`.
fn can_change_roundness(kind: &str) -> bool {
    matches!(
        kind,
        "rectangle" | "iframe" | "embeddable" | "line" | "diamond" | "stickynote" | "image"
    )
}

/// `isLinearElement`/`isLinearElementType`: `arrow` and `line` only, not `freedraw`.
fn is_linear_element(kind: &str) -> bool {
    matches!(kind, "arrow" | "line")
}

/// `adjustRoughness`.
fn adjust_roughness(base: &ElementBase) -> f64 {
    let roughness = base.roughness;
    let max_size = base.width.max(base.height);
    let min_size = base.width.min(base.height);

    // don't reduce roughness if
    let keep =
        // both sides relatively big
        (min_size >= 20.0 && max_size >= 50.0)
        // is round & both sides above 15px
        || (min_size >= 15.0
            && matches!(base.roundness, Slot::Value(_))
            && can_change_roundness(&base.kind))
        // relatively long linear element
        || (is_linear_element(&base.kind) && max_size >= 50.0);

    if keep {
        return roughness;
    }

    (roughness / if max_size < 10.0 { 3.0 } else { 2.0 }).min(2.5)
}

/// `generateRoughOptions`. Returns `None` when `element` is `Element::Raw` (`element.base()`
/// is `None`); the JS instead throws `Unimplemented type ${element.type}` for an
/// unsupported element type there. napkin only ever calls this with a drawable typed
/// element, so `Raw`, not an unsupported `type` string, is what actually drives this
/// branch.
pub fn generate_rough_options(
    element: &Element,
    continuous_path: bool,
    dark_mode: bool,
) -> Option<Options> {
    let base = element.base()?;

    let mut options = Options {
        seed: Some(base.seed),
        stroke_line_dash: match base.stroke_style.as_str() {
            "dashed" => Some(dash_array_dashed(base.stroke_width)),
            "dotted" => Some(dash_array_dotted(base.stroke_width)),
            _ => None,
        },
        // for non-solid strokes, disable multiStroke because it tends to make
        // dashes/dots overlay each other
        disable_multi_stroke: Some(base.stroke_style != "solid"),
        // for non-solid strokes, increase the width a bit to make it visually
        // similar to solid strokes, because we're also disabling multiStroke
        stroke_width: Some(if base.stroke_style != "solid" {
            base.stroke_width + 0.5
        } else {
            base.stroke_width
        }),
        // when increasing strokeWidth, we must explicitly set fillWeight and
        // hachureGap because if not specified, roughjs uses strokeWidth to
        // calculate them (and we don't want the fills to be modified)
        fill_weight: Some(base.stroke_width / 2.0),
        hachure_gap: Some(base.stroke_width * 4.0),
        roughness: Some(adjust_roughness(base)),
        stroke: Some(dark(&base.stroke_color, dark_mode)),
        preserve_vertices: Some(continuous_path || base.roughness < ROUGHNESS_CARTOONIST),
        ..Options::default()
    };

    match element {
        Element::Rectangle(g) | Element::Diamond(g) | Element::Ellipse(g) => {
            options.fill_style = Some(g.base.fill_style.clone());
            options.fill = if is_transparent(&g.base.background_color) {
                None
            } else {
                Some(dark(&g.base.background_color, dark_mode))
            };
            if matches!(element, Element::Ellipse(_)) {
                options.curve_fitting = Some(1.0);
            }
            Some(options)
        }
        Element::Line(l) => {
            if is_path_a_loop(&l.points) {
                options.fill_style = Some(l.base.fill_style.clone());
                options.fill = (l.base.background_color != "transparent")
                    .then(|| dark(&l.base.background_color, dark_mode));
            }
            Some(options)
        }
        Element::Freedraw(f) => {
            if is_path_a_loop(&f.points) {
                options.fill_style = Some(f.base.fill_style.clone());
                options.fill = (f.base.background_color != "transparent")
                    .then(|| dark(&f.base.background_color, dark_mode));
            }
            Some(options)
        }
        Element::Arrow(_) => Some(options),
        Element::Text(_) | Element::Raw(_) => None,
    }
}
