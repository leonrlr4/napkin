//! Port of perfect-freehand 1.2.0's `getStroke` (`getStroke.ts`, `getStrokePoints.ts`,
//! `getStrokeOutlinePoints.ts`, `getStrokeRadius.ts`, `vec.ts`, tag `v1.2.0`), checked
//! against the minified build the baseline generator actually runs
//! (`tools/baseline/node_modules/perfect-freehand/dist/esm/index.js`); the two agreed on
//! every branch this port reaches, so there is nothing to note where they'd differ.
//!
//! Only the option set `getVariableWidthFreedrawOutline` passes is supported:
//! `start`/`end` are never given, so `start.taper`/`end.taper` are always `undefined`
//! (`taperStart`/`taperEnd` in the source) and `start.cap`/`end.cap` default to `true`.
//! Under that precondition:
//! - the taper strength factors (`ts`/`te` in the source) are always 1, since comparing a
//!   length against an `undefined` taper distance is always false in JS;
//! - the "tapered, no cap" and flat-cap (`cap: false`) branches of `getStrokeOutlinePoints`
//!   never run.
//!
//! Neither branch is reachable from `shape::freedraw`, so neither is ported; the code below
//! only implements the paths Excalidraw's fixed options can take, with comments at each spot
//! a skipped branch would have been.

const RATE_OF_PRESSURE_CHANGE: f64 = 0.275;
/// "Browser strokes seem to be off if PI is regular, a tiny offset seems to fix it" (source
/// comment on `FIXED_PI`).
const FIXED_PI: f64 = std::f64::consts::PI + 1e-4;

/// The options `getVariableWidthFreedrawOutline` builds. `start`/`end` have no fields here
/// because Excalidraw never passes them (see module docs).
pub(crate) struct Options {
    pub size: f64,
    pub thinning: f64,
    pub smoothing: f64,
    pub streamline: f64,
    pub simulate_pressure: bool,
    pub easing: fn(f64) -> f64,
    pub last: bool,
}

struct StrokePoint {
    point: [f64; 2],
    pressure: f64,
    vector: [f64; 2],
    distance: f64,
    running_length: f64,
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: [f64; 2], n: f64) -> [f64; 2] {
    [a[0] * n, a[1] * n]
}

fn per(a: [f64; 2]) -> [f64; 2] {
    [a[1], -a[0]]
}

fn dpr(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

fn uni(a: [f64; 2]) -> [f64; 2] {
    let l = len(a);
    [a[0] / l, a[1] / l]
}

/// `dist`: note the argument order inside `hypot`, `(A[1]-B[1], A[0]-B[0])` and not
/// `(A[0]-B[0], A[1]-B[1])`; harmless since `hypot` is symmetric, kept for a literal port.
fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[1] - b[1]).hypot(a[0] - b[0])
}

/// `dist2`: distance *squared* (`len2(sub(A,B))`), unrelated to [`dist`].
fn dist_sq(a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1]
}

fn rot_around(a: [f64; 2], c: [f64; 2], r: f64) -> [f64; 2] {
    let s = r.sin();
    let cs = r.cos();
    let px = a[0] - c[0];
    let py = a[1] - c[1];
    let nx = px * cs - py * s;
    let ny = px * s + py * cs;
    [nx + c[0], ny + c[1]]
}

fn lrp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    add(a, mul(sub(b, a), t))
}

fn prj(a: [f64; 2], b: [f64; 2], c: f64) -> [f64; 2] {
    add(a, mul(b, c))
}

fn neg(a: [f64; 2]) -> [f64; 2] {
    [-a[0], -a[1]]
}

/// `A||B` for the one spot the source relies on JS number truthiness (`U||m`, `getStroke
/// OutlinePoints`): falsy for `0` and `NaN`, not just "absent". Plain `Option::unwrap_or`
/// would miss the `Some(0.0)` case.
fn js_or(a: Option<f64>, b: f64) -> f64 {
    match a {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => b,
    }
}

fn stroke_radius(size: f64, thinning: f64, pressure: f64, easing: fn(f64) -> f64) -> f64 {
    size * easing(0.5 - thinning * (0.5 - pressure))
}

/// `getStrokePoints`. The source accepts `[x, y]` or `[x, y, pressure]` input (and an
/// object form napkin never needs, since `shape.ts` always calls `getStroke` with
/// `number[][]`); here the caller always supplies `[x, y, pressure]`, using `f64::NAN` for
/// JS's missing/`undefined` pressure. `pts[i][2] >= 0` is `false` for `NaN` exactly as it is
/// for `undefined` in JS, so the fallback-pressure logic below needs no special-casing.
fn get_stroke_points(
    points: &[[f64; 3]],
    streamline: f64,
    size: f64,
    last: bool,
) -> Vec<StrokePoint> {
    if points.is_empty() {
        return Vec::new();
    }

    let t = 0.15 + (1.0 - streamline) * 0.85;

    let mut pts: Vec<[f64; 3]> = points.to_vec();

    // Add extra points between the two, to help avoid "dash" lines for strokes with
    // tapered start and ends. The interpolation (`lrp`) here is perfect-freehand's 2D
    // `vec.ts` version, which drops any third (pressure) component: the four points this
    // produces carry `NaN` pressure below, exactly like the JS `number[]` triples that come
    // out one element short.
    if pts.len() == 2 {
        let last_point = pts[1];
        pts.truncate(1);
        for i in 1..5 {
            let xy = lrp(
                [pts[0][0], pts[0][1]],
                [last_point[0], last_point[1]],
                i as f64 / 4.0,
            );
            pts.push([xy[0], xy[1], f64::NAN]);
        }
    }

    // If there's only one point, add another point at a 1pt offset (keeping whatever
    // pressure, real or NaN, the first point carried).
    if pts.len() == 1 {
        let p0 = pts[0];
        pts.push([p0[0] + 1.0, p0[1] + 1.0, p0[2]]);
    }

    let mut stroke_points = vec![StrokePoint {
        point: [pts[0][0], pts[0][1]],
        pressure: if pts[0][2] >= 0.0 { pts[0][2] } else { 0.25 },
        vector: [1.0, 1.0],
        distance: 0.0,
        running_length: 0.0,
    }];

    let mut has_reached_minimum_length = false;
    let mut running_length = 0.0;
    let max = pts.len() - 1;

    for (i, p) in pts.iter().enumerate().skip(1) {
        let prev_point = stroke_points.last().expect("seeded above").point;
        let point = if last && i == max {
            [p[0], p[1]]
        } else {
            lrp(prev_point, [p[0], p[1]], t)
        };

        if point == prev_point {
            continue;
        }

        let distance = dist(point, prev_point);
        running_length += distance;

        if i < max && !has_reached_minimum_length {
            if running_length < size {
                continue;
            }
            has_reached_minimum_length = true;
        }

        stroke_points.push(StrokePoint {
            point,
            pressure: if p[2] >= 0.0 { p[2] } else { 0.5 },
            vector: uni(sub(prev_point, point)),
            distance,
            running_length,
        });
    }

    let second_vector = stroke_points
        .get(1)
        .map(|sp| sp.vector)
        .unwrap_or([0.0, 0.0]);
    stroke_points[0].vector = second_vector;

    stroke_points
}

/// `getStrokeOutlinePoints`. See the module docs for the taper/flat-cap branches this
/// does not port, and why they are unreachable from `shape::freedraw`.
fn get_stroke_outline_points(points: &[StrokePoint], options: &Options) -> Vec<[f64; 2]> {
    if points.is_empty() || options.size <= 0.0 {
        return Vec::new();
    }

    let total_length = points.last().expect("non-empty").running_length;
    let min_distance = (options.size * options.smoothing).powi(2);

    let mut left_pts: Vec<[f64; 2]> = Vec::new();
    let mut right_pts: Vec<[f64; 2]> = Vec::new();

    let mut prev_pressure = points
        .iter()
        .take(10)
        .fold(points[0].pressure, |acc, curr| {
            let mut pressure = curr.pressure;
            if options.simulate_pressure {
                let sp = 1.0_f64.min(curr.distance / options.size);
                let rp = 1.0_f64.min(1.0 - sp);
                pressure = 1.0_f64.min(acc + (rp - acc) * (sp * RATE_OF_PRESSURE_CHANGE));
            }
            (acc + pressure) / 2.0
        });

    let mut radius = stroke_radius(
        options.size,
        options.thinning,
        points.last().expect("non-empty").pressure,
        options.easing,
    );

    let mut first_radius: Option<f64> = None;
    let mut prev_vector = points[0].vector;

    let mut pl = points[0].point;
    let mut pr = pl;
    let mut tl = pl;
    let mut tr = pr;

    let mut is_prev_point_sharp_corner = false;

    for i in 0..points.len() {
        let mut pressure = points[i].pressure;
        let point = points[i].point;
        let vector = points[i].vector;
        let distance = points[i].distance;
        let running_length = points[i].running_length;

        // Removes noise from the end of the line.
        if i < points.len() - 1 && total_length - running_length < 3.0 {
            continue;
        }

        if options.thinning != 0.0 {
            if options.simulate_pressure {
                let sp = 1.0_f64.min(distance / options.size);
                let rp = 1.0_f64.min(1.0 - sp);
                pressure = 1.0_f64
                    .min(prev_pressure + (rp - prev_pressure) * (sp * RATE_OF_PRESSURE_CHANGE));
            }
            radius = stroke_radius(options.size, options.thinning, pressure, options.easing);
        } else {
            radius = options.size / 2.0;
        }

        if first_radius.is_none() {
            first_radius = Some(radius);
        }

        // Apply tapering: `start`/`end` are never passed, so the taper strengths (`ts`/`te`
        // in the source) are always 1 and `radius * min(ts, te)` is just `radius`.
        radius = radius.max(0.01);

        let next_vector = if i < points.len() - 1 {
            points[i + 1].vector
        } else {
            points[i].vector
        };
        let next_dpr = if i < points.len() - 1 {
            dpr(vector, next_vector)
        } else {
            1.0
        };
        let prev_dpr = dpr(vector, prev_vector);

        let is_point_sharp_corner = prev_dpr < 0.0 && !is_prev_point_sharp_corner;
        let is_next_point_sharp_corner = next_dpr < 0.0;

        if is_point_sharp_corner || is_next_point_sharp_corner {
            let offset = mul(per(prev_vector), radius);
            let step = 1.0 / 13.0;
            let mut t = 0.0;
            while t <= 1.0 {
                tl = rot_around(sub(point, offset), point, FIXED_PI * t);
                left_pts.push(tl);
                tr = rot_around(add(point, offset), point, FIXED_PI * -t);
                right_pts.push(tr);
                t += step;
            }
            pl = tl;
            pr = tr;
            if is_next_point_sharp_corner {
                is_prev_point_sharp_corner = true;
            }
            continue;
        }

        is_prev_point_sharp_corner = false;

        if i == points.len() - 1 {
            let offset = mul(per(vector), radius);
            left_pts.push(sub(point, offset));
            right_pts.push(add(point, offset));
            continue;
        }

        let offset = mul(per(lrp(next_vector, vector, next_dpr)), radius);

        tl = sub(point, offset);
        if i <= 1 || dist_sq(pl, tl) > min_distance {
            left_pts.push(tl);
            pl = tl;
        }

        tr = add(point, offset);
        if i <= 1 || dist_sq(pr, tr) > min_distance {
            right_pts.push(tr);
            pr = tr;
        }

        prev_pressure = pressure;
        prev_vector = vector;
    }

    let first_point = points[0].point;
    let last_point = if points.len() > 1 {
        points[points.len() - 1].point
    } else {
        add(points[0].point, [1.0, 1.0])
    };

    // Draw a dot for a single-point (or otherwise too-short) stroke. `taperStart`/
    // `taperEnd` are always `undefined`, so `!(taperStart || taperEnd)` is always true and
    // this branch always returns rather than falling through to the cap-drawing code below.
    if points.len() == 1 {
        let start = prj(
            first_point,
            uni(per(sub(first_point, last_point))),
            -js_or(first_radius, radius),
        );
        let mut dot_pts = Vec::new();
        let step = 1.0 / 13.0;
        let mut t = step;
        while t <= 1.0 {
            dot_pts.push(rot_around(start, first_point, FIXED_PI * 2.0 * t));
            t += step;
        }
        return dot_pts;
    }

    // Draw a start cap: `taperStart`/`taperEnd` are always `undefined`, so the round-cap
    // branch (`capStart` defaults to `true`) always runs.
    let mut start_cap: Vec<[f64; 2]> = Vec::new();
    let step = 1.0 / 13.0;
    let mut t = step;
    while t <= 1.0 {
        start_cap.push(rot_around(right_pts[0], first_point, FIXED_PI * t));
        t += step;
    }

    // Draw an end cap: same precondition, `capEnd` defaults to `true`.
    let direction = per(neg(points.last().expect("non-empty").vector));
    let end_cap_start = prj(last_point, direction, radius);
    let mut end_cap: Vec<[f64; 2]> = Vec::new();
    let step = 1.0 / 29.0;
    let mut t = step;
    while t < 1.0 {
        end_cap.push(rot_around(end_cap_start, last_point, FIXED_PI * 3.0 * t));
        t += step;
    }

    // Left side, then the end cap, then the right side reversed, then the start cap.
    let mut outline = left_pts;
    outline.extend(end_cap);
    right_pts.reverse();
    outline.extend(right_pts);
    outline.extend(start_cap);
    outline
}

/// `getStroke`.
pub(crate) fn get_stroke(points: &[[f64; 3]], options: &Options) -> Vec<[f64; 2]> {
    let stroke_points = get_stroke_points(points, options.streamline, options.size, options.last);
    get_stroke_outline_points(&stroke_points, options)
}
