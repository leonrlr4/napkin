//! Element geometry: rotation, bounding boxes and a per-element cache for them
//! (`packages/element/src/bounds.ts`, `packages/element/src/linearElementEditor.ts`'s
//! `getElementAbsoluteCoords`, `packages/utils/src/shape.ts`'s `getCurvePathOps` and
//! `packages/common/src/points.ts`'s `getSizeFromPoints`, at the pinned commit).

use std::collections::HashMap;
use std::sync::Arc;

use rough::{Op, OpSetType};

use crate::collision::Segment;
use crate::element::Element;
use crate::shape::{ElementShape, ShapeContext, generate_element_shape};

/// `[min_x, min_y, max_x, max_y]` in scene coordinates.
pub type Bounds = [f64; 4];

/// `generateElementShape`'s rendering inputs, fixed for geometry purposes: bounds do not
/// depend on dark mode or the canvas background color, but the function still needs a
/// `ShapeContext` to call it.
fn shape_ctx() -> ShapeContext<'static> {
    ShapeContext {
        dark_mode: false,
        canvas_background_color: "#ffffff",
    }
}

/// `pointRotateRads`.
pub fn rotate_point(point: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    let (x, y) = (point[0], point[1]);
    let (cx, cy) = (center[0], center[1]);
    let cos = angle.cos();
    let sin = angle.sin();
    [
        (x - cx) * cos - (y - cy) * sin + cx,
        (x - cx) * sin + (y - cy) * cos + cy,
    ]
}

/// The bounding box of `points`, min then max per axis; `None` for no points.
fn points_bounds(points: &[[f64; 2]]) -> Option<Bounds> {
    let first = points.first()?;
    Some(bounds_of_nonempty(points, *first))
}

/// Shared by [`points_bounds`] and every fixed-size (always non-empty) corner list.
fn bounds_of_nonempty(points: &[[f64; 2]], first: [f64; 2]) -> Bounds {
    let mut min = first;
    let mut max = first;
    for p in points {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    [min[0], min[1], max[0], max[1]]
}

/// `getSizeFromPoints`: `[width, height]` of the points' bounding box, `[0, 0]` for none.
pub fn size_from_points(points: &[[f64; 2]]) -> [f64; 2] {
    match points_bounds(points) {
        Some([min_x, min_y, max_x, max_y]) => [max_x - min_x, max_y - min_y],
        None => [0.0, 0.0],
    }
}

/// `getBezierValueForT`.
fn bezier_value_for_t(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let one_minus_t = 1.0 - t;
    one_minus_t.powi(3) * p0
        + 3.0 * one_minus_t.powi(2) * t * p1
        + 3.0 * one_minus_t * t.powi(2) * p2
        + t.powi(3) * p3
}

/// `solveQuadratic`: the up-to-two extrema of a cubic Bezier's `t` in `[0, 1]`, evaluated
/// back to their curve value. `None` when the quadratic has no real root.
fn solve_quadratic(p0: f64, p1: f64, p2: f64, p3: f64) -> Option<[Option<f64>; 2]> {
    let i = p1 - p0;
    let j = p2 - p1;
    let k = p3 - p2;

    let a = 3.0 * i - 6.0 * j + 3.0 * k;
    let b = 6.0 * j - 6.0 * i;
    let c = 3.0 * i;

    let sqrt_part = b * b - 4.0 * a * c;
    if sqrt_part < 0.0 {
        return None;
    }

    let (t1, t2) = if a == 0.0 {
        let t = -c / b;
        (t, t)
    } else {
        (
            (-b + sqrt_part.sqrt()) / (2.0 * a),
            (-b - sqrt_part.sqrt()) / (2.0 * a),
        )
    };

    let s1 = (0.0..=1.0)
        .contains(&t1)
        .then(|| bezier_value_for_t(t1, p0, p1, p2, p3));
    let s2 = (0.0..=1.0)
        .contains(&t2)
        .then(|| bezier_value_for_t(t2, p0, p1, p2, p3));
    Some([s1, s2])
}

/// `getCubicBezierCurveBound`.
pub fn cubic_bezier_bounds(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) -> Bounds {
    let sol_x = solve_quadratic(p0[0], p1[0], p2[0], p3[0]);
    let sol_y = solve_quadratic(p0[1], p1[1], p2[1], p3[1]);

    let mut min_x = p0[0].min(p3[0]);
    let mut max_x = p0[0].max(p3[0]);
    if let Some(xs) = sol_x {
        for x in xs.into_iter().flatten() {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
        }
    }

    let mut min_y = p0[1].min(p3[1]);
    let mut max_y = p0[1].max(p3[1]);
    if let Some(ys) = sol_y {
        for y in ys.into_iter().flatten() {
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    [min_x, min_y, max_x, max_y]
}

/// `outer` contains `inner`, edges inclusive (`boundsContainBounds`).
pub fn bounds_contain(outer: &Bounds, inner: &Bounds) -> bool {
    let corners = [
        [inner[0], inner[1]],
        [inner[0], inner[3]],
        [inner[2], inner[1]],
        [inner[2], inner[3]],
    ];
    corners
        .iter()
        .all(|p| p[0] >= outer[0] && p[0] <= outer[2] && p[1] >= outer[1] && p[1] <= outer[3])
}

/// `getCurvePathOps`: the first `"path"`-kind op set of a drawable, or its first set if none
/// is `"path"`.
fn curve_path_ops(shape: &ElementShape) -> Option<&[Op]> {
    let ElementShape::Drawables(drawables) = shape else {
        return None;
    };
    let first = drawables.first()?;
    first
        .sets
        .iter()
        .find(|set| set.kind == OpSetType::Path)
        .or_else(|| first.sets.first())
        .map(|set| set.ops.as_slice())
}

/// `getMinMaxXYFromCurvePathOps`: only `BCurveTo` ops contribute (`move`/`lineTo` do not
/// draw, matching the JS TODOs for `lineTo`/`qcurveTo`). `transform` is applied to each
/// control point before the bezier bound is taken, exactly like JS's optional
/// `transformXY`; passing the identity function reproduces the untransformed case.
fn min_max_xy_from_curve_path_ops(ops: &[Op], transform: impl Fn([f64; 2]) -> [f64; 2]) -> Bounds {
    let mut current = [0.0, 0.0];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for op in ops {
        match op {
            Op::Move(d) => current = *d,
            Op::BCurveTo(d) => {
                let p0 = transform(current);
                let p1 = transform([d[0], d[1]]);
                let p2 = transform([d[2], d[3]]);
                let p3 = transform([d[4], d[5]]);
                current = [d[4], d[5]];

                let b = cubic_bezier_bounds(p0, p1, p2, p3);
                min_x = min_x.min(b[0]);
                min_y = min_y.min(b[1]);
                max_x = max_x.max(b[2]);
                max_y = max_y.max(b[3]);
            }
            Op::LineTo(_) => {}
        }
    }

    [min_x, min_y, max_x, max_y]
}

/// `getElementAbsoluteCoords` without bound text: the unrotated box and its center.
/// `None` for a `Raw` element whose placement cannot be read, or a freedraw/line/arrow with
/// no points to fall back on.
pub fn element_absolute_coords(element: &Element) -> Option<(Bounds, [f64; 2])> {
    match element {
        Element::Freedraw(f) => {
            let [min_x, min_y, max_x, max_y] = points_bounds(&f.points)?;
            let (x1, y1, x2, y2) = (
                min_x + f.base.x,
                min_y + f.base.y,
                max_x + f.base.x,
                max_y + f.base.y,
            );
            Some(([x1, y1, x2, y2], [(x1 + x2) / 2.0, (y1 + y2) / 2.0]))
        }
        Element::Line(l) | Element::Arrow(l) => {
            let shape = generate_element_shape(element, &shape_ctx());
            let ops_bounds = curve_path_ops(&shape)
                .map(|ops| min_max_xy_from_curve_path_ops(ops, |p| p))
                .filter(|b| b.iter().all(|v| v.is_finite()));
            let [min_x, min_y, max_x, max_y] = match ops_bounds {
                Some(b) => b,
                None => points_bounds(&l.points)?,
            };
            let (x1, y1, x2, y2) = (
                min_x + l.base.x,
                min_y + l.base.y,
                max_x + l.base.x,
                max_y + l.base.y,
            );
            Some(([x1, y1, x2, y2], [(x1 + x2) / 2.0, (y1 + y2) / 2.0]))
        }
        _ => {
            let p = element.placement()?;
            let (x1, y1, x2, y2) = (p.x, p.y, p.x + p.width, p.y + p.height);
            Some(([x1, y1, x2, y2], [(x1 + x2) / 2.0, (y1 + y2) / 2.0]))
        }
    }
}

/// `getElementBounds`/`ElementBounds.calculateBounds`: the axis-aligned bounds of the
/// rotated element, rotating about [`element_absolute_coords`]'s center. `None` wherever
/// `element_absolute_coords` is `None`.
pub fn element_bounds(element: &Element) -> Option<Bounds> {
    let (coords, center) = element_absolute_coords(element)?;
    let [x1, y1, x2, y2] = coords;
    let angle = element.placement().map_or(0.0, |p| p.angle);

    Some(match element {
        Element::Freedraw(f) => {
            let rotated: Vec<[f64; 2]> = f
                .points
                .iter()
                .map(|p| rotate_point([p[0] + f.base.x, p[1] + f.base.y], center, angle))
                .collect();
            points_bounds(&rotated).expect("freedraw has points: coords resolved above")
        }
        Element::Line(l) | Element::Arrow(l) => {
            let shape = generate_element_shape(element, &shape_ctx());
            let ops_bounds = curve_path_ops(&shape)
                .map(|ops| {
                    min_max_xy_from_curve_path_ops(ops, |p| {
                        rotate_point([p[0] + l.base.x, p[1] + l.base.y], center, angle)
                    })
                })
                .filter(|b| b.iter().all(|v| v.is_finite()));
            match ops_bounds {
                Some(b) => b,
                None => {
                    let rotated: Vec<[f64; 2]> = l
                        .points
                        .iter()
                        .map(|p| rotate_point([p[0] + l.base.x, p[1] + l.base.y], center, angle))
                        .collect();
                    points_bounds(&rotated).expect("line/arrow has points: coords resolved above")
                }
            }
        }
        Element::Diamond(_) => {
            let corners = [
                rotate_point([(x1 + x2) / 2.0, y1], center, angle),
                rotate_point([(x1 + x2) / 2.0, y2], center, angle),
                rotate_point([x1, (y1 + y2) / 2.0], center, angle),
                rotate_point([x2, (y1 + y2) / 2.0], center, angle),
            ];
            bounds_of_nonempty(&corners, corners[0])
        }
        Element::Ellipse(_) => {
            let w = (x2 - x1) / 2.0;
            let h = (y2 - y1) / 2.0;
            let cos = angle.cos();
            let sin = angle.sin();
            let ww = rough::js::hypot(w * cos, h * sin);
            let hh = rough::js::hypot(h * cos, w * sin);
            [
                center[0] - ww,
                center[1] - hh,
                center[0] + ww,
                center[1] + hh,
            ]
        }
        _ => {
            let corners = [
                rotate_point([x1, y1], center, angle),
                rotate_point([x1, y2], center, angle),
                rotate_point([x2, y2], center, angle),
                rotate_point([x2, y1], center, angle),
            ];
            bounds_of_nonempty(&corners, corners[0])
        }
    })
}

/// One [`GeometryCache`] entry: the `(version, versionNonce)` it was computed for, plus the
/// lazily computed fields themselves. Each field is `None` until first requested, so a
/// cache hit that only ever asks for `bounds` never pays for `absolute_coords`.
#[derive(Default)]
pub(crate) struct GeometryEntry {
    version: f64,
    version_nonce_bits: u64,
    absolute_coords: Option<Option<(Bounds, [f64; 2])>>,
    bounds: Option<Option<Bounds>>,
    /// `collision::GeometryCache::linear_collision_shape`'s cache; lives here so it resets
    /// on the same `(version, versionNonce)` change as every other field. `pub(crate)`:
    /// `collision.rs` reads and fills it directly through [`GeometryCache::entry_for`].
    pub(crate) linear_collision_shape: Option<Arc<[Segment]>>,
}

/// Per-element geometry keyed by `(id, version, versionNonce)`, one entry per id. Mirrors
/// `ElementBounds`'s `WeakMap` caches, minus the `nonRotated` variant (napkin has no caller
/// for it yet) and keyed by id instead of object identity, since elements are plain data
/// that gets replaced wholesale rather than mutated in place behind a stable reference.
#[derive(Default)]
pub struct GeometryCache {
    entries: HashMap<String, GeometryEntry>,
}

impl GeometryCache {
    /// The entry for `element`, reset to empty when its `(version, versionNonce)` no longer
    /// matches what it was last computed for. `pub(crate)`: `collision.rs`'s
    /// `linear_collision_shape` reads and fills its own field of the same entry.
    pub(crate) fn entry_for(&mut self, element: &Element) -> &mut GeometryEntry {
        let id = element.id().unwrap_or("").to_owned();
        let version = element.version();
        let version_nonce_bits = element.version_nonce().to_bits();
        let entry = self.entries.entry(id).or_default();
        if entry.version != version || entry.version_nonce_bits != version_nonce_bits {
            *entry = GeometryEntry {
                version,
                version_nonce_bits,
                ..GeometryEntry::default()
            };
        }
        entry
    }

    pub fn absolute_coords(&mut self, element: &Element) -> Option<(Bounds, [f64; 2])> {
        let entry = self.entry_for(element);
        *entry
            .absolute_coords
            .get_or_insert_with(|| element_absolute_coords(element))
    }

    pub fn bounds(&mut self, element: &Element) -> Option<Bounds> {
        let entry = self.entry_for(element);
        *entry.bounds.get_or_insert_with(|| element_bounds(element))
    }

    /// `getCommonBounds` over non-deleted elements with bounds. `None` when none qualify
    /// (JS instead returns `[0, 0, 0, 0]` for an empty input; napkin's callers need to tell
    /// "no elements" apart from "elements at the origin").
    pub fn common_bounds<'a>(
        &mut self,
        elements: impl IntoIterator<Item = &'a Element>,
    ) -> Option<Bounds> {
        let mut result: Option<Bounds> = None;
        for element in elements {
            if element.is_deleted() {
                continue;
            }
            let Some([x1, y1, x2, y2]) = self.bounds(element) else {
                continue;
            };
            result = Some(match result {
                None => [x1, y1, x2, y2],
                Some([min_x, min_y, max_x, max_y]) => {
                    [min_x.min(x1), min_y.min(y1), max_x.max(x2), max_y.max(y2)]
                }
            });
        }
        result
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

    use serde_json::json;

    use super::*;
    use crate::sample;

    fn element(value: serde_json::Value) -> Element {
        Element::from_value(value)
    }

    fn assert_bounds(actual: Option<Bounds>, expected: Bounds) {
        let actual = actual.expect("bounds");
        for i in 0..4 {
            assert!(
                (actual[i] - expected[i]).abs() < 1e-9,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn rotate_point_matches_point_rotate_rads() {
        let p = rotate_point([10.0, 0.0], [0.0, 0.0], FRAC_PI_2);
        assert!(p[0].abs() < 1e-12 && (p[1] - 10.0).abs() < 1e-12, "{p:?}");
        assert_eq!(
            size_from_points(&[[0.0, 0.0], [-5.0, 3.0], [10.0, -2.0]]),
            [15.0, 5.0]
        );
        assert_eq!(size_from_points(&[]), [0.0, 0.0]);
    }

    #[test]
    fn cubic_bounds_include_the_curve_extremum() {
        let b = cubic_bezier_bounds([0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]);
        assert_bounds(Some(b), [0.0, 0.0, 10.0, 7.5]);
        assert!(bounds_contain(
            &[0.0, 0.0, 10.0, 10.0],
            &[0.0, 2.0, 10.0, 10.0]
        ));
        assert!(!bounds_contain(
            &[0.0, 0.0, 10.0, 10.0],
            &[0.0, 2.0, 10.1, 10.0]
        ));
    }

    #[test]
    fn generic_bounds_rotate_about_the_center() {
        let rect = element(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]));
        assert_eq!(
            element_absolute_coords(&rect),
            Some(([0.0, 0.0, 100.0, 50.0], [50.0, 25.0]))
        );
        assert_bounds(element_bounds(&rect), [0.0, 0.0, 100.0, 50.0]);

        let rotated = element(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"angle": FRAC_PI_2}),
        ));
        assert_eq!(
            element_absolute_coords(&rotated).unwrap().0,
            [0.0, 0.0, 100.0, 50.0]
        );
        assert_bounds(element_bounds(&rotated), [25.0, -25.0, 75.0, 75.0]);

        let ellipse = element(sample::with(
            sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]),
            json!({"angle": FRAC_PI_4}),
        ));
        let half = 50.0f64.hypot(25.0) * FRAC_PI_4.cos();
        assert_bounds(
            element_bounds(&ellipse),
            [50.0 - half, 25.0 - half, 50.0 + half, 25.0 + half],
        );

        let diamond = element(sample::generic("diamond", "d", [0.0, 0.0, 100.0, 50.0]));
        assert_bounds(element_bounds(&diamond), [0.0, 0.0, 100.0, 50.0]);
    }

    #[test]
    fn freedraw_and_linear_bounds_come_from_points_and_curves() {
        let free = element(sample::freedraw(
            "f",
            [100.0, 100.0],
            &[[0.0, 0.0], [10.0, -5.0], [20.0, 5.0]],
        ));
        assert_eq!(
            element_absolute_coords(&free),
            Some(([100.0, 95.0, 120.0, 105.0], [110.0, 100.0]))
        );

        // Roughness 0 keeps every rough.js control point on its segment, so the curve bounds
        // equal the polyline's.
        let line = element(sample::with(
            sample::linear(
                "line",
                "l",
                [10.0, 20.0],
                &[[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]],
            ),
            json!({"roughness": 0, "roundness": null}),
        ));
        let (coords, center) = element_absolute_coords(&line).expect("line coords");
        assert_bounds(Some(coords), [10.0, 20.0, 110.0, 70.0]);
        assert!((center[0] - 60.0).abs() < 1e-9 && (center[1] - 45.0).abs() < 1e-9);

        // A rounded (curved) line's bounds cover at least its points.
        let curve = element(sample::linear(
            "line",
            "c",
            [0.0, 0.0],
            &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]],
        ));
        let b = element_bounds(&curve).expect("curve bounds");
        assert!(
            b[0] <= 0.0 && b[1] <= 0.0 && b[2] >= 100.0 && b[3] >= 50.0,
            "{b:?}"
        );
    }

    #[test]
    fn raw_elements_use_their_placement_and_cache_follows_versions() {
        let image = element(
            json!({"id": "i", "type": "image", "x": 5, "y": 6, "width": 7, "height": 8, "version": 1, "versionNonce": 1}),
        );
        assert_eq!(
            element_absolute_coords(&image),
            Some(([5.0, 6.0, 12.0, 14.0], [8.5, 10.0]))
        );
        assert_eq!(
            element_absolute_coords(&element(json!({"id": "x", "type": "magic"}))),
            None
        );

        let mut cache = GeometryCache::default();
        let a = element(sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]));
        let b = element(sample::generic("ellipse", "b", [20.0, -5.0, 10.0, 10.0]));
        assert_eq!(cache.common_bounds([&a, &b]), Some([0.0, -5.0, 30.0, 10.0]));
        assert_eq!(cache.common_bounds(std::iter::empty()), None);

        let moved = element(sample::with(
            sample::generic("rectangle", "a", [50.0, 0.0, 10.0, 10.0]),
            json!({"versionNonce": 99}),
        ));
        assert_eq!(cache.bounds(&moved), Some([50.0, 0.0, 60.0, 10.0]));
    }
}
