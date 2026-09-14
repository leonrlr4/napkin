//! Port of `points-on-curve@0.2.0`: `lib/index.js`, `lib/curve-to-bezier.js`.

/// `lib/index.js` `distance`: distance between 2 points.
fn distance(p1: [f64; 2], p2: [f64; 2]) -> f64 {
    distance_sq(p1, p2).sqrt()
}

/// `lib/index.js` `distanceSq`: distance between 2 points squared.
fn distance_sq(p1: [f64; 2], p2: [f64; 2]) -> f64 {
    (p1[0] - p2[0]).powf(2.0) + (p1[1] - p2[1]).powf(2.0)
}

/// `lib/index.js` `distanceToSegmentSq`: distance squared from a point `p` to the line
/// segment `vw`.
fn distance_to_segment_sq(p: [f64; 2], v: [f64; 2], w: [f64; 2]) -> f64 {
    let l2 = distance_sq(v, w);
    if l2 == 0.0 {
        return distance_sq(p, v);
    }
    let t = ((p[0] - v[0]) * (w[0] - v[0]) + (p[1] - v[1]) * (w[1] - v[1])) / l2;
    let t = t.clamp(0.0, 1.0);
    distance_sq(p, lerp(v, w, t))
}

/// `lib/index.js` `lerp`.
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// `lib/index.js` `flatness`. Adapted from
/// <https://seant23.wordpress.com/2010/11/12/offset-bezier-curves/>.
fn flatness(points: &[[f64; 2]], offset: usize) -> f64 {
    let p1 = points[offset];
    let p2 = points[offset + 1];
    let p3 = points[offset + 2];
    let p4 = points[offset + 3];
    let mut ux = 3.0 * p2[0] - 2.0 * p1[0] - p4[0];
    ux *= ux;
    let mut uy = 3.0 * p2[1] - 2.0 * p1[1] - p4[1];
    uy *= uy;
    let mut vx = 3.0 * p3[0] - 2.0 * p4[0] - p1[0];
    vx *= vx;
    let mut vy = 3.0 * p3[1] - 2.0 * p4[1] - p1[1];
    vy *= vy;
    if ux < vx {
        ux = vx;
    }
    if uy < vy {
        uy = vy;
    }
    ux + uy
}

/// `lib/index.js` `getPointsOnBezierCurveWithSplitting`. JS threads the accumulator through
/// an optional `newPoints` parameter defaulting to `[]`; every call site here already has an
/// accumulator in hand, so it is always `&mut Vec`.
fn get_points_on_bezier_curve_with_splitting(
    points: &[[f64; 2]],
    offset: usize,
    tolerance: f64,
    out_points: &mut Vec<[f64; 2]>,
) {
    if flatness(points, offset) < tolerance {
        let p0 = points[offset];
        if !out_points.is_empty() {
            let d = distance(out_points[out_points.len() - 1], p0);
            if d > 1.0 {
                out_points.push(p0);
            }
        } else {
            out_points.push(p0);
        }
        out_points.push(points[offset + 3]);
    } else {
        // subdivide
        let t = 0.5;
        let p1 = points[offset];
        let p2 = points[offset + 1];
        let p3 = points[offset + 2];
        let p4 = points[offset + 3];
        let q1 = lerp(p1, p2, t);
        let q2 = lerp(p2, p3, t);
        let q3 = lerp(p3, p4, t);
        let r1 = lerp(q1, q2, t);
        let r2 = lerp(q2, q3, t);
        let red = lerp(r1, r2, t);
        get_points_on_bezier_curve_with_splitting(&[p1, q1, r1, red], 0, tolerance, out_points);
        get_points_on_bezier_curve_with_splitting(&[red, r2, q3, p4], 0, tolerance, out_points);
    }
}

/// `lib/index.js` `simplify`.
pub fn simplify(points: &[[f64; 2]], distance: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    simplify_points(points, 0, points.len(), distance, &mut out);
    out
}

/// `lib/index.js` `simplifyPoints`: Ramer-Douglas-Peucker algorithm,
/// <https://en.wikipedia.org/wiki/Ramer%E2%80%93Douglas%E2%80%93Peucker_algorithm>.
fn simplify_points(
    points: &[[f64; 2]],
    start: usize,
    end: usize,
    epsilon: f64,
    out_points: &mut Vec<[f64; 2]>,
) {
    // find the most distance point from the endpoints
    let s = points[start];
    let e = points[end - 1];
    let mut max_dist_sq = 0.0;
    let mut max_ndx = 1; // JS: initialised to 1, not `start + 1`
    for (offset, &p) in points[(start + 1)..(end - 1)].iter().enumerate() {
        let i = start + 1 + offset;
        let dist_sq = distance_to_segment_sq(p, s, e);
        if dist_sq > max_dist_sq {
            max_dist_sq = dist_sq;
            max_ndx = i;
        }
    }
    // if that point is too far, split
    if max_dist_sq.sqrt() > epsilon {
        simplify_points(points, start, max_ndx + 1, epsilon, out_points);
        simplify_points(points, max_ndx, end, epsilon, out_points);
    } else if out_points.is_empty() {
        out_points.push(s);
        out_points.push(e);
    } else {
        out_points.push(e);
    }
}

/// `lib/index.js` `pointsOnBezierCurves`.
pub fn points_on_bezier_curves(
    points: &[[f64; 2]],
    tolerance: f64,
    distance: Option<f64>,
) -> Vec<[f64; 2]> {
    let mut new_points = Vec::new();
    let num_segments = (points.len() as f64 - 1.0) / 3.0;
    let mut i = 0.0;
    while i < num_segments {
        let offset = (i * 3.0) as usize;
        get_points_on_bezier_curve_with_splitting(points, offset, tolerance, &mut new_points);
        i += 1.0;
    }
    if distance.is_some_and(|d| d > 0.0) {
        let mut out = Vec::new();
        simplify_points(
            &new_points,
            0,
            new_points.len(),
            distance.expect("checked by is_some_and above"),
            &mut out,
        );
        return out;
    }
    new_points
}

/// `lib/curve-to-bezier.js` `curveToBezier`. Returns `None` where the JS throws (fewer than
/// three points).
pub fn curve_to_bezier(points_in: &[[f64; 2]], curve_tightness: f64) -> Option<Vec<[f64; 2]>> {
    let len = points_in.len();
    if len < 3 {
        return None;
    }
    let mut out = Vec::new();
    if len == 3 {
        out.push(points_in[0]);
        out.push(points_in[1]);
        out.push(points_in[2]);
        out.push(points_in[2]);
    } else {
        let mut points = Vec::new();
        points.push(points_in[0]);
        points.push(points_in[0]);
        for (i, p) in points_in.iter().enumerate().skip(1) {
            points.push(*p);
            if i == points_in.len() - 1 {
                points.push(*p);
            }
        }
        let s = 1.0 - curve_tightness;
        out.push(points[0]);
        let mut i = 1;
        while i + 2 < points.len() {
            let cached_vert_array = points[i];
            // b[0] in the JS is assigned but never read; not ported.
            let b1 = [
                cached_vert_array[0] + (s * points[i + 1][0] - s * points[i - 1][0]) / 6.0,
                cached_vert_array[1] + (s * points[i + 1][1] - s * points[i - 1][1]) / 6.0,
            ];
            let b2 = [
                points[i + 1][0] + (s * points[i][0] - s * points[i + 2][0]) / 6.0,
                points[i + 1][1] + (s * points[i][1] - s * points[i + 2][1]) / 6.0,
            ];
            let b3 = [points[i + 1][0], points[i + 1][1]];
            out.push(b1);
            out.push(b2);
            out.push(b3);
            i += 1;
        }
    }
    Some(out)
}
