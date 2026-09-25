//! Point hit testing: whether a point lies on or inside an element's outline
//! (`packages/element/src/collision.ts`, `packages/element/src/distance.ts` and the
//! `deconstruct*`/`isPathALoop`/`getCornerRadius` helpers of `packages/element/src/utils.ts`,
//! all at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`).
//!
//! The outline tests (`distance_to_element`, and `linear_collision_shape` for line/arrow/
//! freedraw) port the cited functions directly: rectanguloid and diamond corners stay exact
//! cubic Beziers, and ellipse distance keeps the three-iteration Newton-ish approximation
//! `ellipseDistanceFromPoint` uses. The "is this point inside the shape" tests
//! (`is_point_in_element`) do not port `isPointInElement`'s ray-cast-and-count-intersections
//! approach; instead they use an equivalent analytic containment test in the element's own
//! unrotated frame (a closed-form inside/outside check for rectanguloid, diamond and
//! ellipse, and an even-odd ray cast against a 16-segment polyline approximation of the
//! cubic collision shape for a closed line/freedraw). This avoids porting
//! `curveIntersectLineSegment`'s Newton solver, which only `isPointInElement` and the
//! (out of scope for point hit testing) binding code need.

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
// Rectanguloid (rectangle, text, and any `Raw` element regardless of its own `type`, exactly
// as `distanceToElement`'s switch maps image/frame/embeddable/stickynote/iframe/magicframe to
// `distanceToRectanguloidElement` as well: none of those have a napkin `Element` variant of
// their own, so this arm covers them uniformly)
// ---------------------------------------------------------------------------

/// `getCornerRadius(Math.min(width, height), element)`, then `deconstructRectanguloidElement`'s
/// `if (radius === 0) radius = 0.01`. A `Rectangle`'s own `roundness` applies; every other
/// element this is called for (text, `Raw`) has none.
fn rectanguloid_radius(element: &Element, min_dim: f64) -> f64 {
    let none = Slot::Missing;
    let roundness = match element {
        Element::Rectangle(g) => &g.base.roundness,
        _ => &none,
    };
    let r = crate::shape::corner_radius(min_dim, roundness);
    if r == 0.0 { 0.01 } else { r }
}

/// `deconstructRectanguloidElement` with `offset` fixed at 0: the only other caller in
/// Excalidraw is binding, which this task does not port. Returns the 4 straight sides and
/// the 4 rounded-corner curves, in the element's own unrotated coordinate frame.
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
fn deconstruct_diamond(
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

/// `isPointInElement`'s equivalent for a diamond: a plain (unrounded) point-in-quadrilateral
/// test against the 4 diamond vertices, via the sign of the cross product against each edge
/// (a convex quadrilateral, so a consistent sign on every edge means "inside"). Corner
/// rounding is not modelled here: it only changes the classification for points within
/// [`diamond_radius`] of a vertex, an area the outline test (`distance_to_element`, which
/// does use the exact rounded corners) already covers via `hit_element_itself`'s
/// `inside || outline`.
fn is_point_in_diamond(placement: Placement, local: [f64; 2]) -> bool {
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
    let lx = local[0] - placement.x;
    let ly = local[1] - placement.y;
    let verts = [
        [top_x, top_y],
        [right_x, right_y],
        [bottom_x, bottom_y],
        [left_x, left_y],
    ];

    let mut sign = 0.0;
    for i in 0..4 {
        let a = verts[i];
        let b = verts[(i + 1) % 4];
        let cross = (b[0] - a[0]) * (ly - a[1]) - (b[1] - a[1]) * (lx - a[0]);
        if cross == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
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

/// `ellipseIncludesPoint`.
fn is_point_in_ellipse(placement: Placement, local: [f64; 2]) -> bool {
    let hw = placement.width / 2.0;
    let hh = placement.height / 2.0;
    if hw == 0.0 || hh == 0.0 {
        return false;
    }
    let cx = placement.x + hw;
    let cy = placement.y + hh;
    let nx = (local[0] - cx) / hw;
    let ny = (local[1] - cy) / hh;
    nx * nx + ny * ny <= 1.0
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

/// Whether `point` lies inside the element's closed outline (`isPointInElement`).
pub fn is_point_in_element(
    geometry: &mut GeometryCache,
    element: &Element,
    point: [f64; 2],
) -> bool {
    match element {
        Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_) => {
            is_point_in_linear(geometry, element, point)
        }
        Element::Diamond(_) => {
            let Some((_, center)) = geometry.absolute_coords(element) else {
                return false;
            };
            let Some(placement) = element.placement() else {
                return false;
            };
            is_point_in_diamond(placement, to_local_frame(point, center, placement))
        }
        Element::Ellipse(_) => {
            let Some((_, center)) = geometry.absolute_coords(element) else {
                return false;
            };
            let Some(placement) = element.placement() else {
                return false;
            };
            is_point_in_ellipse(placement, to_local_frame(point, center, placement))
        }
        Element::Rectangle(_) | Element::Text(_) | Element::Raw(_) => {
            is_point_in_rectanguloid(geometry, element, point)
        }
    }
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
        Element::Rectangle(_) | Element::Text(_) | Element::Raw(_) => {
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

    if should_test_inside(element) {
        is_point_in_element(geometry, element, point)
            || distance_to_element(geometry, element, point) <= threshold
    } else {
        distance_to_element(geometry, element, point) <= threshold
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use serde_json::json;

    use super::*;
    use crate::sample;

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
}
