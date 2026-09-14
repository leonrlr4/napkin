//! Port of `points-on-path@0.2.1`: `lib/index.js`.

use crate::path_data::{self, PathError, Segment};
use crate::points_on_curve;

/// A number's JS truthiness: only `0` and `NaN` are falsy.
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `lib/index.js` `pointsOnPath`, `appendPendingCurve` closure.
fn append_pending_curve(
    pending_curve: &mut Vec<[f64; 2]>,
    current_points: &mut Vec<[f64; 2]>,
    tolerance: f64,
) {
    if pending_curve.len() >= 4 {
        current_points.extend(points_on_curve::points_on_bezier_curves(
            pending_curve,
            tolerance,
            None,
        ));
    }
    pending_curve.clear();
}

/// `lib/index.js` `pointsOnPath`, `appendPendingPoints` closure.
fn append_pending_points(
    pending_curve: &mut Vec<[f64; 2]>,
    current_points: &mut Vec<[f64; 2]>,
    sets: &mut Vec<Vec<[f64; 2]>>,
    tolerance: f64,
) {
    append_pending_curve(pending_curve, current_points, tolerance);
    if !current_points.is_empty() {
        sets.push(std::mem::take(current_points));
    }
}

/// `lib/index.js` `pointsOnPath`.
pub fn points_on_path(
    d: &str,
    tolerance: f64,
    distance: Option<f64>,
) -> Result<Vec<Vec<[f64; 2]>>, PathError> {
    let segments = path_data::parse_path(d)?;
    let normalized = path_data::normalize(&path_data::absolutize(&segments));
    let mut sets: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current_points: Vec<[f64; 2]> = Vec::new();
    let mut start = [0.0, 0.0];
    let mut pending_curve: Vec<[f64; 2]> = Vec::new();

    for Segment { key, data } in &normalized {
        match key {
            'M' => {
                append_pending_points(
                    &mut pending_curve,
                    &mut current_points,
                    &mut sets,
                    tolerance,
                );
                start = [data[0], data[1]];
                current_points.push(start);
            }
            'L' => {
                append_pending_curve(&mut pending_curve, &mut current_points, tolerance);
                current_points.push([data[0], data[1]]);
            }
            'C' => {
                if pending_curve.is_empty() {
                    let last_point = current_points.last().copied().unwrap_or(start);
                    pending_curve.push([last_point[0], last_point[1]]);
                }
                pending_curve.push([data[0], data[1]]);
                pending_curve.push([data[2], data[3]]);
                pending_curve.push([data[4], data[5]]);
            }
            'Z' => {
                append_pending_curve(&mut pending_curve, &mut current_points, tolerance);
                current_points.push([start[0], start[1]]);
            }
            _ => {}
        }
    }
    append_pending_points(
        &mut pending_curve,
        &mut current_points,
        &mut sets,
        tolerance,
    );

    if !distance.is_some_and(truthy) {
        return Ok(sets);
    }
    let distance = distance.expect("checked by is_some_and above");

    let mut out = Vec::new();
    for set in &sets {
        let simplified_set = points_on_curve::simplify(set, distance);
        if !simplified_set.is_empty() {
            out.push(simplified_set);
        }
    }
    Ok(out)
}
