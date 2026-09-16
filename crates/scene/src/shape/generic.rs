//! Port of the `rectangle`, `diamond` and `ellipse` branches of `_generateElementShape`
//! (`packages/element/src/shape.ts`), `getCornerRadius` (`packages/element/src/utils.ts`)
//! and `getDiamondPoints` (`packages/element/src/bounds.ts`), all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. `iframe`/`embeddable` are excluded: napkin
//! treats those element types as `Element::Raw`, so `modifyIframeLikeForRoughOptions`
//! (which only rewrites those two types) never applies here.

use rough::{Drawable, RoughGenerator};

use crate::element::{Element, GenericElement, Roundness};
use crate::json::Slot;

use super::options::generate_rough_options;

/// `DEFAULT_PROPORTIONAL_RADIUS` (`packages/common/src/constants.ts`).
const DEFAULT_PROPORTIONAL_RADIUS: f64 = 0.25;

/// `DEFAULT_ADAPTIVE_RADIUS` (px, `packages/common/src/constants.ts`).
const DEFAULT_ADAPTIVE_RADIUS: f64 = 32.0;

/// `ROUNDNESS.LEGACY` (`packages/common/src/constants.ts`).
const ROUNDNESS_LEGACY: f64 = 1.0;

/// `ROUNDNESS.PROPORTIONAL_RADIUS`.
const ROUNDNESS_PROPORTIONAL_RADIUS: f64 = 2.0;

/// `ROUNDNESS.ADAPTIVE_RADIUS`.
const ROUNDNESS_ADAPTIVE_RADIUS: f64 = 3.0;

/// `getCornerRadius`. `roundness?.type` and `roundness?.value` treat absent and `null`
/// alike, matched here by `Slot::value()`.
fn corner_radius(x: f64, roundness: &Slot<Roundness>) -> f64 {
    let Some(r) = roundness.value() else {
        return 0.0;
    };
    if r.kind == ROUNDNESS_PROPORTIONAL_RADIUS || r.kind == ROUNDNESS_LEGACY {
        return x * DEFAULT_PROPORTIONAL_RADIUS;
    }
    if r.kind == ROUNDNESS_ADAPTIVE_RADIUS {
        let fixed_radius_size = r.value.value().copied().unwrap_or(DEFAULT_ADAPTIVE_RADIUS);
        let cutoff_size = fixed_radius_size / DEFAULT_PROPORTIONAL_RADIUS;
        return if x <= cutoff_size {
            x * DEFAULT_PROPORTIONAL_RADIUS
        } else {
            fixed_radius_size
        };
    }
    0.0
}

/// `getDiamondPoints`: `topX, topY, rightX, rightY, bottomX, bottomY, leftX, leftY`.
/// `Math.floor` and `f64::floor` agree for every finite input, so no `rough::js` helper
/// is needed here (unlike `Math.round`).
fn diamond_points(width: f64, height: f64) -> [f64; 8] {
    let top_x = (width / 2.0).floor() + 1.0;
    let top_y = 0.0;
    let right_x = width;
    let right_y = (height / 2.0).floor() + 1.0;
    let bottom_x = top_x;
    let bottom_y = height;
    let left_x = 0.0;
    let left_y = right_y;
    [
        top_x, top_y, right_x, right_y, bottom_x, bottom_y, left_x, left_y,
    ]
}

/// `_generateElementShape`'s `"rectangle"` case. `None` means rough rejected the generated
/// corner-radius path (the caller falls back to [`super::ElementShape::Placeholder`]); the
/// upfront geometry bound in `generate_element_shape` keeps this from happening for any
/// baseline or realistic file, but rough's path parser can still reject a path built from
/// finite numbers that are merely large.
pub(super) fn rectangle(
    generator: &RoughGenerator,
    element: &Element,
    g: &GenericElement,
    dark_mode: bool,
) -> Option<Drawable> {
    let base = &g.base;
    let w = base.width;
    let h = base.height;
    if matches!(base.roundness, Slot::Value(_)) {
        let r = corner_radius(w.min(h), &base.roundness);
        let w_r = w - r;
        let h_r = h - r;
        let d = format!(
            "M {r} 0 L {w_r} 0 Q {w} 0, {w} {r} L {w} {h_r} Q {w} {h}, {w_r} {h} \
             L {r} {h} Q 0 {h}, 0 {h_r} L 0 {r} Q 0 0, {r} 0"
        );
        let options = generate_rough_options(element, true, dark_mode).expect("rectangle draws");
        generator.path(&d, &options).ok()
    } else {
        let options = generate_rough_options(element, false, dark_mode).expect("rectangle draws");
        Some(generator.rectangle(0.0, 0.0, w, h, &options))
    }
}

/// `_generateElementShape`'s `"diamond"` case. `None` means rough rejected the generated
/// corner-radius path; see [`rectangle`]'s doc comment.
pub(super) fn diamond(
    generator: &RoughGenerator,
    element: &Element,
    g: &GenericElement,
    dark_mode: bool,
) -> Option<Drawable> {
    let base = &g.base;
    let [
        top_x,
        top_y,
        right_x,
        right_y,
        bottom_x,
        bottom_y,
        left_x,
        left_y,
    ] = diamond_points(base.width, base.height);
    if matches!(base.roundness, Slot::Value(_)) {
        let vertical_radius = corner_radius((top_x - left_x).abs(), &base.roundness);
        let horizontal_radius = corner_radius((right_y - top_y).abs(), &base.roundness);
        let a = top_x + vertical_radius;
        let b = top_y + horizontal_radius;
        let c = right_x - vertical_radius;
        let d_coord = right_y - horizontal_radius;
        let e = right_y + horizontal_radius;
        let f = bottom_x + vertical_radius;
        let gg = bottom_y - horizontal_radius;
        let h = bottom_x - vertical_radius;
        let i = left_x + vertical_radius;
        let j = left_y + horizontal_radius;
        let k = left_y - horizontal_radius;
        let l = top_x - vertical_radius;
        let d = format!(
            "M {a} {b} L {c} {d_coord} \
             C {right_x} {right_y}, {right_x} {right_y}, {c} {e} \
             L {f} {gg} \
             C {bottom_x} {bottom_y}, {bottom_x} {bottom_y}, {h} {gg} \
             L {i} {j} \
             C {left_x} {left_y}, {left_x} {left_y}, {i} {k} \
             L {l} {b} \
             C {top_x} {top_y}, {top_x} {top_y}, {a} {b}"
        );
        let options = generate_rough_options(element, true, dark_mode).expect("diamond draws");
        generator.path(&d, &options).ok()
    } else {
        let options = generate_rough_options(element, false, dark_mode).expect("diamond draws");
        Some(generator.polygon(
            &[
                [top_x, top_y],
                [right_x, right_y],
                [bottom_x, bottom_y],
                [left_x, left_y],
            ],
            &options,
        ))
    }
}

/// `_generateElementShape`'s `"ellipse"` case.
pub(super) fn ellipse(
    generator: &RoughGenerator,
    element: &Element,
    g: &GenericElement,
    dark_mode: bool,
) -> Drawable {
    let base = &g.base;
    let options = generate_rough_options(element, false, dark_mode).expect("ellipse draws");
    generator.ellipse(
        base.width / 2.0,
        base.height / 2.0,
        base.width,
        base.height,
        &options,
    )
}
