//! `bin/renderer.js`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::core::{Op, OpSet, OpSetType, Point, ResolvedOptions};
use crate::js::truthy;
use crate::math::Random;

/// The options object rough.js threads through the renderer. `random(ops)` lazily hangs a
/// `Random` on it; a copy made with `Object.assign({}, o)` shares that `Random` if it
/// already exists and gets its own on first use if not. Cloning the `Rc` reproduces both,
/// so port every `Object.assign({}, o, ...)` as `o.clone()` followed by field writes.
#[derive(Clone, Debug)]
pub(crate) struct Ctx {
    pub o: ResolvedOptions,
    randomizer: Option<Rc<RefCell<Random>>>,
}

impl Ctx {
    pub fn new(o: ResolvedOptions) -> Ctx {
        Ctx {
            o,
            randomizer: None,
        }
    }

    /// `random(ops)`.
    pub fn random(&mut self) -> f64 {
        let seed = if truthy(self.o.seed) {
            self.o.seed
        } else {
            0.0
        };
        self.randomizer
            .get_or_insert_with(|| Rc::new(RefCell::new(Random::new(seed))))
            .borrow_mut()
            .next()
    }

    /// `cloneOptionsAlterSeed`.
    pub fn clone_alter_seed(&self) -> Ctx {
        let mut o = self.o.clone();
        if truthy(o.seed) {
            o.seed += 1.0;
        }
        Ctx::new(o)
    }
}

/// bin/renderer.js `line`.
pub(crate) fn line(x1: f64, y1: f64, x2: f64, y2: f64, o: &mut Ctx) -> OpSet {
    OpSet {
        kind: OpSetType::Path,
        ops: _double_line(x1, y1, x2, y2, o, false),
    }
}

/// bin/renderer.js `linearPath`.
pub(crate) fn linear_path(points: &[Point], close: bool, o: &mut Ctx) -> OpSet {
    let len = points.len();
    if len > 2 {
        let mut ops = Vec::new();
        for i in 0..(len - 1) {
            ops.extend(_double_line(
                points[i][0],
                points[i][1],
                points[i + 1][0],
                points[i + 1][1],
                o,
                false,
            ));
        }
        if close {
            ops.extend(_double_line(
                points[len - 1][0],
                points[len - 1][1],
                points[0][0],
                points[0][1],
                o,
                false,
            ));
        }
        return OpSet {
            kind: OpSetType::Path,
            ops,
        };
    } else if len == 2 {
        return line(points[0][0], points[0][1], points[1][0], points[1][1], o);
    }
    OpSet {
        kind: OpSetType::Path,
        ops: Vec::new(),
    }
}

/// bin/renderer.js `polygon`.
pub(crate) fn polygon(points: &[Point], o: &mut Ctx) -> OpSet {
    linear_path(points, true, o)
}

/// bin/renderer.js `rectangle`.
pub(crate) fn rectangle(x: f64, y: f64, width: f64, height: f64, o: &mut Ctx) -> OpSet {
    let points = [
        [x, y],
        [x + width, y],
        [x + width, y + height],
        [x, y + height],
    ];
    polygon(&points, o)
}

/// bin/renderer.js `curve`.
pub(crate) fn curve(points: &[Point], o: &mut Ctx) -> OpSet {
    let mut o1 = _curve_with_offset(points, 1.0 * (1.0 + o.o.roughness * 0.2), o);
    if !o.o.disable_multi_stroke {
        let o2 = _curve_with_offset(
            points,
            1.5 * (1.0 + o.o.roughness * 0.22),
            &mut o.clone_alter_seed(),
        );
        o1.extend(o2);
    }
    OpSet {
        kind: OpSetType::Path,
        ops: o1,
    }
}

/// bin/renderer.js `generateEllipseParams` return value.
pub(crate) struct EllipseParams {
    pub increment: f64,
    pub rx: f64,
    pub ry: f64,
}

/// bin/renderer.js `ellipseWithParams` return value.
pub(crate) struct EllipseResult {
    pub estimated_points: Vec<Point>,
    pub opset: OpSet,
}

/// bin/renderer.js `generateEllipseParams`.
pub(crate) fn generate_ellipse_params(width: f64, height: f64, o: &mut Ctx) -> EllipseParams {
    let psq = (std::f64::consts::PI
        * 2.0
        * (((width / 2.0).powf(2.0) + (height / 2.0).powf(2.0)) / 2.0).sqrt())
    .sqrt();
    let step_count =
        o.o.curve_step_count
            .max((o.o.curve_step_count / 200.0_f64.sqrt()) * psq)
            .ceil();
    let increment = (std::f64::consts::PI * 2.0) / step_count;
    let mut rx = (width / 2.0).abs();
    let mut ry = (height / 2.0).abs();
    let curve_fit_randomness = 1.0 - o.o.curve_fitting;
    rx += offset_opt(rx * curve_fit_randomness, o, 1.0);
    ry += offset_opt(ry * curve_fit_randomness, o, 1.0);
    EllipseParams { increment, rx, ry }
}

/// bin/renderer.js `ellipseWithParams`.
pub(crate) fn ellipse_with_params(
    x: f64,
    y: f64,
    o: &mut Ctx,
    ellipse_params: &EllipseParams,
) -> EllipseResult {
    // JS: `ellipseParams.increment * _offset(0.1, _offset(0.4, 1, o), o)`. Argument
    // expressions evaluate left to right, so the inner `_offset` call draws before the outer.
    let inner = offset(0.4, 1.0, o, 1.0);
    let overlap = ellipse_params.increment * offset(0.1, inner, o, 1.0);
    let (ap1, cp1) = _compute_ellipse_points(
        ellipse_params.increment,
        x,
        y,
        ellipse_params.rx,
        ellipse_params.ry,
        1.0,
        overlap,
        o,
    );
    let mut o1 = _curve(&ap1, None, o);
    if !o.o.disable_multi_stroke && o.o.roughness != 0.0 {
        let (ap2, _cp2) = _compute_ellipse_points(
            ellipse_params.increment,
            x,
            y,
            ellipse_params.rx,
            ellipse_params.ry,
            1.5,
            0.0,
            o,
        );
        let o2 = _curve(&ap2, None, o);
        o1.extend(o2);
    }
    EllipseResult {
        estimated_points: cp1,
        opset: OpSet {
            kind: OpSetType::Path,
            ops: o1,
        },
    }
}

/// bin/renderer.js `arc`.
#[expect(clippy::too_many_arguments, reason = "mirrors renderer.js")]
pub(crate) fn arc(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    start: f64,
    stop: f64,
    closed: bool,
    rough_closure: bool,
    o: &mut Ctx,
) -> OpSet {
    let cx = x;
    let cy = y;
    let mut rx = (width / 2.0).abs();
    let mut ry = (height / 2.0).abs();
    rx += offset_opt(rx * 0.01, o, 1.0);
    ry += offset_opt(ry * 0.01, o, 1.0);
    let mut strt = start;
    let mut stp = stop;
    while strt < 0.0 {
        strt += std::f64::consts::PI * 2.0;
        stp += std::f64::consts::PI * 2.0;
    }
    if (stp - strt) > (std::f64::consts::PI * 2.0) {
        strt = 0.0;
        stp = std::f64::consts::PI * 2.0;
    }
    let ellipse_inc = (std::f64::consts::PI * 2.0) / o.o.curve_step_count;
    let arc_inc = (ellipse_inc / 2.0).min((stp - strt) / 2.0);
    let mut ops = _arc(arc_inc, cx, cy, rx, ry, strt, stp, 1.0, o);
    if !o.o.disable_multi_stroke {
        let o2 = _arc(arc_inc, cx, cy, rx, ry, strt, stp, 1.5, o);
        ops.extend(o2);
    }
    if closed {
        if rough_closure {
            ops.extend(_double_line(
                cx,
                cy,
                cx + rx * strt.cos(),
                cy + ry * strt.sin(),
                o,
                false,
            ));
            ops.extend(_double_line(
                cx,
                cy,
                cx + rx * stp.cos(),
                cy + ry * stp.sin(),
                o,
                false,
            ));
        } else {
            ops.push(Op::LineTo([cx, cy]));
            ops.push(Op::LineTo([cx + rx * strt.cos(), cy + ry * strt.sin()]));
        }
    }
    OpSet {
        kind: OpSetType::Path,
        ops,
    }
}

// Fills

/// bin/renderer.js `solidFillPolygon`.
pub(crate) fn solid_fill_polygon(_polygon_list: &[Vec<Point>], _o: &mut Ctx) -> OpSet {
    todo!()
}

/// bin/renderer.js `patternFillPolygons`.
pub(crate) fn pattern_fill_polygons(_polygon_list: &mut [Vec<Point>], _o: &mut Ctx) -> OpSet {
    todo!()
}

/// bin/renderer.js `patternFillArc`. Implemented in Task 10.
pub(crate) fn pattern_fill_arc(
    _x: f64,
    _y: f64,
    _width: f64,
    _height: f64,
    _start: f64,
    _stop: f64,
    _o: &mut Ctx,
) -> OpSet {
    todo!()
}

// Private helpers

/// bin/renderer.js `_offset`.
fn offset(min: f64, max: f64, o: &mut Ctx, roughness_gain: f64) -> f64 {
    o.o.roughness * roughness_gain * ((o.random() * (max - min)) + min)
}

/// bin/renderer.js `_offsetOpt`.
fn offset_opt(x: f64, o: &mut Ctx, roughness_gain: f64) -> f64 {
    offset(-x, x, o, roughness_gain)
}

/// bin/renderer.js `_doubleLine`.
fn _double_line(x1: f64, y1: f64, x2: f64, y2: f64, o: &mut Ctx, filling: bool) -> Vec<Op> {
    let single_stroke = if filling {
        o.o.disable_multi_stroke_fill
    } else {
        o.o.disable_multi_stroke
    };
    let mut o1 = _line(x1, y1, x2, y2, o, true, false);
    if single_stroke {
        return o1;
    }
    let o2 = _line(x1, y1, x2, y2, o, true, true);
    o1.extend(o2);
    o1
}

/// bin/renderer.js `_line`.
fn _line(x1: f64, y1: f64, x2: f64, y2: f64, o: &mut Ctx, r#move: bool, overlay: bool) -> Vec<Op> {
    let length_sq = (x1 - x2).powf(2.0) + (y1 - y2).powf(2.0);
    let length = length_sq.sqrt();
    let roughness_gain = if length < 200.0 {
        1.0
    } else if length > 500.0 {
        0.4
    } else {
        (-0.0016668) * length + 1.233334
    };
    let mut offset = if truthy(o.o.max_randomness_offset) {
        o.o.max_randomness_offset
    } else {
        0.0
    };
    if (offset * offset * 100.0) > length_sq {
        offset = length / 10.0;
    }
    let half_offset = offset / 2.0;
    let diverge_point = 0.2 + o.random() * 0.2;
    let mut mid_disp_x = o.o.bowing * o.o.max_randomness_offset * (y2 - y1) / 200.0;
    let mut mid_disp_y = o.o.bowing * o.o.max_randomness_offset * (x1 - x2) / 200.0;
    mid_disp_x = offset_opt(mid_disp_x, o, roughness_gain);
    mid_disp_y = offset_opt(mid_disp_y, o, roughness_gain);
    let mut ops = Vec::new();
    // The JS closures draw from the randomizer on every call; taking `o` as a parameter
    // keeps each draw where the JS evaluates it.
    let random_half = |o: &mut Ctx| offset_opt(half_offset, o, roughness_gain);
    let random_full = |o: &mut Ctx| offset_opt(offset, o, roughness_gain);
    let preserve_vertices = o.o.preserve_vertices;
    if r#move {
        if overlay {
            ops.push(Op::Move([
                x1 + if preserve_vertices {
                    0.0
                } else {
                    random_half(o)
                },
                y1 + if preserve_vertices {
                    0.0
                } else {
                    random_half(o)
                },
            ]));
        } else {
            ops.push(Op::Move([
                x1 + if preserve_vertices {
                    0.0
                } else {
                    offset_opt(offset, o, roughness_gain)
                },
                y1 + if preserve_vertices {
                    0.0
                } else {
                    offset_opt(offset, o, roughness_gain)
                },
            ]));
        }
    }
    if overlay {
        ops.push(Op::BCurveTo([
            mid_disp_x + x1 + (x2 - x1) * diverge_point + random_half(o),
            mid_disp_y + y1 + (y2 - y1) * diverge_point + random_half(o),
            mid_disp_x + x1 + 2.0 * (x2 - x1) * diverge_point + random_half(o),
            mid_disp_y + y1 + 2.0 * (y2 - y1) * diverge_point + random_half(o),
            x2 + if preserve_vertices {
                0.0
            } else {
                random_half(o)
            },
            y2 + if preserve_vertices {
                0.0
            } else {
                random_half(o)
            },
        ]));
    } else {
        ops.push(Op::BCurveTo([
            mid_disp_x + x1 + (x2 - x1) * diverge_point + random_full(o),
            mid_disp_y + y1 + (y2 - y1) * diverge_point + random_full(o),
            mid_disp_x + x1 + 2.0 * (x2 - x1) * diverge_point + random_full(o),
            mid_disp_y + y1 + 2.0 * (y2 - y1) * diverge_point + random_full(o),
            x2 + if preserve_vertices {
                0.0
            } else {
                random_full(o)
            },
            y2 + if preserve_vertices {
                0.0
            } else {
                random_full(o)
            },
        ]));
    }
    ops
}

/// bin/renderer.js `_curveWithOffset`.
fn _curve_with_offset(points: &[Point], offset: f64, o: &mut Ctx) -> Vec<Op> {
    let mut ps: Vec<Point> = Vec::new();
    ps.push([
        points[0][0] + offset_opt(offset, o, 1.0),
        points[0][1] + offset_opt(offset, o, 1.0),
    ]);
    ps.push([
        points[0][0] + offset_opt(offset, o, 1.0),
        points[0][1] + offset_opt(offset, o, 1.0),
    ]);
    for i in 1..points.len() {
        ps.push([
            points[i][0] + offset_opt(offset, o, 1.0),
            points[i][1] + offset_opt(offset, o, 1.0),
        ]);
        if i == (points.len() - 1) {
            ps.push([
                points[i][0] + offset_opt(offset, o, 1.0),
                points[i][1] + offset_opt(offset, o, 1.0),
            ]);
        }
    }
    _curve(&ps, None, o)
}

/// bin/renderer.js `_curve`. Every 4.6.4 call site passes `null` for `closePoint`.
fn _curve(points: &[Point], close_point: Option<Point>, o: &mut Ctx) -> Vec<Op> {
    let len = points.len();
    let mut ops = Vec::new();
    if len > 3 {
        let mut b: [Point; 4] = [[0.0; 2]; 4];
        let s = 1.0 - o.o.curve_tightness;
        ops.push(Op::Move([points[1][0], points[1][1]]));
        for i in 1..(len - 2) {
            let cached_vert_array = points[i];
            b[0] = [cached_vert_array[0], cached_vert_array[1]];
            b[1] = [
                cached_vert_array[0] + (s * points[i + 1][0] - s * points[i - 1][0]) / 6.0,
                cached_vert_array[1] + (s * points[i + 1][1] - s * points[i - 1][1]) / 6.0,
            ];
            b[2] = [
                points[i + 1][0] + (s * points[i][0] - s * points[i + 2][0]) / 6.0,
                points[i + 1][1] + (s * points[i][1] - s * points[i + 2][1]) / 6.0,
            ];
            b[3] = [points[i + 1][0], points[i + 1][1]];
            ops.push(Op::BCurveTo([
                b[1][0], b[1][1], b[2][0], b[2][1], b[3][0], b[3][1],
            ]));
        }
        if let Some(close_point) = close_point {
            let ro = o.o.max_randomness_offset;
            ops.push(Op::LineTo([
                close_point[0] + offset_opt(ro, o, 1.0),
                close_point[1] + offset_opt(ro, o, 1.0),
            ]));
        }
    } else if len == 3 {
        ops.push(Op::Move([points[1][0], points[1][1]]));
        ops.push(Op::BCurveTo([
            points[1][0],
            points[1][1],
            points[2][0],
            points[2][1],
            points[2][0],
            points[2][1],
        ]));
    } else if len == 2 {
        ops.extend(_double_line(
            points[0][0],
            points[0][1],
            points[1][0],
            points[1][1],
            o,
            false,
        ));
    }
    ops
}

/// bin/renderer.js `_computeEllipsePoints`. Returns `(allPoints, corePoints)`. JS pushes the
/// same array into both lists; `Point` is `Copy`, so copying it into each `Vec` here has the
/// same effect as long as nothing rewrites `corePoints` in place before `allPoints` is turned
/// into ops (true through Task 9; filling that mutates `corePoints` arrives later).
#[expect(clippy::too_many_arguments, reason = "mirrors renderer.js")]
fn _compute_ellipse_points(
    increment: f64,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    offset: f64,
    overlap: f64,
    o: &mut Ctx,
) -> (Vec<Point>, Vec<Point>) {
    let core_only = o.o.roughness == 0.0;
    let mut core_points: Vec<Point> = Vec::new();
    let mut all_points: Vec<Point> = Vec::new();
    if core_only {
        let increment = increment / 4.0;
        all_points.push([cx + rx * (-increment).cos(), cy + ry * (-increment).sin()]);
        let mut angle = 0.0;
        while angle <= std::f64::consts::PI * 2.0 {
            let p = [cx + rx * angle.cos(), cy + ry * angle.sin()];
            core_points.push(p);
            all_points.push(p);
            angle += increment;
        }
        all_points.push([cx + rx * 0.0_f64.cos(), cy + ry * 0.0_f64.sin()]);
        all_points.push([cx + rx * increment.cos(), cy + ry * increment.sin()]);
    } else {
        let rad_offset = offset_opt(0.5, o, 1.0) - (std::f64::consts::PI / 2.0);
        all_points.push([
            offset_opt(offset, o, 1.0) + cx + 0.9 * rx * (rad_offset - increment).cos(),
            offset_opt(offset, o, 1.0) + cy + 0.9 * ry * (rad_offset - increment).sin(),
        ]);
        let end_angle = std::f64::consts::PI * 2.0 + rad_offset - 0.01;
        let mut angle = rad_offset;
        while angle < end_angle {
            let p = [
                offset_opt(offset, o, 1.0) + cx + rx * angle.cos(),
                offset_opt(offset, o, 1.0) + cy + ry * angle.sin(),
            ];
            core_points.push(p);
            all_points.push(p);
            angle += increment;
        }
        all_points.push([
            offset_opt(offset, o, 1.0)
                + cx
                + rx * (rad_offset + std::f64::consts::PI * 2.0 + overlap * 0.5).cos(),
            offset_opt(offset, o, 1.0)
                + cy
                + ry * (rad_offset + std::f64::consts::PI * 2.0 + overlap * 0.5).sin(),
        ]);
        all_points.push([
            offset_opt(offset, o, 1.0) + cx + 0.98 * rx * (rad_offset + overlap).cos(),
            offset_opt(offset, o, 1.0) + cy + 0.98 * ry * (rad_offset + overlap).sin(),
        ]);
        all_points.push([
            offset_opt(offset, o, 1.0) + cx + 0.9 * rx * (rad_offset + overlap * 0.5).cos(),
            offset_opt(offset, o, 1.0) + cy + 0.9 * ry * (rad_offset + overlap * 0.5).sin(),
        ]);
    }
    (all_points, core_points)
}

/// bin/renderer.js `_arc`.
#[expect(clippy::too_many_arguments, reason = "mirrors renderer.js")]
fn _arc(
    increment: f64,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    strt: f64,
    stp: f64,
    offset: f64,
    o: &mut Ctx,
) -> Vec<Op> {
    let rad_offset = strt + offset_opt(0.1, o, 1.0);
    let mut points: Vec<Point> = Vec::new();
    points.push([
        offset_opt(offset, o, 1.0) + cx + 0.9 * rx * (rad_offset - increment).cos(),
        offset_opt(offset, o, 1.0) + cy + 0.9 * ry * (rad_offset - increment).sin(),
    ]);
    let mut angle = rad_offset;
    while angle <= stp {
        points.push([
            offset_opt(offset, o, 1.0) + cx + rx * angle.cos(),
            offset_opt(offset, o, 1.0) + cy + ry * angle.sin(),
        ]);
        angle += increment;
    }
    points.push([cx + rx * stp.cos(), cy + ry * stp.sin()]);
    points.push([cx + rx * stp.cos(), cy + ry * stp.sin()]);
    _curve(&points, None, o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_an_existing_randomizer_only() {
        let mut a = Ctx::new(ResolvedOptions {
            seed: 1.0,
            ..ResolvedOptions::default()
        });
        let mut before = a.clone();
        let first = a.random();
        let mut after = a.clone();
        assert_eq!(
            before.random(),
            first,
            "copy made before the first draw starts its own sequence"
        );
        assert_ne!(
            after.random(),
            first,
            "copy made after the first draw continues the shared one"
        );
        // A copied (unshared) randomizer would also pass the assertion above, since its next
        // draw is the second value too. Only a shared one has been advanced by `after`, so
        // `a` now yields the third value of the sequence.
        let mut fresh = Random::new(1.0);
        let sequence: Vec<f64> = (0..3).map(|_| fresh.next()).collect();
        assert_eq!(
            a.random(),
            sequence[2],
            "the original sees the draw its later copy made"
        );
    }
}
