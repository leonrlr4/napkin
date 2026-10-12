//! Point hit testing: whether a point lies on or inside an element's outline
//! (`packages/element/src/collision.ts`, `packages/element/src/distance.ts` and the
//! `deconstruct*`/`isPathALoop`/`getCornerRadius` helpers of `packages/element/src/utils.ts`,
//! all at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`).
//!
//! The outline tests (`distance_to_element`, and `linear_collision_shape` for line/arrow/
//! freedraw) port the cited functions directly: rectanguloid and diamond corners stay exact
//! cubic Beziers, and ellipse distance keeps the three-iteration Newton-ish approximation
//! `ellipseDistanceFromPoint` uses. The "is this point inside the shape" test
//! (`is_point_in_element`) ports `isPointInElement`'s ray-cast-and-count-intersections for
//! rectangle, text, image, diamond and ellipse, so a point exactly on the outline falls on the
//! same side as in JS; a closed line/freedraw uses an even-odd ray cast against a 16-segment
//! polyline approximation of the cubic collision shape.
//!
//! Binding uses the intersection side directly: `intersect_element_with_line_segment` ports
//! `intersectElementWithLineSegment` (including the outline grown by an offset, and
//! `crate::curve::curve_intersect_line_segment`'s Newton solver for rounded corners and
//! curves), and `hovered_element_for_binding` ports `getHoveredElementForBinding`.

use std::sync::Arc;

use rough::{Op, OpSet, OpSetType, Options, RoughGenerator};
use serde_json::Value;

use crate::element::{Element, LinearElement, Placement, Roundness};
use crate::geometry::GeometryCache;
use crate::json::Slot;

/// `DEFAULT_TRANSFORM_HANDLE_SPACING` (`packages/common/src/constants.ts`), in CSS pixels.
pub const DEFAULT_TRANSFORM_HANDLE_SPACING: f64 = 2.0;
/// `SIDE_RESIZING_THRESHOLD`, in CSS pixels.
pub const SIDE_RESIZING_THRESHOLD: f64 = 2.0 * DEFAULT_TRANSFORM_HANDLE_SPACING;
/// `DEFAULT_COLLISION_THRESHOLD`, in CSS pixels.
pub const DEFAULT_COLLISION_THRESHOLD: f64 = 2.0 * SIDE_RESIZING_THRESHOLD - 0.00001;
/// `LINE_CONFIRM_THRESHOLD`.
pub const LINE_CONFIRM_THRESHOLD: f64 = 8.0;

/// A straight segment, as its two endpoints.
type LineSeg = [[f64; 2]; 2];
/// A cubic Bezier curve, as its 4 control points (`p0`, `p1`, `p2`, `p3`).
type CurveSeg = [[f64; 2]; 4];

/// One piece of an element's outline in scene coordinates. For line/arrow/freedraw these
/// already include the element's own rotation ([`GeometryCache::linear_collision_shape`]'s
/// doc comment); for every other element type the outline is derived directly from the
/// element's placement wherever it is needed, and this type does not appear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line(LineSeg),
    Cubic(CurveSeg),
}

/// `element.strokeWidth`; a `Raw` element reads it from its JSON when it is a number, 0
/// otherwise (it has no `ElementBase` to fall back on).
pub(crate) fn stroke_width(element: &Element) -> f64 {
    match element {
        Element::Raw(v) => v.get("strokeWidth").and_then(Value::as_f64).unwrap_or(0.0),
        _ => element.base().map_or(0.0, |b| b.stroke_width),
    }
}

/// `getElementHitThreshold`: `max(strokeWidth / 2 + 0.1, 0.85 * DEFAULT_COLLISION_THRESHOLD /
/// zoom)`.
pub fn hit_threshold(element: &Element, zoom: f64) -> f64 {
    (stroke_width(element) / 2.0 + 0.1).max(0.85 * DEFAULT_COLLISION_THRESHOLD / zoom)
}

/// `hasBackground`: element (and tool-only) types that can carry a fill.
fn has_background(kind: &str) -> bool {
    matches!(
        kind,
        "rectangle"
            | "stickynote"
            | "iframe"
            | "embeddable"
            | "ellipse"
            | "diamond"
            | "line"
            | "freedraw"
            | "autoshape"
            | "bucketfill"
    )
}

/// `backgroundColor`, defaulting to `"transparent"` when absent (a `Raw` element without the
/// key, or any element type with no such field).
fn background_color(element: &Element) -> &str {
    match element {
        Element::Raw(v) => v
            .get("backgroundColor")
            .and_then(Value::as_str)
            .unwrap_or("transparent"),
        _ => element
            .base()
            .map_or("transparent", |b| b.background_color.as_str()),
    }
}

/// `hasBoundTextElement`, simplified to "has a `boundElements` entry of type `text`" (the
/// full check also requires the element to be a rectangle/stickynote/ellipse/diamond
/// container, which every realistic bound-text owner already is).
fn has_bound_text_element(element: &Element) -> bool {
    element
        .bound_elements()
        .iter()
        .any(|&(_, kind)| kind == "text")
}

/// The points of a line, arrow or freedraw element; a `Raw` element reads its JSON `points`
/// array, skipping non-numeric-pair entries. Empty for every other element type.
fn linear_points(element: &Element) -> std::borrow::Cow<'_, [[f64; 2]]> {
    match element {
        Element::Line(l) | Element::Arrow(l) => std::borrow::Cow::Borrowed(&l.points),
        Element::Freedraw(f) => std::borrow::Cow::Borrowed(&f.points),
        Element::Raw(v) => std::borrow::Cow::Owned(
            v.get("points")
                .and_then(Value::as_array)
                .map(|points| {
                    points
                        .iter()
                        .filter_map(|p| {
                            let p = p.as_array()?;
                            Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?])
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        _ => std::borrow::Cow::Borrowed(&[]),
    }
}

/// `isPathALoop`: whether `points`' first and last point are close enough, relative to
/// `zoom`, to be considered a closed loop.
pub fn is_path_a_loop(points: &[[f64; 2]], zoom: f64) -> bool {
    if points.len() < 3 {
        return false;
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let distance = rough::js::hypot(last[0] - first[0], last[1] - first[1]);
    distance <= LINE_CONFIRM_THRESHOLD / zoom
}

/// `shouldTestInside`.
pub fn should_test_inside(element: &Element) -> bool {
    let kind = element.kind();
    if kind == "arrow" {
        return false;
    }

    let is_draggable_from_inside = (has_background(kind)
        && !crate::color::is_transparent(background_color(element)))
        || has_bound_text_element(element)
        || kind == "iframe"
        || kind == "embeddable"
        || kind == "text";

    match kind {
        "line" | "freedraw" => {
            is_draggable_from_inside && is_path_a_loop(&linear_points(element), 1.0)
        }
        _ => is_draggable_from_inside || kind == "image",
    }
}

// ---------------------------------------------------------------------------
// Bezier/segment primitives shared by distance and inside tests
// ---------------------------------------------------------------------------

/// `bezierEquation`.
fn bezier_point(c: CurveSeg, t: f64) -> [f64; 2] {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let cc = 3.0 * mt * t * t;
    let d = t * t * t;
    [
        a * c[0][0] + b * c[1][0] + cc * c[2][0] + d * c[3][0],
        a * c[0][1] + b * c[1][1] + cc * c[2][1] + d * c[3][1],
    ]
}

/// `localMinimum`: bisects `[min, max]` for the parameter minimizing `f`, to within `e`.
/// `None` when `[min, max]` is already narrower than `e` (the JS reads `k` as `undefined`
/// then, since the `while` body never runs).
fn local_minimum(min: f64, max: f64, f: impl Fn(f64) -> f64, e: f64) -> Option<f64> {
    let mut m = min;
    let mut n = max;
    let mut k = None;
    while n - m > e {
        let candidate = (n + m) / 2.0;
        k = Some(candidate);
        if f(candidate - e) < f(candidate + e) {
            n = candidate;
        } else {
            m = candidate;
        }
    }
    k
}

/// `curveClosestParameter`.
fn curve_closest_parameter(c: CurveSeg, p: [f64; 2]) -> f64 {
    const TOLERANCE: f64 = 1e-3;
    const MAX_STEPS: usize = 30;

    let dist_at = |t: f64| {
        let q = bezier_point(c, t);
        rough::js::hypot(p[0] - q[0], p[1] - q[1])
    };

    let mut min = f64::INFINITY;
    let mut closest_step = 0;
    for step in 0..=MAX_STEPS {
        let d = dist_at(step as f64 / MAX_STEPS as f64);
        if d < min {
            min = d;
            closest_step = step;
        }
    }

    let t0 = ((closest_step as f64 - 1.0) / MAX_STEPS as f64).max(0.0);
    let t1 = ((closest_step as f64 + 1.0) / MAX_STEPS as f64).min(1.0);
    local_minimum(t0, t1, dist_at, TOLERANCE).unwrap_or(closest_step as f64 / MAX_STEPS as f64)
}

/// `curvePointDistance`.
fn curve_point_distance(c: CurveSeg, p: [f64; 2]) -> f64 {
    let t = curve_closest_parameter(c, p);
    let q = bezier_point(c, t);
    rough::js::hypot(p[0] - q[0], p[1] - q[1])
}

/// `lineSegmentClosestParameter`.
fn line_segment_closest_parameter(p: [f64; 2], seg: LineSeg) -> f64 {
    let [x1, y1] = seg[0];
    let [x2, y2] = seg[1];
    let a = p[0] - x1;
    let b = p[1] - y1;
    let c = x2 - x1;
    let d = y2 - y1;
    let dot = a * c + b * d;
    let len_sq = c * c + d * d;
    let param = if len_sq != 0.0 { dot / len_sq } else { 0.0 };
    param.clamp(0.0, 1.0)
}

/// `distanceToLineSegment`.
fn distance_to_line_segment(p: [f64; 2], seg: LineSeg) -> f64 {
    let t = line_segment_closest_parameter(p, seg);
    let [x1, y1] = seg[0];
    let [x2, y2] = seg[1];
    let q = [x1 + t * (x2 - x1), y1 + t * (y2 - y1)];
    rough::js::hypot(p[0] - q[0], p[1] - q[1])
}

/// A corner curve shared by rectanguloid and diamond deconstruction: from `edge_end`, bulging
/// towards `corner`, to `next_edge_start`. The two control points sit 2/3 of the way from
/// each endpoint towards `corner` (`packages/element/src/utils.ts`'s repeated
/// `endpoint + (2/3) * (corner - endpoint)` pattern in `deconstructRectanguloidElement`).
fn rounded_corner(edge_end: [f64; 2], corner: [f64; 2], next_edge_start: [f64; 2]) -> CurveSeg {
    let lerp = |from: [f64; 2], to: [f64; 2]| {
        [
            from[0] + (2.0 / 3.0) * (to[0] - from[0]),
            from[1] + (2.0 / 3.0) * (to[1] - from[1]),
        ]
    };
    [
        edge_end,
        lerp(edge_end, corner),
        lerp(next_edge_start, corner),
        next_edge_start,
    ]
}

// ---------------------------------------------------------------------------
// Rectanguloid (rectangle, text, image, and any `Raw` element regardless of its own `type`,
// exactly as `distanceToElement`'s switch maps image/frame/embeddable/stickynote/iframe/
// magicframe to `distanceToRectanguloidElement` as well: the types without a napkin
// `Element` variant of their own land in the `Raw` arm)
// ---------------------------------------------------------------------------

/// `getCornerRadius(Math.min(width, height), element)`, then `deconstructRectanguloidElement`'s
/// `if (radius === 0) radius = 0.01`. A `Rectangle`'s own `roundness` applies; every other
/// element this is called for (text, image, `Raw`) has none.
fn rectanguloid_radius(element: &Element, min_dim: f64) -> f64 {
    let none = Slot::Missing;
    let roundness = match element {
        Element::Rectangle(g) => &g.base.roundness,
        _ => &none,
    };
    let r = crate::shape::corner_radius(min_dim, roundness);
    if r == 0.0 { 0.01 } else { r }
}

/// `deconstructRectanguloidElement` with `offset` 0 (`offset_outline` grows it). Returns the
/// 4 straight sides and the 4 rounded-corner curves, in the element's own unrotated
/// coordinate frame.
fn deconstruct_rectanguloid(placement: Placement, radius: f64) -> ([LineSeg; 4], [CurveSeg; 4]) {
    let (x, y, w, h) = (placement.x, placement.y, placement.width, placement.height);
    let r = radius;
    let top = [[x + r, y], [x + w - r, y]];
    let right = [[x + w, y + r], [x + w, y + h - r]];
    let bottom = [[x + r, y + h], [x + w - r, y + h]];
    let left = [[x, y + h - r], [x, y + r]];

    let top_left = rounded_corner(left[1], [x, y], top[0]);
    let top_right = rounded_corner(top[1], [x + w, y], right[0]);
    let bottom_right = rounded_corner(right[1], [x + w, y + h], bottom[1]);
    let bottom_left = rounded_corner(bottom[0], [x, y + h], left[0]);

    (
        [top, right, bottom, left],
        [top_left, top_right, bottom_right, bottom_left],
    )
}

/// Rotates `point` by `-placement.angle` about `center`, i.e. into the element's own
/// unrotated frame ("emulate a rotated shape by rotating the point the other way instead").
fn to_local_frame(point: [f64; 2], center: [f64; 2], placement: Placement) -> [f64; 2] {
    if placement.angle == 0.0 {
        point
    } else {
        crate::geometry::rotate_point(point, center, -placement.angle)
    }
}

fn distance_to_rectanguloid(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> f64 {
    let Some((_, center)) = geometry.absolute_coords(element) else {
        return f64::INFINITY;
    };
    let Some(placement) = element.placement() else {
        return f64::INFINITY;
    };
    let local = to_local_frame(point, center, placement);
    let radius = rectanguloid_radius(element, placement.width.min(placement.height));
    let (sides, corners) = deconstruct_rectanguloid(placement, radius);

    let mut min = f64::INFINITY;
    for side in sides {
        min = min.min(distance_to_line_segment(local, side));
    }
    for corner in corners {
        min = min.min(curve_point_distance(corner, local));
    }
    min
}

/// Point-in-rounded-rectangle, in the element's own unrotated local coordinates (`lx`/`ly`
/// relative to the box's own origin): a plain rectangle test, with each of the 4 corners
/// carved into a circular arc of radius `r`. This is `isPointInElement`'s equivalent for a
/// rectanguloid: the real corner is a cubic Bezier (see [`rounded_corner`]), but a circular
/// arc of the same radius is indistinguishable for hit testing.
fn point_in_rounded_rect(lx: f64, ly: f64, w: f64, h: f64, r: f64) -> bool {
    if lx < 0.0 || lx > w || ly < 0.0 || ly > h {
        return false;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    if lx < r && ly < r {
        return (lx - r).powi(2) + (ly - r).powi(2) <= r * r;
    }
    if lx > w - r && ly < r {
        return (lx - (w - r)).powi(2) + (ly - r).powi(2) <= r * r;
    }
    if lx > w - r && ly > h - r {
        return (lx - (w - r)).powi(2) + (ly - (h - r)).powi(2) <= r * r;
    }
    if lx < r && ly > h - r {
        return (lx - r).powi(2) + (ly - (h - r)).powi(2) <= r * r;
    }
    true
}

fn is_point_in_rectanguloid(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> bool {
    let Some((_, center)) = geometry.absolute_coords(element) else {
        return false;
    };
    let Some(placement) = element.placement() else {
        return false;
    };
    let local = to_local_frame(point, center, placement);
    let radius = rectanguloid_radius(element, placement.width.min(placement.height));
    point_in_rounded_rect(
        local[0] - placement.x,
        local[1] - placement.y,
        placement.width,
        placement.height,
        radius,
    )
}

// ---------------------------------------------------------------------------
// Diamond
// ---------------------------------------------------------------------------

/// `getDiamondBaseCorners`'s vertical/horizontal radius: `getCornerRadius` when the element
/// has a `roundness`, otherwise 1% of the corresponding dimension (`utils.ts`'s ternary; note
/// this is *not* the rectanguloid's "bump zero up to 0.01" rule).
fn diamond_radius(dim: f64, roundness: &Slot<Roundness>) -> f64 {
    if matches!(roundness, Slot::Value(_)) {
        crate::shape::corner_radius(dim, roundness)
    } else {
        dim * 0.01
    }
}

/// `getDiamondBaseCorners` plus `deconstructDiamondElement`'s `offset`-less branch: the 4
/// sides and 4 corner curves of a diamond, in the element's own unrotated coordinate frame.
pub(crate) fn deconstruct_diamond(
    placement: Placement,
    roundness: &Slot<Roundness>,
) -> ([LineSeg; 4], [CurveSeg; 4]) {
    let [
        top_x,
        top_y,
        right_x,
        right_y,
        bottom_x,
        bottom_y,
        left_x,
        left_y,
    ] = crate::shape::diamond_points(placement.width, placement.height);
    let vr = diamond_radius((top_x - left_x).abs(), roundness);
    let hr = diamond_radius((right_y - top_y).abs(), roundness);

    let (x, y) = (placement.x, placement.y);
    let top = [x + top_x, y + top_y];
    let right = [x + right_x, y + right_y];
    let bottom = [x + bottom_x, y + bottom_y];
    let left = [x + left_x, y + left_y];

    let right_curve = [
        [right[0] - vr, right[1] - hr],
        right,
        right,
        [right[0] - vr, right[1] + hr],
    ];
    let bottom_curve = [
        [bottom[0] + vr, bottom[1] - hr],
        bottom,
        bottom,
        [bottom[0] - vr, bottom[1] - hr],
    ];
    let left_curve = [
        [left[0] + vr, left[1] + hr],
        left,
        left,
        [left[0] + vr, left[1] - hr],
    ];
    let top_curve = [
        [top[0] - vr, top[1] + hr],
        top,
        top,
        [top[0] + vr, top[1] + hr],
    ];

    let sides = [
        [right_curve[3], bottom_curve[0]],
        [bottom_curve[3], left_curve[0]],
        [left_curve[3], top_curve[0]],
        [top_curve[3], right_curve[0]],
    ];
    (sides, [right_curve, bottom_curve, left_curve, top_curve])
}

fn distance_to_diamond(geometry: &mut GeometryCache, element: &Element, point: [f64; 2]) -> f64 {
    let Element::Diamond(g) = element else {
        unreachable!("distance_to_diamond only called for Element::Diamond")
    };
    let Some((_, center)) = geometry.absolute_coords(element) else {
        return f64::INFINITY;
    };
    let Some(placement) = element.placement() else {
        return f64::INFINITY;
    };
    let local = to_local_frame(point, center, placement);
    let (sides, corners) = deconstruct_diamond(placement, &g.base.roundness);

    let mut min = f64::INFINITY;
    for side in sides {
        min = min.min(distance_to_line_segment(local, side));
    }
    for corner in corners {
        min = min.min(curve_point_distance(corner, local));
    }
    min
}

// ---------------------------------------------------------------------------
// Ellipse
// ---------------------------------------------------------------------------

/// JS `Math.sign`, except for `0`/`-0`/`NaN` (all map to plain `0.0` here): the only caller,
/// [`ellipse_distance_from_point`], only ever multiplies the result by another factor, and
/// `0.0 * x == -0.0 * x` for every finite `x` that matters here.
fn math_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `ellipseDistanceFromPoint`, with `p` already translated so the ellipse's center is the
/// origin. The 3-iteration loop is Excalidraw's own fixed-iteration-count approximation, not
/// a convergence loop: it is ported as-is, not replaced with a loop-until-converged version.
fn ellipse_distance_from_point(p: [f64; 2], half_width: f64, half_height: f64) -> f64 {
    if p[0] == 0.0 && p[1] == 0.0 {
        // `Math.sign(0) === 0` zeroes out both components of `[minX, minY]` below, so the
        // ported algorithm reports a distance of exactly 0 for a point sitting exactly on
        // the ellipse's own center - never true geometrically (the nearest boundary point
        // from dead center is `min(half_width, half_height)` away). This is the one input
        // where the loop's fixed iteration count cannot be trusted, so it is special-cased
        // instead of ported as-is.
        return half_width.min(half_height);
    }

    let a = half_width;
    let b = half_height;
    let px = p[0].abs();
    let py = p[1].abs();

    let mut tx = 0.707_f64;
    let mut ty = 0.707_f64;
    for _ in 0..3 {
        let x = a * tx;
        let y = b * ty;
        let ex = (a * a - b * b) * tx.powi(3) / a;
        let ey = (b * b - a * a) * ty.powi(3) / b;
        let rx = x - ex;
        let ry = y - ey;
        let qx = px - ex;
        let qy = py - ey;
        let r = rough::js::hypot(ry, rx);
        let q = rough::js::hypot(qy, qx);
        tx = ((qx * r / q + ex) / a).clamp(0.0, 1.0);
        ty = ((qy * r / q + ey) / b).clamp(0.0, 1.0);
        let t = rough::js::hypot(ty, tx);
        tx /= t;
        ty /= t;
    }

    let min = [a * tx * math_sign(p[0]), b * ty * math_sign(p[1])];
    rough::js::hypot(p[0] - min[0], p[1] - min[1])
}

fn distance_to_ellipse(geometry: &mut GeometryCache, element: &Element, point: [f64; 2]) -> f64 {
    let Some((_, center)) = geometry.absolute_coords(element) else {
        return f64::INFINITY;
    };
    let Some(placement) = element.placement() else {
        return f64::INFINITY;
    };
    let local = to_local_frame(point, center, placement);
    let translated = [local[0] - center[0], local[1] - center[1]];
    ellipse_distance_from_point(translated, placement.width / 2.0, placement.height / 2.0)
}

// ---------------------------------------------------------------------------
// Line, arrow, freedraw
// ---------------------------------------------------------------------------

/// `isElbowArrow`.
fn is_elbow_arrow(l: &LinearElement) -> bool {
    l.elbowed == Some(true)
}

/// The collision options `generateLinearCollisionShape` builds: fixed roughness of 0 (an
/// exact outline, not a sketchy one) with vertices preserved and multi-stroke disabled, so
/// the generated ops line up exactly with `points`.
fn collision_options(seed: f64) -> Options {
    Options {
        seed: Some(seed),
        disable_multi_stroke: Some(true),
        disable_multi_stroke_fill: Some(true),
        roughness: Some(0.0),
        preserve_vertices: Some(true),
        ..Options::default()
    }
}

/// The outline ("path") op set of a rough.js-generated drawable, or its first set if none is
/// tagged as the outline (mirrors `geometry.rs`'s `curve_path_ops`).
fn path_ops(sets: &[OpSet]) -> &[Op] {
    sets.iter()
        .find(|set| set.kind == OpSetType::Path)
        .or_else(|| sets.first())
        .map_or(&[], |set| set.ops.as_slice())
}

/// Walks a rough.js curve's `move`/`bcurveTo` ops, rotating each control point by `angle`
/// about `center` and translating by `base`, appending one [`Segment::Cubic`] per
/// `bcurveTo`. Mirrors `generateLinearCollisionShape`'s per-op `pointRotateRads` calls.
fn curve_ops_to_segments(ops: &[Op], base: [f64; 2], center: [f64; 2], angle: f64) -> Vec<Segment> {
    let place = |d: [f64; 2]| {
        crate::geometry::rotate_point([base[0] + d[0], base[1] + d[1]], center, angle)
    };

    let mut segments = Vec::new();
    let mut current = None;
    for op in ops {
        match op {
            Op::Move(d) => current = Some(place(*d)),
            Op::BCurveTo(d) => {
                let p0 = current.expect("a bcurveTo op follows a move");
                let p1 = place([d[0], d[1]]);
                let p2 = place([d[2], d[3]]);
                let p3 = place([d[4], d[5]]);
                segments.push(Segment::Cubic([p0, p1, p2, p3]));
                current = Some(p3);
            }
            Op::LineTo(_) => {}
        }
    }
    segments
}

/// `generateLinearCollisionShape` for line/arrow/freedraw, already rotated and translated
/// into scene coordinates.
fn compute_linear_collision_shape(element: &Element, center: [f64; 2]) -> Vec<Segment> {
    let generator = RoughGenerator::new();

    match element {
        Element::Line(l) | Element::Arrow(l) => {
            let base = [l.base.x, l.base.y];
            let angle = l.base.angle;

            if is_elbow_arrow(l) {
                // `scene::shape` builds an elbow arrow's rough.js path string
                // (`shape::linear`'s private `elbow_arrow_path`) but does not expose it, and
                // elbow arrows are never rotated in practice, so this treats the raw points
                // as an (unrotated) polyline rather than porting the rounded-corner path.
                return l
                    .points
                    .windows(2)
                    .map(|w| {
                        Segment::Line([
                            [base[0] + w[0][0], base[1] + w[0][1]],
                            [base[0] + w[1][0], base[1] + w[1][1]],
                        ])
                    })
                    .collect();
            }

            if l.points.is_empty() {
                return Vec::new();
            }

            if matches!(l.base.roundness, Slot::Value(_)) {
                let drawable = generator.curve(&l.points, &collision_options(l.base.seed));
                let ops = path_ops(&drawable.sets);
                let taken = &ops[..l.points.len().min(ops.len())];
                curve_ops_to_segments(taken, base, center, angle)
            } else {
                l.points
                    .iter()
                    .map(|p| {
                        crate::geometry::rotate_point(
                            [base[0] + p[0], base[1] + p[1]],
                            center,
                            angle,
                        )
                    })
                    .collect::<Vec<_>>()
                    .windows(2)
                    .map(|w| Segment::Line([w[0], w[1]]))
                    .collect()
            }
        }
        Element::Freedraw(f) => {
            if f.points.len() < 2 {
                return Vec::new();
            }
            let base = [f.base.x, f.base.y];
            let angle = f.base.angle;
            let simplified = rough::points_on_curve::simplify(&f.points, 0.75);
            let drawable = generator.curve(&simplified, &collision_options(f.base.seed));
            let ops = path_ops(&drawable.sets);
            let taken = &ops[..f.points.len().min(ops.len())];
            curve_ops_to_segments(taken, base, center, angle)
        }
        _ => Vec::new(),
    }
}

impl GeometryCache {
    /// `generateLinearCollisionShape` for line/arrow/freedraw, in scene coordinates: each
    /// [`Segment`] already reflects the element's rotation, so callers compare it directly
    /// against a query point without rotating that point first (contrast the rectanguloid/
    /// diamond/ellipse tests above, which rotate the query point into the element's local
    /// frame instead). Cached per element the same way `bounds`/`absolute_coords` are.
    pub fn linear_collision_shape(&mut self, element: &Element) -> Arc<[Segment]> {
        let Some((_, center)) = self.absolute_coords(element) else {
            return Arc::from([]);
        };
        let entry = self.entry_for(element);
        if let Some(cached) = &entry.linear_collision_shape {
            return cached.clone();
        }
        let shape: Arc<[Segment]> = compute_linear_collision_shape(element, center).into();
        entry.linear_collision_shape = Some(shape.clone());
        shape
    }
}

/// Even-odd ray cast (crossing number) of a horizontal ray from `point` towards `+x` against
/// `edges`, an unordered set of segments forming a closed boundary. Edges are independent of
/// each other in this test, so a curve already flattened into consecutive sub-segments (see
/// [`is_point_in_linear`]) works the same as the polygon's own straight sides.
fn point_in_polygon_parity(point: [f64; 2], edges: &[LineSeg]) -> bool {
    let (px, py) = (point[0], point[1]);
    let mut inside = false;
    for &[[x1, y1], [x2, y2]] in edges {
        if (y1 > py) != (y2 > py) {
            let x_intersect = x1 + (py - y1) / (y2 - y1) * (x2 - x1);
            if px < x_intersect {
                inside = !inside;
            }
        }
    }
    inside
}

/// `isPointInElement`'s equivalent for a closed line/freedraw: flattens each
/// [`Segment::Cubic`] into 16 straight sub-segments, then runs an even-odd ray cast (`point`
/// is already in scene coordinates, matching [`GeometryCache::linear_collision_shape`]).
fn is_point_in_linear(geometry: &mut GeometryCache, element: &Element, point: [f64; 2]) -> bool {
    if !is_path_a_loop(&linear_points(element), 1.0) {
        return false;
    }

    let segments = geometry.linear_collision_shape(element);
    let mut edges: Vec<LineSeg> = Vec::new();
    const SUBDIVISIONS: usize = 16;
    for segment in segments.iter() {
        match segment {
            Segment::Line(l) => edges.push(*l),
            Segment::Cubic(c) => {
                let mut prev = c[0];
                for i in 1..=SUBDIVISIONS {
                    let p = bezier_point(*c, i as f64 / SUBDIVISIONS as f64);
                    edges.push([prev, p]);
                    prev = p;
                }
            }
        }
    }
    point_in_polygon_parity(point, &edges)
}

/// Whether `point` lies inside the element's closed outline (`isPointInElement`). A line,
/// arrow or freedraw is tested analytically; the shapes bindings aim at (rectangle, text,
/// image, diamond, ellipse) are tested the way JS does, by counting where a ray from `point`
/// crosses the outline ([`is_point_in_shape_by_ray`]), so a point exactly on the outline can
/// land on either side, as it does there.
pub fn is_point_in_element(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> bool {
    match element {
        Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_) => {
            is_point_in_linear(geometry, element, point)
        }
        Element::Diamond(_)
        | Element::Ellipse(_)
        | Element::Rectangle(_)
        | Element::Text(_)
        | Element::Image(_) => is_point_in_shape_by_ray(geometry, element, point),
        Element::Raw(_) => is_point_in_rectanguloid(geometry, element, point),
    }
}

/// `isPointInElement` for a closed shape: within the rotated bounds, and an odd number of
/// distinct crossings between the outline and the segment from `point` to the point twice the
/// larger side away from the bounds center, on the far side of `point`.
fn is_point_in_shape_by_ray(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> bool {
    let (Some([x1, y1, x2, y2]), Some(placement)) = (geometry.bounds(element), element.placement())
    else {
        return false;
    };
    if !(point[0] <= x1.max(x2)
        && point[0] >= x1.min(x2)
        && point[1] <= y1.max(y2)
        && point[1] >= y1.min(y2))
    {
        return false;
    }

    let center = [(x1 + x2) / 2.0, (y1 + y2) / 2.0];
    // `vectorFromPoint(point, center, 0.1)`: a point nearer than 0.1 to the center points down.
    let offset = [point[0] - center[0], point[1] - center[1]];
    let direction = if offset[0] * offset[0] + offset[1] * offset[1] < 0.1 * 0.1 {
        [0.0, 1.0]
    } else {
        let magnitude = (offset[0] * offset[0] + offset[1] * offset[1]).sqrt();
        [offset[0] / magnitude, offset[1] / magnitude]
    };
    let reach = placement.width.max(placement.height) * 2.0;
    let far = [
        center[0] + direction[0] * reach,
        center[1] + direction[1] * reach,
    ];

    let mut distinct: Vec<[f64; 2]> = Vec::new();
    for hit in intersect_element_with_line_segment(geometry, element, [point, far], 0.0) {
        // `pointsEqual`'s default tolerance, `PRECISION`.
        if !distinct
            .iter()
            .any(|q| (q[0] - hit[0]).abs() < 10e-5 && (q[1] - hit[1]).abs() < 10e-5)
        {
            distinct.push(hit);
        }
    }
    distinct.len() % 2 == 1
}

/// `distanceToElement`.
pub fn distance_to_element(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> f64 {
    match element {
        Element::Diamond(_) => distance_to_diamond(geometry, element, point),
        Element::Ellipse(_) => distance_to_ellipse(geometry, element, point),
        Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_) => {
            let segments = geometry.linear_collision_shape(element);
            let mut min = f64::INFINITY;
            for segment in segments.iter() {
                let d = match segment {
                    Segment::Line(l) => distance_to_line_segment(point, *l),
                    Segment::Cubic(c) => curve_point_distance(*c, point),
                };
                min = min.min(d);
            }
            min
        }
        Element::Rectangle(_) | Element::Text(_) | Element::Image(_) | Element::Raw(_) => {
            distance_to_rectanguloid(geometry, element, point)
        }
    }
}

/// `hitElementItself`, without the frame-name label hit test (napkin has no frame-name
/// bounds cache yet).
pub fn hit_element_itself(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
    threshold: f64,
) -> bool {
    hit_element_itself_with(geometry, element, point, threshold, false)
}

/// [`hit_element_itself`] with `overrideShouldTestInside` exposed: `test_inside` forces the
/// "inside the shape or on its outline" test even for a shape [`should_test_inside`] would
/// only test on its outline.
pub fn hit_element_itself_with(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
    threshold: f64,
    test_inside: bool,
) -> bool {
    let Some((unrotated, center)) = geometry.absolute_coords(element) else {
        return false;
    };
    let Some(placement) = element.placement() else {
        return false;
    };

    // Fast reject: an exact equivalent of `isPointInRotatedBounds`, but against the
    // element's own unrotated bounds (rotating the query point the other way around the
    // same center) rather than JS's already-rotated `getElementBounds` box; both admit
    // exactly the query points the precise test below can possibly hit.
    let local = to_local_frame(point, center, placement);
    let [x1, y1, x2, y2] = unrotated;
    let in_bounds = local[0] >= x1 - threshold
        && local[0] <= x2 + threshold
        && local[1] >= y1 - threshold
        && local[1] <= y2 + threshold;
    if !in_bounds {
        return false;
    }

    if test_inside || should_test_inside(element) {
        is_point_in_element(geometry, element, point)
            || distance_to_element(geometry, element, point) <= threshold
    } else {
        distance_to_element(geometry, element, point) <= threshold
    }
}

// ---------------------------------------------------------------------------
// Segment/outline intersection and binding targets
// ---------------------------------------------------------------------------

/// `pointRotateRads`, including its early return of the same point for a zero angle (which
/// differs from the rotation formula by rounding in the last bit).
pub(crate) fn rotate_unless_zero(point: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    if angle == 0.0 {
        point
    } else {
        crate::geometry::rotate_point(point, center, angle)
    }
}

/// `doBoundsIntersect`: whether two boxes overlap, edges touching not counting.
fn do_bounds_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

/// `lineSegmentIntersectionPoints`: where the lines through `l` and `s` cross, when that
/// point lies on both segments (within `PRECISION`, `10e-5`).
pub(crate) fn line_segment_intersection_points(l: LineSeg, s: LineSeg) -> Option<[f64; 2]> {
    const PRECISION: f64 = 10e-5;
    let a1 = l[1][1] - l[0][1];
    let b1 = l[0][0] - l[1][0];
    let a2 = s[1][1] - s[0][1];
    let b2 = s[0][0] - s[1][0];
    let d = a1 * b2 - a2 * b1;
    if d == 0.0 {
        return None;
    }
    let c1 = a1 * l[0][0] + b1 * l[0][1];
    let c2 = a2 * s[0][0] + b2 * s[0][1];
    let candidate = [(c1 * b2 - c2 * b1) / d, (a1 * c2 - a2 * c1) / d];

    let on = |seg: LineSeg| distance_to_line_segment(candidate, seg) < PRECISION;
    (on(s) && on(l)).then_some(candidate)
}

/// `ellipseSegmentInterceptPoints`: the 0, 1 or 2 points where `s` crosses the ellipse with
/// the given center and half axes, in the order JS lists them.
fn ellipse_segment_intercept_points(
    center: [f64; 2],
    rx: f64,
    ry: f64,
    s: LineSeg,
) -> Vec<[f64; 2]> {
    let dir = [s[1][0] - s[0][0], s[1][1] - s[0][1]];
    let diff = [s[0][0] - center[0], s[0][1] - center[1]];
    let m_dir = [dir[0] / (rx * rx), dir[1] / (ry * ry)];
    let m_diff = [diff[0] / (rx * rx), diff[1] / (ry * ry)];

    let a = dir[0] * m_dir[0] + dir[1] * m_dir[1];
    let b = dir[0] * m_diff[0] + dir[1] * m_diff[1];
    let c = diff[0] * m_diff[0] + diff[1] * m_diff[1] - 1.0;
    let d = b * b - a * c;

    let at = |t: f64| {
        [
            s[0][0] + (s[1][0] - s[0][0]) * t,
            s[0][1] + (s[1][1] - s[0][1]) * t,
        ]
    };
    let mut hits = Vec::new();
    if d > 0.0 {
        let t_a = (-b - d.sqrt()) / a;
        let t_b = (-b + d.sqrt()) / a;
        if (0.0..=1.0).contains(&t_a) {
            hits.push(at(t_a));
        }
        if (0.0..=1.0).contains(&t_b) {
            hits.push(at(t_b));
        }
    } else if d == 0.0 {
        let t = -b / a;
        if (0.0..=1.0).contains(&t) {
            hits.push(at(t));
        }
    }
    hits
}

/// The sides and corner curves of a rectanguloid or diamond whose four rounded corners are
/// `base_corners`, with the outline grown by `offset` when it is positive: each corner is
/// replaced by the Catmull-Rom cubics through its curve moved `offset` along the normal, and
/// the sides join consecutive corners (`deconstructRectanguloidElement` and
/// `deconstructDiamondElement`).
fn offset_outline(base_corners: [CurveSeg; 4], offset: f64) -> (Vec<LineSeg>, Vec<CurveSeg>) {
    let corners: Vec<Vec<CurveSeg>> = base_corners
        .iter()
        .map(|&corner| {
            if offset > 0.0 {
                crate::curve::curve_catmull_rom_cubic_approx(&crate::curve::curve_offset_points(
                    corner, offset,
                ))
            } else {
                vec![corner]
            }
        })
        .collect();

    let sides = (0..4)
        .map(|i| {
            let from = corners[i].last().expect("a corner has at least one curve")[3];
            let to = corners[(i + 1) % 4][0][0];
            [from, to]
        })
        .collect();
    (sides, corners.into_iter().flatten().collect())
}

/// `lineIntersections` and `curveIntersections`: crossings of `sides` and `corners` (in the
/// element's unrotated frame) with `segment`, each rotated by `angle` about `center`.
fn outline_intersections(
    sides: &[LineSeg],
    corners: &[CurveSeg],
    segment: LineSeg,
    center: [f64; 2],
    angle: f64,
) -> Vec<[f64; 2]> {
    let mut hits = Vec::new();
    for &side in sides {
        if let Some(p) = line_segment_intersection_points(side, segment) {
            hits.push(rotate_unless_zero(p, center, angle));
        }
    }
    let segment_bounds = [
        segment[0][0].min(segment[1][0]),
        segment[0][1].min(segment[1][1]),
        segment[0][0].max(segment[1][0]),
        segment[0][1].max(segment[1][1]),
    ];
    for &corner in corners {
        let curve_bounds =
            crate::geometry::cubic_bezier_bounds(corner[0], corner[1], corner[2], corner[3]);
        if !do_bounds_intersect(curve_bounds, segment_bounds) {
            continue;
        }
        for p in crate::curve::curve_intersect_line_segment(corner, segment, 4) {
            hits.push(rotate_unless_zero(p, center, angle));
        }
    }
    hits
}

/// `intersectElementWithLineSegment(element, elementsMap, segment, offset)`: every point
/// where `segment` crosses `element`'s outline grown by `offset`, in the order JS returns
/// them (straight sides first, then curves).
pub fn intersect_element_with_line_segment(
    geometry: &mut GeometryCache,
    element: &Element,
    segment: [[f64; 2]; 2],
    offset: f64,
) -> Vec<[f64; 2]> {
    // Cheap reject against the (rotated) bounds first, as JS does.
    let intersector_bounds = [
        (segment[0][0] - offset).min(segment[1][0] - offset),
        (segment[0][1] - offset).min(segment[1][1] - offset),
        (segment[0][0] + offset).max(segment[1][0] + offset),
        (segment[0][1] + offset).max(segment[1][1] + offset),
    ];
    let Some(element_bounds) = geometry.bounds(element) else {
        return Vec::new();
    };
    if !do_bounds_intersect(intersector_bounds, element_bounds) {
        return Vec::new();
    }

    match element {
        Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_) => {
            intersect_linear(geometry, element, segment)
        }
        Element::Raw(_) if matches!(element.kind(), "line" | "arrow" | "freedraw") => Vec::new(),
        _ => {
            let (Some((_, center)), Some(placement)) =
                (geometry.absolute_coords(element), element.placement())
            else {
                return Vec::new();
            };
            let angle = placement.angle;
            let a = rotate_unless_zero(segment[0], center, -angle);
            let b = rotate_unless_zero(segment[1], center, -angle);

            match element {
                Element::Ellipse(_) => ellipse_segment_intercept_points(
                    center,
                    placement.width / 2.0 + offset,
                    placement.height / 2.0 + offset,
                    [a, b],
                )
                .into_iter()
                .map(|p| rotate_unless_zero(p, center, angle))
                .collect(),
                Element::Diamond(g) => {
                    let (_, corners) = deconstruct_diamond(placement, &g.base.roundness);
                    let (sides, corners) = offset_outline(corners, offset);
                    outline_intersections(&sides, &corners, [a, b], center, angle)
                }
                _ => {
                    let radius =
                        rectanguloid_radius(element, placement.width.min(placement.height));
                    let (_, corners) = deconstruct_rectanguloid(placement, radius);
                    let (sides, corners) = offset_outline(corners, offset);
                    outline_intersections(&sides, &corners, [a, b], center, angle)
                }
            }
        }
    }
}

/// `intersectLinearOrFreeDrawWithLineSegment`: the collision shape is already in scene
/// coordinates, so the segment is used as given.
fn intersect_linear(
    geometry: &mut GeometryCache,
    element: &Element,
    segment: LineSeg,
) -> Vec<[f64; 2]> {
    let shape = geometry.linear_collision_shape(element);
    let mut hits = Vec::new();
    for piece in shape.iter() {
        if let Segment::Line(l) = piece
            && let Some(p) = line_segment_intersection_points(*l, segment)
        {
            hits.push(p);
        }
    }
    let segment_bounds = [
        segment[0][0].min(segment[1][0]),
        segment[0][1].min(segment[1][1]),
        segment[0][0].max(segment[1][0]),
        segment[0][1].max(segment[1][1]),
    ];
    for piece in shape.iter() {
        let Segment::Cubic(c) = piece else { continue };
        let curve_bounds = crate::geometry::cubic_bezier_bounds(c[0], c[1], c[2], c[3]);
        if !do_bounds_intersect(curve_bounds, segment_bounds) {
            continue;
        }
        hits.extend(crate::curve::curve_intersect_line_segment(*c, segment, 10));
    }
    hits
}

/// `isBindableElement(element, false)`: an unlocked rectangle, diamond, ellipse, image, or a
/// text that is not a container's label. A `Raw` element is never a target.
pub fn is_bindable(element: &Element) -> bool {
    !element.is_locked() && is_bindable_including_locked(element)
}

/// `isBindableElement(element)` (its `includeLocked` default): [`is_bindable`] without the
/// `locked` check, which is what `updateBoundElements` applies to a changed element.
pub fn is_bindable_including_locked(element: &Element) -> bool {
    match element {
        Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_) | Element::Image(_) => {
            true
        }
        Element::Text(_) => element.container_id().is_none(),
        _ => false,
    }
}

/// The outline of a bindable shape as a closed polyline in scene coordinates, rotated with the
/// shape: straight sides contribute their ends, rounded corners eight chords each, an ellipse
/// 64 points. `None` for an element without a placement or that is not a rectangle, diamond,
/// ellipse, image or text.
pub fn outline_polyline(geometry: &mut GeometryCache, element: &Element) -> Option<Vec<[f64; 2]>> {
    const CORNER_CHORDS: usize = 8;
    const ELLIPSE_POINTS: usize = 64;
    let placement = element.placement()?;
    let (_, center) = geometry.absolute_coords(element)?;

    fn push_curve(points: &mut Vec<[f64; 2]>, curve: CurveSeg) {
        for step in 0..=CORNER_CHORDS {
            points.push(crate::curve::bezier_equation(
                curve,
                step as f64 / CORNER_CHORDS as f64,
            ));
        }
    }

    let mut points: Vec<[f64; 2]> = Vec::new();
    match element {
        Element::Ellipse(_) => {
            let rx = placement.width / 2.0;
            let ry = placement.height / 2.0;
            let (cx, cy) = (placement.x + rx, placement.y + ry);
            for step in 0..ELLIPSE_POINTS {
                let t = step as f64 / ELLIPSE_POINTS as f64 * std::f64::consts::TAU;
                points.push([cx + rx * t.cos(), cy + ry * t.sin()]);
            }
        }
        Element::Diamond(diamond) => {
            let (sides, corners) = deconstruct_diamond(placement, &diamond.base.roundness);
            // Corners run right, bottom, left, top; side `i` leads from corner `i` to `i + 1`.
            for i in 0..4 {
                push_curve(&mut points, corners[i]);
                points.extend(sides[i]);
            }
        }
        Element::Rectangle(_) | Element::Image(_) | Element::Text(_) => {
            let radius = rectanguloid_radius(element, placement.width.min(placement.height));
            let (sides, corners) = deconstruct_rectanguloid(placement, radius);
            let [top, right, bottom, left] = sides;
            let [top_left, top_right, bottom_right, bottom_left] = corners;
            points.extend(top);
            push_curve(&mut points, top_right);
            points.extend(right);
            push_curve(&mut points, bottom_right);
            points.extend([bottom[1], bottom[0]]);
            push_curve(&mut points, bottom_left);
            points.extend(left);
            push_curve(&mut points, top_left);
        }
        _ => return None,
    }
    if let Some(&first) = points.first() {
        points.push(first);
    }
    Some(
        points
            .into_iter()
            .map(|p| rotate_unless_zero(p, center, placement.angle))
            .collect(),
    )
}

/// `bindingBorderTest` for a shape (not a frame): whether `point` is inside the shape (no
/// crossing between the shape's center and the point) or within `tolerance` of its outline.
fn binding_border_test(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
    tolerance: f64,
) -> bool {
    // Cheap test first: the shape's rotated bounds against a box around the point.
    let t = tolerance.max(1.0);
    let probe = [point[0] - t, point[1] - t, point[0] + t, point[1] + t];
    let Some(bounds) = geometry.bounds(element) else {
        return false;
    };
    if !do_bounds_intersect(probe, bounds) {
        return false;
    }
    let Some((_, center)) = geometry.absolute_coords(element) else {
        return false;
    };

    let intersections =
        intersect_element_with_line_segment(geometry, element, [center, point], 0.0);
    let distance = distance_to_element(geometry, element, point);
    intersections.is_empty() || distance <= tolerance
}

/// `getHoveredElementForBinding(point, elements, elementsMap, tolerance)`: position of the
/// bindable element to bind to, front to back, stopping at the first one with a
/// non-transparent background, then preferring the smallest diagonal (the furthest back
/// among equals). Deleted elements are skipped.
pub fn hovered_element_for_binding(
    geometry: &mut GeometryCache,
    elements: &[Element],
    point: [f64; 2],
    tolerance: f64,
) -> Option<usize> {
    let mut candidates: Vec<usize> = Vec::new();
    for index in (0..elements.len()).rev() {
        let element = &elements[index];
        if element.is_deleted()
            || !is_bindable(element)
            || !binding_border_test(geometry, element, point, tolerance)
        {
            continue;
        }
        candidates.push(index);
        if has_background(element.kind())
            && !crate::color::is_transparent(background_color(element))
        {
            break;
        }
    }

    let diagonal_sq = |index: usize| {
        elements[index]
            .placement()
            .map_or(0.0, |p| p.width.powi(2) + p.height.powi(2))
    };
    // Largest first (a stable sort, like JS), so `pop` takes the smallest.
    candidates.sort_by(|&a, &b| diagonal_sq(b).total_cmp(&diagonal_sq(a)));
    candidates.pop()
}

// ---------------------------------------------------------------------------
// Segment hit testing (the eraser: `eraser/index.ts`'s `eraserTest`)
// ---------------------------------------------------------------------------

/// Whether two straight segments cross, without computing the crossing point itself
/// (`segmentsIntersectAt`'s existence check): the standard 2D parametric line test, requiring
/// both `t` and `u` in `[0, 1)`. Parallel or collinear segments (`denominator == 0`) never
/// count, matching `segmentsIntersectAt`'s own `null` return for that case.
fn segments_intersect(a: LineSeg, b: LineSeg) -> bool {
    let (rx, ry) = (a[1][0] - a[0][0], a[1][1] - a[0][1]);
    let (sx, sy) = (b[1][0] - b[0][0], b[1][1] - b[0][1]);
    let denominator = rx * sy - ry * sx;
    if denominator == 0.0 {
        return false;
    }
    let (ix, iy) = (b[0][0] - a[0][0], b[0][1] - a[0][1]);
    let u = (ix * ry - iy * rx) / denominator;
    let t = (ix * sy - iy * sx) / denominator;
    (0.0..1.0).contains(&t) && (0.0..1.0).contains(&u) && u != 0.0
}

/// `lineSegmentsDistance`: 0 when the segments cross, else the minimum of each endpoint's
/// distance to the other segment.
fn segments_distance(a: LineSeg, b: LineSeg) -> f64 {
    if segments_intersect(a, b) {
        return 0.0;
    }
    distance_to_line_segment(a[0], b)
        .min(distance_to_line_segment(a[1], b))
        .min(distance_to_line_segment(b[0], a))
        .min(distance_to_line_segment(b[1], a))
}

/// The minimum distance between `seg` and a cubic outline piece, flattened into 16 straight
/// sub-segments (mirroring [`is_point_in_linear`]'s own flattening) since there is no closed
/// form for a segment-to-Bezier distance.
fn segment_to_curve_distance(seg: LineSeg, c: CurveSeg) -> f64 {
    const SUBDIVISIONS: usize = 16;
    let mut min = f64::INFINITY;
    let mut prev = c[0];
    for i in 1..=SUBDIVISIONS {
        let p = bezier_point(c, i as f64 / SUBDIVISIONS as f64);
        min = min.min(segments_distance(seg, [prev, p]));
        prev = p;
    }
    min
}

/// The minimum distance between `seg` and an outline built from straight `sides` and curved
/// `corners`, both already in the same frame as `seg` (rectanguloid and diamond deconstruction).
fn segment_to_sides_and_corners(seg: LineSeg, sides: &[LineSeg], corners: &[CurveSeg]) -> f64 {
    let mut min = f64::INFINITY;
    for &side in sides {
        min = min.min(segments_distance(seg, side));
    }
    for &corner in corners {
        min = min.min(segment_to_curve_distance(seg, corner));
    }
    min
}

/// An ellipse's boundary approximated as a 32-sided polygon, in the element's own local frame:
/// unlike a rectanguloid or diamond, an ellipse's outline has no finite Bezier decomposition to
/// reuse here, and the eraser has no need for sub-pixel precision.
fn segment_to_ellipse_distance(seg: LineSeg, placement: Placement) -> f64 {
    const SIDES: usize = 32;
    let (hw, hh) = (placement.width / 2.0, placement.height / 2.0);
    let (cx, cy) = (placement.x + hw, placement.y + hh);
    let point_at = |i: usize| {
        let theta = i as f64 / SIDES as f64 * std::f64::consts::TAU;
        [cx + hw * theta.cos(), cy + hh * theta.sin()]
    };
    let mut min = f64::INFINITY;
    let mut prev = point_at(0);
    for i in 1..=SIDES {
        let p = point_at(i);
        min = min.min(segments_distance(seg, [prev, p]));
        prev = p;
    }
    min
}

/// The minimum distance between `seg` (in scene coordinates) and `element`'s outline.
fn segment_distance_to_element(
    geometry: &mut GeometryCache,
    element: &Element,
    from: [f64; 2],
    to: [f64; 2],
) -> f64 {
    match element {
        Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_) => {
            let pieces = geometry.linear_collision_shape(element);
            let seg = [from, to];
            let mut min = f64::INFINITY;
            for piece in pieces.iter() {
                let d = match piece {
                    Segment::Line(l) => segments_distance(seg, *l),
                    Segment::Cubic(c) => segment_to_curve_distance(seg, *c),
                };
                min = min.min(d);
            }
            min
        }
        Element::Diamond(g) => {
            let Some((_, center)) = geometry.absolute_coords(element) else {
                return f64::INFINITY;
            };
            let Some(placement) = element.placement() else {
                return f64::INFINITY;
            };
            let seg = [
                to_local_frame(from, center, placement),
                to_local_frame(to, center, placement),
            ];
            let (sides, corners) = deconstruct_diamond(placement, &g.base.roundness);
            segment_to_sides_and_corners(seg, &sides, &corners)
        }
        Element::Ellipse(_) => {
            let Some((_, center)) = geometry.absolute_coords(element) else {
                return f64::INFINITY;
            };
            let Some(placement) = element.placement() else {
                return f64::INFINITY;
            };
            let seg = [
                to_local_frame(from, center, placement),
                to_local_frame(to, center, placement),
            ];
            segment_to_ellipse_distance(seg, placement)
        }
        Element::Rectangle(_) | Element::Text(_) | Element::Image(_) | Element::Raw(_) => {
            let Some((_, center)) = geometry.absolute_coords(element) else {
                return f64::INFINITY;
            };
            let Some(placement) = element.placement() else {
                return f64::INFINITY;
            };
            let seg = [
                to_local_frame(from, center, placement),
                to_local_frame(to, center, placement),
            ];
            let radius = rectanguloid_radius(element, placement.width.min(placement.height));
            let (sides, corners) = deconstruct_rectanguloid(placement, radius);
            segment_to_sides_and_corners(seg, &sides, &corners)
        }
    }
}

/// Whether the eraser segment from `from` to `to` touches `element`: either endpoint inside it
/// (`shouldTestInside`; both ends are tested here rather than only `eraserTest`'s single
/// `lastPoint`, so a segment that starts inside a filled shape and immediately leaves it still
/// counts), or its outline within `threshold` (`eraserTest`'s `lineSegmentsDistance` calls,
/// generalized here to every element kind rather than only freedraw/arrow/open-line, where JS
/// uses `intersectElementWithLineSegment` for the rest).
pub fn segment_hits_element(
    geometry: &mut GeometryCache,
    element: &Element,
    from: [f64; 2],
    to: [f64; 2],
    threshold: f64,
) -> bool {
    // Cheap reject first (`doBoundsIntersect`, checked before `eraserTest` runs any real
    // geometry): both the segment's own bounds and the element's rotated bounds padded by
    // `threshold`, so this can only ever reject a query the precise test below would too.
    let Some(element_bounds) = geometry.bounds(element) else {
        return false;
    };
    let segment_bounds = [
        from[0].min(to[0]) - threshold,
        from[1].min(to[1]) - threshold,
        from[0].max(to[0]) + threshold,
        from[1].max(to[1]) + threshold,
    ];
    let padded_element_bounds = [
        element_bounds[0] - threshold,
        element_bounds[1] - threshold,
        element_bounds[2] + threshold,
        element_bounds[3] + threshold,
    ];
    if !bounds_intersect(segment_bounds, padded_element_bounds) {
        return false;
    }

    if should_test_inside(element)
        && (is_point_in_element(geometry, element, from)
            || is_point_in_element(geometry, element, to))
    {
        return true;
    }
    segment_distance_to_element(geometry, element, from, to) <= threshold
}

/// `doBoundsIntersect`: whether two axis-aligned boxes overlap.
fn bounds_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use serde_json::json;

    use super::*;
    use crate::sample;

    #[test]
    fn a_point_exactly_on_the_outline_is_not_inside_when_testing_with_no_threshold() {
        // `isPointInElement` counts ray crossings, and a ray starting on the outline does not
        // cross it an odd number of times; with threshold 0 the rounding in the distance to
        // the side leaves it just off the outline too.
        let element =
            Element::from_value(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]));
        let mut geometry = GeometryCache::default();
        assert!(!hit_element_itself_with(
            &mut geometry,
            &element,
            [100.0, 50.01],
            0.0,
            true
        ));
        assert!(hit_element_itself_with(
            &mut geometry,
            &element,
            [99.99999, 50.01],
            0.0,
            true
        ));
        assert!(hit_element_itself_with(
            &mut geometry,
            &element,
            [100.0, 50.01],
            0.001,
            true
        ));
    }

    fn hit(element: &Element, point: [f64; 2], zoom: f64) -> bool {
        let mut geometry = GeometryCache::default();
        hit_element_itself(&mut geometry, element, point, hit_threshold(element, zoom))
    }

    fn rect(background: &str) -> Element {
        Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"backgroundColor": background}),
        ))
    }

    fn polyline(kind: &str, background: &str, points: &[[f64; 2]]) -> Element {
        Element::from_value(sample::with(
            sample::linear(kind, "l", [0.0, 0.0], points),
            json!({"backgroundColor": background, "roughness": 0, "roundness": null}),
        ))
    }

    const SQUARE: [[f64; 2]; 5] = [
        [0.0, 0.0],
        [100.0, 0.0],
        [100.0, 100.0],
        [0.0, 100.0],
        [0.0, 0.0],
    ];

    #[test]
    fn threshold_scales_with_zoom_and_stroke_width() {
        let r = rect("transparent");
        assert!((hit_threshold(&r, 1.0) - 0.85 * DEFAULT_COLLISION_THRESHOLD).abs() < 1e-12);
        assert!((hit_threshold(&r, 4.0) - 0.85 * DEFAULT_COLLISION_THRESHOLD / 4.0).abs() < 1e-12);
        let thick = Element::from_value(sample::with(
            sample::generic("rectangle", "t", [0.0, 0.0, 10.0, 10.0]),
            json!({"strokeWidth": 20}),
        ));
        assert!((hit_threshold(&thick, 1.0) - 10.1).abs() < 1e-12);
    }

    #[test]
    fn transparent_shapes_hit_only_near_the_outline() {
        let r = rect("transparent");
        assert!(hit(&r, [50.0, 0.0], 1.0));
        assert!(hit(&r, [50.0, -5.0], 1.0));
        assert!(!hit(&r, [50.0, -10.0], 1.0));
        assert!(!hit(&r, [50.0, 50.0], 1.0));
        assert!(!hit(&r, [50.0, -5.0], 4.0));
        assert!(hit(&rect("#ffc9c9"), [50.0, 50.0], 1.0));

        let ellipse = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]));
        assert!(hit(&ellipse, [50.0, 1.0], 1.0));
        assert!(hit(&ellipse, [100.0, 25.0], 1.0));
        assert!(!hit(&ellipse, [50.0, 25.0], 1.0));

        let diamond =
            Element::from_value(sample::generic("diamond", "d", [0.0, 0.0, 100.0, 100.0]));
        assert!(hit(&diamond, [75.0, 25.0], 1.0));
        assert!(!hit(&diamond, [50.0, 50.0], 1.0));
    }

    #[test]
    fn inside_rules_follow_should_test_inside() {
        assert!(hit(
            &polyline("line", "#ffc9c9", &SQUARE),
            [50.0, 50.0],
            1.0
        ));
        assert!(!hit(
            &polyline("line", "transparent", &SQUARE),
            [50.0, 50.0],
            1.0
        ));
        assert!(!hit(
            &polyline("arrow", "#ffc9c9", &SQUARE),
            [50.0, 50.0],
            1.0
        ));
        assert!(hit(
            &polyline("arrow", "#ffc9c9", &SQUARE),
            [50.0, 3.0],
            1.0
        ));
        // An open line never has an inside.
        let open = polyline("line", "#ffc9c9", &SQUARE[..4]);
        assert!(!hit(&open, [50.0, 50.0], 1.0));

        let text = Element::from_value(sample::text("t", [0.0, 0.0, 40.0, 25.0], "hi", None));
        assert!(hit(&text, [20.0, 12.0], 1.0));

        let labelled = Element::from_value(sample::with(
            sample::generic("rectangle", "c", [0.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ));
        assert!(hit(&labelled, [50.0, 50.0], 1.0));
    }

    #[test]
    fn raw_elements_hit_like_their_excalidraw_types() {
        let raw = |kind: &str| {
            Element::from_value(
                json!({"id": "x", "type": kind, "x": 0, "y": 0, "width": 100,
                "height": 100, "angle": 0, "strokeWidth": 2, "backgroundColor": "transparent",
                "version": 1, "versionNonce": 1}),
            )
        };
        assert!(hit(&raw("image"), [50.0, 50.0], 1.0));
        assert!(hit(&raw("embeddable"), [50.0, 50.0], 1.0));
        assert!(!hit(&raw("frame"), [50.0, 50.0], 1.0));
        assert!(hit(&raw("frame"), [50.0, 1.0], 1.0));
    }

    #[test]
    fn rotation_is_applied_to_the_query_point() {
        let rotated = Element::from_value(sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 20.0]),
            json!({"angle": FRAC_PI_2}),
        ));
        assert!(hit(&rotated, [40.0, 30.0], 1.0));
        assert!(!hit(&rotated, [90.0, 10.0], 1.0));
        assert!(is_path_a_loop(&SQUARE, 1.0));
        assert!(!is_path_a_loop(&SQUARE[..3], 1.0));
    }

    #[test]
    fn segment_meets_a_rectangle_grown_by_the_offset() {
        let rect = Element::from_value(sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]));
        let hits = intersect_element_with_line_segment(
            &mut GeometryCache::default(),
            &rect,
            [[-20.0, 25.0], [50.0, 25.0]],
            5.0,
        );
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(
            (hits[0][0] + 5.0).abs() < 1e-9 && (hits[0][1] - 25.0).abs() < 1e-9,
            "{hits:?}"
        );
    }

    #[test]
    fn segment_through_an_ellipse_hits_both_sides() {
        let ellipse = Element::from_value(sample::generic("ellipse", "e", [0.0, 0.0, 100.0, 50.0]));
        let mut hits = intersect_element_with_line_segment(
            &mut GeometryCache::default(),
            &ellipse,
            [[-10.0, 25.0], [110.0, 25.0]],
            0.0,
        );
        hits.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert!(
            (hits[0][0]).abs() < 1e-9 && (hits[1][0] - 100.0).abs() < 1e-9,
            "{hits:?}"
        );
    }

    #[test]
    fn a_filled_shape_stops_the_search_behind_it() {
        // A big transparent rectangle under a small filled one: the point is inside both, and
        // the filled one is in front, so it wins without looking further back.
        let elements = vec![
            Element::from_value(sample::generic(
                "rectangle",
                "big",
                [0.0, 0.0, 300.0, 300.0],
            )),
            Element::from_value(sample::with(
                sample::generic("rectangle", "small", [100.0, 100.0, 50.0, 50.0]),
                json!({"backgroundColor": "#ffc9c9"}),
            )),
        ];
        let hit = hovered_element_for_binding(
            &mut GeometryCache::default(),
            &elements,
            [120.0, 120.0],
            15.0,
        );
        assert_eq!(hit, Some(1));
    }
}
