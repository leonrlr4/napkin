//! Port of `hachure-fill@0.5.2`: `bin/hachure.js`.

use std::cmp::Ordering;

use crate::core::Point;
use crate::js::{self, truthy};

/// One edge of the polygon's edge table: the `y` range it spans, its current `x` at `ymin`,
/// and the slope used to advance `x` as the scanline moves.
#[derive(Clone, Copy)]
struct Edge {
    ymin: f64,
    ymax: f64,
    x: f64,
    islope: f64,
}

/// `bin/hachure.js` `areSamePoints`.
fn are_same_points(p1: Point, p2: Point) -> bool {
    p1[0] == p2[0] && p1[1] == p2[1]
}

/// `bin/hachure.js` `rotatePoints`: rotates `points` in place around `center` by `degrees`.
///
/// `cos`/`sin` are `f64`'s own, not ported (see `js.rs`'s module docs for why and the
/// measured divergence rate). The rotated polygon feeds the scanline intersection that
/// decides how many hachure lines get drawn, so a last-bit difference here can change a
/// hachure fill's line count, not just a coordinate.
fn rotate_points(points: &mut [Point], center: Point, degrees: f64) {
    if points.is_empty() {
        return;
    }
    let [cx, cy] = center;
    let angle = (std::f64::consts::PI / 180.0) * degrees;
    let cos = angle.cos();
    let sin = angle.sin();
    for p in points.iter_mut() {
        let [x, y] = *p;
        p[0] = (x - cx) * cos - (y - cy) * sin + cx;
        p[1] = (x - cx) * sin + (y - cy) * cos + cy;
    }
}

/// `bin/hachure.js` `rotateLines`. The JS flattens both endpoints of every line into one array
/// and rotates that array in place, so all the mutated points share a single `cos`/`sin` pair
/// computed once. Rotating each line separately here produces identical values: `cos`/`sin`
/// depend only on `degrees`, not on iteration order.
fn rotate_lines(lines: &mut [[Point; 2]], center: Point, degrees: f64) {
    for line in lines.iter_mut() {
        rotate_points(line, center, degrees);
    }
}

/// `bin/hachure.js` `hachureLines`. The JS opens with a check that wraps a single polygon
/// (rather than a list of polygons) into a one-element list; rough.js never calls it that way,
/// so that branch is not ported (see the M1 plan's decision 5).
pub fn hachure_lines(
    polygons: &mut [Vec<Point>],
    hachure_gap: f64,
    hachure_angle: f64,
    hachure_step_offset: f64,
) -> Vec<[Point; 2]> {
    let angle = hachure_angle;
    let gap = hachure_gap.max(0.1);
    let rotation_center = [0.0, 0.0];
    if truthy(angle) {
        for polygon in polygons.iter_mut() {
            rotate_points(polygon, rotation_center, angle);
        }
    }
    let mut lines = straight_hachure_lines(polygons, gap, hachure_step_offset);
    if truthy(angle) {
        for polygon in polygons.iter_mut() {
            rotate_points(polygon, rotation_center, -angle);
        }
        rotate_lines(&mut lines, rotation_center, -angle);
    }
    lines
}

/// `bin/hachure.js` `straightHachureLines`.
fn straight_hachure_lines(
    polygons: &[Vec<Point>],
    gap: f64,
    hachure_step_offset: f64,
) -> Vec<[Point; 2]> {
    let mut vertex_array: Vec<Vec<Point>> = Vec::new();
    for polygon in polygons {
        // `[...polygon]` is a shallow copy; if a closing point is appended it lands in this
        // new array, so it never mutates the caller's polygon.
        let mut vertices = polygon.clone();
        if !are_same_points(vertices[0], vertices[vertices.len() - 1]) {
            vertices.push([vertices[0][0], vertices[0][1]]);
        }
        if vertices.len() > 2 {
            vertex_array.push(vertices);
        }
    }
    let mut lines: Vec<[Point; 2]> = Vec::new();
    let gap = gap.max(0.1);
    // Create sorted edges table
    let mut edges: Vec<Edge> = Vec::new();
    for vertices in &vertex_array {
        for pair in vertices.windows(2) {
            let p1 = pair[0];
            let p2 = pair[1];
            if p1[1] != p2[1] {
                let ymin = p1[1].min(p2[1]);
                edges.push(Edge {
                    ymin,
                    ymax: p1[1].max(p2[1]),
                    x: if ymin == p1[1] { p1[0] } else { p2[0] },
                    islope: (p2[0] - p1[0]) / (p2[1] - p1[1]),
                });
            }
        }
    }
    edges.sort_by(|e1, e2| {
        if e1.ymin < e2.ymin {
            return Ordering::Less;
        }
        if e1.ymin > e2.ymin {
            return Ordering::Greater;
        }
        if e1.x < e2.x {
            return Ordering::Less;
        }
        if e1.x > e2.x {
            return Ordering::Greater;
        }
        if e1.ymax == e2.ymax {
            return Ordering::Equal;
        }
        if e1.ymax < e2.ymax {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });
    if edges.is_empty() {
        return lines;
    }
    // Start scanning
    // JS wraps each active edge as `{ s: y, edge }`; `s` is assigned but never read anywhere,
    // so the port keeps `Edge`s directly in `active_edges` without that wrapper.
    let mut active_edges: Vec<Edge> = Vec::new();
    let mut y = edges[0].ymin;
    let mut iteration: f64 = 0.0;
    while !active_edges.is_empty() || !edges.is_empty() {
        if !edges.is_empty() {
            let mut ix: Option<usize> = None;
            for (i, edge) in edges.iter().enumerate() {
                if edge.ymin > y {
                    break;
                }
                ix = Some(i);
            }
            if let Some(ix) = ix {
                let removed: Vec<Edge> = edges.drain(..=ix).collect();
                active_edges.extend(removed);
            }
        }
        active_edges.retain(|edge| edge.ymax > y);
        active_edges.sort_by(|a, b| {
            if a.x == b.x {
                Ordering::Equal
            } else if a.x < b.x {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        });
        // fill between the edges
        if (hachure_step_offset != 1.0 || iteration % gap == 0.0) && active_edges.len() > 1 {
            for pair in active_edges.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                let ce = &pair[0];
                let ne = &pair[1];
                lines.push([[js::math_round(ce.x), y], [js::math_round(ne.x), y]]);
            }
        }
        y += hachure_step_offset;
        for ae in active_edges.iter_mut() {
            ae.x += hachure_step_offset * ae.islope;
        }
        iteration += 1.0;
    }
    lines
}
