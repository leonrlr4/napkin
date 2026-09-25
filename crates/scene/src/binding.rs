//! Arrow binding creation, ported at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` from
//! `packages/element/src/binding.ts`'s `bindBindingElement` (non-elbow branch), `applyBinding`,
//! `calculateFixedPointForNonElbowArrowBinding` (no focus point), `getBindingGap` and
//! `normalizeFixedPoint`; `packages/element/src/linearElementEditor.ts`'s
//! `getPointAtIndexGlobalCoordinates`; and `packages/element/src/bounds.ts`'s
//! `elementCenterPoint`/`getCenterForBounds`. Arrows following a moved shape
//! (`updateBoundElements`) is not here: napkin does not do it yet.

use serde_json::json;

use crate::collision;
use crate::element::{Element, LinearEnd};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{element_absolute_coords, element_bounds, rotate_point};
use crate::new_element::bump_version;

/// `BASE_BINDING_GAP`.
pub const BASE_BINDING_GAP: f64 = 5.0;
/// `MIN_BINDABLE_SIZE`.
const MIN_BINDABLE_SIZE: f64 = 1.0;
/// `FIXED_POINT_BOUND`.
const FIXED_POINT_BOUND: f64 = 10.0;

/// `normalizeFixedPoint`: a non-finite point becomes the "just off center" sentinel; otherwise
/// each ratio is clamped to `±FIXED_POINT_BOUND`, and a clamped ratio within `EPSILON` of `0.5`
/// (in either coordinate) is nudged to `0.5001` to keep an arrow's heading from flipping on
/// floating-point noise.
pub fn normalize_fixed_point(point: [f64; 2]) -> [f64; 2] {
    if !point[0].is_finite() || !point[1].is_finite() {
        return [0.5001, 0.5001];
    }
    const EPSILON: f64 = 0.0001;
    let clamped = point.map(|ratio| ratio.clamp(-FIXED_POINT_BOUND, FIXED_POINT_BOUND));
    if clamped.iter().any(|ratio| (ratio - 0.5).abs() < EPSILON) {
        clamped.map(|ratio| {
            if (ratio - 0.5).abs() < EPSILON {
                0.5001
            } else {
                ratio
            }
        })
    } else {
        clamped
    }
}

/// `getPointAtIndexGlobalCoordinates(arrow, 0 | -1)`: `None` for anything but a line or arrow,
/// or one with no points at the requested end.
pub fn linear_end_point(element: &Element, end: LinearEnd) -> Option<[f64; 2]> {
    let (Element::Line(l) | Element::Arrow(l)) = element else {
        return None;
    };
    let point = match end {
        LinearEnd::Start => l.points.first(),
        LinearEnd::End => l.points.last(),
    }?;
    let (_, center) = element_absolute_coords(element)?;
    Some(rotate_point(
        [l.base.x + point[0], l.base.y + point[1]],
        center,
        l.base.angle,
    ))
}

/// `calculateFixedPointForNonElbowArrowBinding` without a focus point: `arrow`'s `end` point,
/// expressed as a ratio of `target`'s unrotated bounds (`elementCenterPoint`/
/// `getCenterForBounds` is `target`'s axis-aligned [`element_bounds`] center, not its
/// unrotated placement center). `None` when either element's geometry cannot be read.
pub fn fixed_point_for(arrow: &Element, end: LinearEnd, target: &Element) -> Option<[f64; 2]> {
    let edge = linear_end_point(arrow, end)?;
    let placement = target.placement()?;
    let [x1, y1, x2, y2] = element_bounds(target)?;
    let center = [x1 + (x2 - x1) / 2.0, y1 + (y2 - y1) / 2.0];
    let unrotated = rotate_point(edge, center, -placement.angle);
    if placement.width < MIN_BINDABLE_SIZE || placement.height < MIN_BINDABLE_SIZE {
        return Some(normalize_fixed_point([0.5, 0.5]));
    }
    // `getBindingGap` for a non-elbow arrow.
    let gap = BASE_BINDING_GAP + collision::stroke_width(target) / 2.0;
    Some(normalize_fixed_point([
        (unrotated[0] - placement.x) / placement.width.max(gap),
        (unrotated[1] - placement.y) / placement.height.max(gap),
    ]))
}

/// `bindBindingElement(arrow, target, "orbit", end)` for a non-elbow arrow (`applyBinding`):
/// sets the arrow's binding to `{elementId, mode: "orbit", fixedPoint}` and, unless the target
/// already lists the arrow, appends it to the target's `boundElements`; each element that
/// changed gets a version bump (`scene.mutateElement` in `applyBinding`). A no-op when
/// `fixed_point_for` cannot compute a fixed point (an untyped or unplaceable arrow or target).
pub fn bind_arrow(
    file: &mut SceneFile,
    arrow: usize,
    end: LinearEnd,
    target: usize,
    env: &mut impl Env,
) {
    let Some(fixed_point) = fixed_point_for(&file.elements[arrow], end, &file.elements[target])
    else {
        return;
    };
    let target_id = file.elements[target]
        .id()
        .expect("bindable target has an id")
        .to_owned();
    let arrow_id = file.elements[arrow]
        .id()
        .expect("arrow has an id")
        .to_owned();

    let before = file.elements[arrow].clone();
    file.elements[arrow].set_binding(
        end,
        json!({"elementId": target_id, "mode": "orbit", "fixedPoint": fixed_point}),
    );
    if file.elements[arrow] != before {
        bump_version(&mut file.elements[arrow], env);
    }

    let before = file.elements[target].clone();
    file.elements[target].add_bound_element(&arrow_id, "arrow");
    if file.elements[target] != before {
        bump_version(&mut file.elements[target], env);
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use serde_json::json;

    use super::*;
    use crate::sample;

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(0);
        }

        fn now_ms(&mut self) -> f64 {
            1.0
        }
    }

    #[test]
    fn normalize_fixed_point_handles_nan_bounds_and_near_half() {
        assert_eq!(normalize_fixed_point([f64::NAN, 0.2]), [0.5001, 0.5001]);
        assert_eq!(
            normalize_fixed_point([f64::INFINITY, 0.2]),
            [0.5001, 0.5001]
        );
        assert_eq!(normalize_fixed_point([20.0, -20.0]), [10.0, -10.0]);
        assert_eq!(normalize_fixed_point([0.50005, 0.2]), [0.5001, 0.2]);
        assert_eq!(normalize_fixed_point([0.3, 0.49995]), [0.3, 0.5001]);
        assert_eq!(normalize_fixed_point([0.1, 0.9]), [0.1, 0.9]);
    }

    #[test]
    fn fixed_point_for_a_rotated_target_uses_its_unrotated_frame() {
        // A 100x100 target centered at (50, 50), rotated 90°: the arrow's end point sits at
        // its own (100, 50) in scene coordinates, which is the target's own (unrotated) top
        // edge midpoint, i.e. ratio (0.5, 0).
        let arrow = Element::from_value(sample::linear(
            "arrow",
            "a",
            [0.0, 0.0],
            &[[0.0, 0.0], [100.0, 50.0]],
        ));
        let target = Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"angle": FRAC_PI_2, "strokeWidth": 0}),
        ));
        let [x, y] = fixed_point_for(&arrow, LinearEnd::End, &target).expect("fixed point");
        // The raw ratio is exactly (0.5, 0); `normalize_fixed_point` nudges the 0.5 away.
        assert!((x - 0.5001).abs() < 1e-9, "{x}");
        assert!(y.abs() < 1e-9, "{y}");
    }

    #[test]
    fn tiny_target_binds_to_its_center() {
        let arrow = Element::from_value(sample::linear(
            "arrow",
            "a",
            [0.0, 0.0],
            &[[0.0, 0.0], [10.0, 10.0]],
        ));
        let target = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 0.5, 20.0]));
        assert_eq!(
            fixed_point_for(&arrow, LinearEnd::End, &target),
            Some([0.5001, 0.5001])
        );
    }

    #[test]
    fn binding_both_ends_to_the_same_target_adds_it_once() {
        let mut file = sample::file(vec![
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 0.0]]),
            sample::generic("rectangle", "r", [100.0, -25.0, 50.0, 50.0]),
        ]);
        bind_arrow(&mut file, 0, LinearEnd::Start, 1, &mut FixedEnv);
        bind_arrow(&mut file, 0, LinearEnd::End, 1, &mut FixedEnv);
        assert_eq!(file.elements[1].bound_elements(), vec![("a", "arrow")]);
        assert!(file.elements[0].binding_target(LinearEnd::Start).is_some());
        assert!(file.elements[0].binding_target(LinearEnd::End).is_some());
    }
}
