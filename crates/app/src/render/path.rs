//! Converting rough.js op sets and freedraw outline ops into lyon paths, and splitting a
//! flattened path into dashes following HTML canvas `setLineDash` (`bin/canvas.js`'s
//! `_drawToContext`, and the browser's own dash algorithm it relies on).

use lyon::math::point;
use lyon::path::{Path, PathEvent};

/// `rough::Op::Move`/`LineTo`/`BCurveTo` as a lyon path. `Move` starts a new subpath (closing
/// any open one first, without joining it back to its start); the ops otherwise draw into the
/// currently open subpath. If the first op is not `Move`, the path starts from `(0, 0)`,
/// mirroring an HTML canvas `Path2D` with no current point yet.
pub fn ops_path(ops: &[rough::Op]) -> Path {
    let mut builder = Path::builder();
    let mut open = false;
    for op in ops {
        match op {
            rough::Op::Move(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(point(p[0] as f32, p[1] as f32));
                open = true;
            }
            rough::Op::LineTo(p) => {
                if !open {
                    builder.begin(point(0.0, 0.0));
                    open = true;
                }
                builder.line_to(point(p[0] as f32, p[1] as f32));
            }
            rough::Op::BCurveTo(d) => {
                if !open {
                    builder.begin(point(0.0, 0.0));
                    open = true;
                }
                builder.cubic_bezier_to(
                    point(d[0] as f32, d[1] as f32),
                    point(d[2] as f32, d[3] as f32),
                    point(d[4] as f32, d[5] as f32),
                );
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

/// `scene::shape::PathOp::Move`/`Quad`/`Line`/`Close` as a lyon path (a freedraw stroke
/// outline, which always opens with `Move`).
pub fn outline_path(ops: &[scene::shape::PathOp]) -> Path {
    use scene::shape::PathOp;

    let mut builder = Path::builder();
    let mut open = false;
    for op in ops {
        match op {
            PathOp::Move(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(point(p[0] as f32, p[1] as f32));
                open = true;
            }
            PathOp::Quad(d) => {
                builder.quadratic_bezier_to(
                    point(d[0] as f32, d[1] as f32),
                    point(d[2] as f32, d[3] as f32),
                );
            }
            PathOp::Line(p) => {
                builder.line_to(point(p[0] as f32, p[1] as f32));
            }
            PathOp::Close => {
                builder.end(true);
                open = false;
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

// Arc lengths and the walk along the pattern are done in `f64`: a `f32` division such as
// `15.0 / 100.0` does not round-trip through a later multiply back to an exact `15.0`, and
// `dash_polyline`'s output is expected to land exactly on dash boundaries that fall on whole
// input coordinates. `f64`'s extra precision keeps that rounding error far enough below `f32`
// ULP that the final cast to `f32` lands on the same value the exact real-number computation
// would.
fn distance(a: [f32; 2], b: [f32; 2]) -> f64 {
    let dx = (b[0] - a[0]) as f64;
    let dy = (b[1] - a[1]) as f64;
    (dx * dx + dy * dy).sqrt()
}

/// The point at arc length `len` along `vertices` (whose cumulative arc lengths at each vertex
/// are `cum`), clamped to the polyline's ends.
fn point_at(vertices: &[[f32; 2]], cum: &[f64], len: f64) -> [f32; 2] {
    if len <= 0.0 || vertices.len() < 2 {
        return vertices[0];
    }
    for i in 1..vertices.len() {
        if len <= cum[i] {
            let span = cum[i] - cum[i - 1];
            let t = if span > 0.0 {
                (len - cum[i - 1]) / span
            } else {
                0.0
            };
            let [ax, ay] = vertices[i - 1].map(|v| v as f64);
            let [bx, by] = vertices[i].map(|v| v as f64);
            return [(ax + (bx - ax) * t) as f32, (ay + (by - ay) * t) as f32];
        }
    }
    vertices[vertices.len() - 1]
}

/// The dash covering arc length `[start, end]`: its endpoints, plus every vertex strictly
/// between them (a dash crossing a corner bends there instead of cutting across it). `start ==
/// end` (a zero-length dash) still yields two identical points, so a round cap draws a dot.
fn dash_segment(vertices: &[[f32; 2]], cum: &[f64], start: f64, end: f64) -> Vec<[f32; 2]> {
    let mut segment = vec![point_at(vertices, cum, start)];
    for (i, &v) in vertices.iter().enumerate() {
        if cum[i] > start && cum[i] < end {
            segment.push(v);
        }
    }
    segment.push(point_at(vertices, cum, end));
    segment
}

/// Splits one flattened subpath into dashes following HTML canvas `setLineDash`: an odd
/// pattern repeats twice, the phase starts at 0 for every subpath, and a closed subpath
/// includes its closing segment. `pattern` with no positive entry, or any negative entry,
/// disables dashing (canvas ignores an invalid dash list): the whole polyline (closing segment
/// included) comes back as the only "dash".
pub fn dash_polyline(points: &[[f32; 2]], closed: bool, pattern: &[f32]) -> Vec<Vec<[f32; 2]>> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut vertices = points.to_vec();
    if closed {
        vertices.push(points[0]);
    }

    let invalid =
        pattern.is_empty() || pattern.iter().all(|&v| v == 0.0) || pattern.iter().any(|&v| v < 0.0);
    if invalid {
        return vec![vertices];
    }

    let full_pattern: Vec<f64> = if pattern.len() % 2 == 1 {
        pattern
            .iter()
            .chain(pattern.iter())
            .map(|&v| v as f64)
            .collect()
    } else {
        pattern.iter().map(|&v| v as f64).collect()
    };

    let mut cum = vec![0.0_f64; vertices.len()];
    for i in 1..vertices.len() {
        cum[i] = cum[i - 1] + distance(vertices[i - 1], vertices[i]);
    }
    let total_length = *cum.last().expect("vertices is non-empty");

    let mut dashes = Vec::new();
    let mut cursor = 0.0_f64;
    let mut index = 0_usize;
    loop {
        let step = full_pattern[index % full_pattern.len()];
        let reach = cursor + step;
        if index.is_multiple_of(2) {
            dashes.push(dash_segment(
                &vertices,
                &cum,
                cursor,
                reach.min(total_length),
            ));
        }
        if reach >= total_length {
            break;
        }
        cursor = reach;
        index += 1;
    }
    dashes
}

/// `path` flattened with `tolerance` and dashed subpath by subpath: each dash becomes its own
/// open subpath in the result.
pub fn dashed(path: &Path, pattern: &[f32], tolerance: f32) -> Path {
    use lyon::path::iterator::PathIterator;

    let mut builder = Path::builder();
    let mut current: Vec<[f32; 2]> = Vec::new();

    for event in path.iter().flattened(tolerance) {
        match event {
            PathEvent::Begin { at } => {
                current.clear();
                current.push(at.to_array());
            }
            PathEvent::Line { to, .. } => {
                current.push(to.to_array());
            }
            PathEvent::End { close, .. } => {
                for dash in dash_polyline(&current, close, pattern) {
                    if dash.len() < 2 {
                        continue;
                    }
                    builder.begin(point(dash[0][0], dash[0][1]));
                    for p in &dash[1..] {
                        builder.line_to(point(p[0], p[1]));
                    }
                    builder.end(false);
                }
                current.clear();
            }
            PathEvent::Quadratic { .. } | PathEvent::Cubic { .. } => {
                unreachable!("flattened() only yields Begin/Line/End")
            }
        }
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_a_straight_line() {
        let dashes = dash_polyline(&[[0.0, 0.0], [100.0, 0.0]], false, &[8.0, 10.0]);
        assert_eq!(dashes.len(), 6);
        assert_eq!(dashes[0], vec![[0.0, 0.0], [8.0, 0.0]]);
        assert_eq!(dashes[5], vec![[90.0, 0.0], [98.0, 0.0]]);
    }

    #[test]
    fn odd_patterns_repeat_twice() {
        let dashes = dash_polyline(&[[0.0, 0.0], [100.0, 0.0]], false, &[5.0]);
        assert_eq!(dashes.len(), 10);
        assert_eq!(dashes[1], vec![[10.0, 0.0], [15.0, 0.0]]);
    }

    #[test]
    fn dashes_turn_corners_and_include_the_closing_segment() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let dashes = dash_polyline(&square, true, &[15.0, 5.0]);
        assert_eq!(dashes.len(), 2);
        assert_eq!(dashes[0], vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]]);
        assert_eq!(dashes[1], vec![[10.0, 10.0], [0.0, 10.0], [0.0, 5.0]]);
    }

    #[test]
    fn every_subpath_restarts_the_pattern() {
        use lyon::path::PathEvent;

        let mut builder = lyon::path::Path::builder();
        for y in [0.0, 10.0] {
            builder.begin(lyon::math::point(0.0, y));
            builder.line_to(lyon::math::point(20.0, y));
            builder.end(false);
        }
        let dashed = dashed(&builder.build(), &[8.0, 4.0], 0.1);
        let subpaths = dashed
            .iter()
            .filter(|e| matches!(e, PathEvent::Begin { .. }))
            .count();
        assert_eq!(subpaths, 4);
    }
}
