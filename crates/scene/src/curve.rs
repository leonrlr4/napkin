//! Cubic Bezier helpers from `packages/math/src/curve.ts` at the pinned commit: arc length
//! (Legendre-Gauss, 24 points), point at a fraction of the arc length, the Newton solver
//! `curveIntersectLineSegment` uses, and the offset/Catmull-Rom curve approximation used to
//! grow a rounded outline by a distance.

/// A cubic Bezier curve as its 4 control points.
pub type Curve = [[f64; 2]; 4];

/// `LegendreGaussN24TValues` (`packages/math/src/constants.ts`).
const LEGENDRE_GAUSS_N24_T: [f64; 24] = [
    -0.06405689286260563,
    0.06405689286260563,
    -0.1911188674736163,
    0.1911188674736163,
    -0.3150426796961634,
    0.3150426796961634,
    -0.4337935076260451,
    0.4337935076260451,
    -0.5454214713888396,
    0.5454214713888396,
    -0.6480936519369755,
    0.6480936519369755,
    -0.7401241915785544,
    0.7401241915785544,
    -0.820001985973903,
    0.820001985973903,
    -0.8864155270044011,
    0.8864155270044011,
    -0.9382745520027328,
    0.9382745520027328,
    -0.9747285559713095,
    0.9747285559713095,
    -0.9951872199970213,
    0.9951872199970213,
];

/// `LegendreGaussN24CValues`.
const LEGENDRE_GAUSS_N24_C: [f64; 24] = [
    0.12793819534675216,
    0.12793819534675216,
    0.1258374563468283,
    0.1258374563468283,
    0.12167047292780339,
    0.12167047292780339,
    0.1155056680537256,
    0.1155056680537256,
    0.10744427011596563,
    0.10744427011596563,
    0.09761865210411388,
    0.09761865210411388,
    0.08619016153195327,
    0.08619016153195327,
    0.0733464814110803,
    0.0733464814110803,
    0.05929858491543678,
    0.05929858491543678,
    0.04427743881741981,
    0.04427743881741981,
    0.028531388628933663,
    0.028531388628933663,
    0.0123412297999872,
    0.0123412297999872,
];

/// `bezierEquation`.
pub fn bezier_equation(c: Curve, t: f64) -> [f64; 2] {
    let mt = 1.0 - t;
    let (a, b, cc, d) = (
        mt.powi(3),
        3.0 * mt.powi(2) * t,
        3.0 * mt * t.powi(2),
        t.powi(3),
    );
    [
        a * c[0][0] + b * c[1][0] + cc * c[2][0] + d * c[3][0],
        a * c[0][1] + b * c[1][1] + cc * c[2][1] + d * c[3][1],
    ]
}

/// `curveTangent`: the derivative of the curve at `t`.
pub fn curve_tangent(c: Curve, t: f64) -> [f64; 2] {
    let [p0, p1, p2, p3] = c;
    let mt = 1.0 - t;
    [
        -3.0 * mt * mt * p0[0] + 3.0 * mt * mt * p1[0] - 6.0 * t * mt * p1[0] - 3.0 * t * t * p2[0]
            + 6.0 * t * mt * p2[0]
            + 3.0 * t * t * p3[0],
        -3.0 * mt * mt * p0[1] + 3.0 * mt * mt * p1[1] - 6.0 * t * mt * p1[1] - 3.0 * t * t * p2[1]
            + 6.0 * t * mt * p2[1]
            + 3.0 * t * t * p3[1],
    ]
}

/// Quadrature of `|curveTangent|` over `[0, upper]` with `z = upper / 2` (the shared body of
/// `curveLength` and `curveLengthAtParameter`).
fn tangent_integral(c: Curve, z: f64) -> f64 {
    let mut sum = 0.0;
    for i in 0..24 {
        let t = z * LEGENDRE_GAUSS_N24_T[i] + z;
        let d = curve_tangent(c, t);
        sum += LEGENDRE_GAUSS_N24_C[i] * (d[0] * d[0] + d[1] * d[1]).sqrt();
    }
    z * sum
}

/// `curveLength`.
pub fn curve_length(c: Curve) -> f64 {
    tangent_integral(c, 0.5)
}

/// `curveLengthAtParameter`: the arc length from `t = 0` to `t`.
pub fn curve_length_at_parameter(c: Curve, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return curve_length(c);
    }
    tangent_integral(c, t / 2.0)
}

/// `curvePointAtLength`: the point `percent` (0 to 1) of the way along the arc length, found by
/// bisection on `t`. `total` is the curve's already known length.
pub fn curve_point_at_length(c: Curve, percent: f64, total: f64) -> [f64; 2] {
    if percent <= 0.0 {
        return bezier_equation(c, 0.0);
    }
    if percent >= 1.0 {
        return bezier_equation(c, 1.0);
    }

    let target = total * percent;
    let mut t_min = 0.0;
    let mut t_max = 1.0;
    let mut t = percent;
    let tolerance = total * 0.0001;

    for _ in 0..20 {
        let current = curve_length_at_parameter(c, t);
        if (current - target).abs() < tolerance {
            break;
        }
        if current < target {
            t_min = t;
        } else {
            t_max = t;
        }
        t = (t_min + t_max) / 2.0;
    }

    bezier_equation(c, t)
}

/// `solveWithAnalyticalJacobian`: Newton iteration on the curve parameter `t` and segment
/// parameter `s`; `None` when it does not converge within `iter_limit` steps or the Jacobian
/// is singular.
fn solve_with_analytical_jacobian(
    c: Curve,
    l: [[f64; 2]; 2],
    mut t0: f64,
    mut s0: f64,
    tolerance: f64,
    iter_limit: usize,
) -> Option<[f64; 2]> {
    let mut error = f64::INFINITY;
    let mut iter = 0;

    while error >= tolerance {
        if iter >= iter_limit {
            return None;
        }

        let bt = 1.0 - t0;
        let bt2 = bt * bt;
        let bt3 = bt2 * bt;
        let t0_2 = t0 * t0;
        let t0_3 = t0_2 * t0;

        let bezier_x =
            bt3 * c[0][0] + 3.0 * bt2 * t0 * c[1][0] + 3.0 * bt * t0_2 * c[2][0] + t0_3 * c[3][0];
        let bezier_y =
            bt3 * c[0][1] + 3.0 * bt2 * t0 * c[1][1] + 3.0 * bt * t0_2 * c[2][1] + t0_3 * c[3][1];

        let line_x = l[0][0] + s0 * (l[1][0] - l[0][0]);
        let line_y = l[0][1] + s0 * (l[1][1] - l[0][1]);

        let fx = bezier_x - line_x;
        let fy = bezier_y - line_y;

        error = fx.abs() + fy.abs();
        if error < tolerance {
            break;
        }

        let dfx_dt = -3.0 * bt2 * c[0][0] + 3.0 * bt2 * c[1][0]
            - 6.0 * bt * t0 * c[1][0]
            - 3.0 * t0_2 * c[2][0]
            + 6.0 * bt * t0 * c[2][0]
            + 3.0 * t0_2 * c[3][0];
        let dfy_dt = -3.0 * bt2 * c[0][1] + 3.0 * bt2 * c[1][1]
            - 6.0 * bt * t0 * c[1][1]
            - 3.0 * t0_2 * c[2][1]
            + 6.0 * bt * t0 * c[2][1]
            + 3.0 * t0_2 * c[3][1];

        let dfx_ds = -(l[1][0] - l[0][0]);
        let dfy_ds = -(l[1][1] - l[0][1]);

        let det = dfx_dt * dfy_ds - dfx_ds * dfy_dt;
        if det.abs() < 1e-12 {
            return None;
        }

        let inv_det = 1.0 / det;
        let dt = inv_det * (dfy_ds * -fx - dfx_ds * -fy);
        let ds = inv_det * (-dfy_dt * -fx + dfx_dt * -fy);

        t0 += dt;
        s0 += ds;
        iter += 1;
    }

    Some([t0, s0])
}

/// `curveIntersectLineSegment` with its default tolerance of `1e-2`: at most one crossing,
/// found by trying the initial guesses `t = 0.5, 0.2, 0.8`.
pub fn curve_intersect_line_segment(
    c: Curve,
    l: [[f64; 2]; 2],
    iter_limit: usize,
) -> Vec<[f64; 2]> {
    const TOLERANCE: f64 = 1e-2;
    for guess in [0.5, 0.2, 0.8] {
        let Some([t, s]) = solve_with_analytical_jacobian(c, l, guess, 0.0, TOLERANCE, iter_limit)
        else {
            continue;
        };
        if t < 0.0 || t > 1.0 || s < 0.0 || s > 1.0 {
            continue;
        }
        return vec![bezier_equation(c, t)];
    }
    Vec::new()
}

/// `curveOffsetPoints` with 50 steps: points along the curve moved by `offset` along the
/// right-hand normal of the tangent.
pub fn curve_offset_points(c: Curve, offset: f64) -> Vec<[f64; 2]> {
    const STEPS: usize = 50;
    (0..=STEPS)
        .map(|i| {
            let t = i as f64 / STEPS as f64;
            let p = bezier_equation(c, t);
            let tangent = curve_tangent(c, t);
            let m = (tangent[0] * tangent[0] + tangent[1] * tangent[1]).sqrt();
            let unit = if m == 0.0 {
                [0.0, 0.0]
            } else {
                [tangent[0] / m, tangent[1] / m]
            };
            // `vectorNormal`: (x, y) -> (y, -x).
            [p[0] + unit[1] * offset, p[1] + -unit[0] * offset]
        })
        .collect()
}

/// `curveCatmullRomCubicApproxPoints` with the default tension of 0.5: one cubic per
/// consecutive pair of `points`.
pub fn curve_catmull_rom_cubic_approx(points: &[[f64; 2]]) -> Vec<Curve> {
    const TENSION: f64 = 0.5;
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n - 1)
        .map(|i| {
            let p0 = points[i.saturating_sub(1)];
            let p1 = points[i];
            let p2 = points[(i + 1).min(n - 1)];
            let p3 = points[(i + 2).min(n - 1)];
            let tangent1 = [(p2[0] - p0[0]) * TENSION, (p2[1] - p0[1]) * TENSION];
            let tangent2 = [(p3[0] - p1[0]) * TENSION, (p3[1] - p1[1]) * TENSION];
            [
                p1,
                [p1[0] + tangent1[0] / 3.0, p1[1] + tangent1[1] / 3.0],
                [p2[0] - tangent2[0] / 3.0, p2[1] - tangent2[1] / 3.0],
                p2,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cubic whose control points lie on a straight line, evenly spaced: its arc length
    /// is the chord length and `t` is proportional to arc length.
    const STRAIGHT: Curve = [[0.0, 0.0], [30.0, 40.0], [60.0, 80.0], [90.0, 120.0]];

    #[test]
    fn straight_curve_length_is_the_chord() {
        assert!((curve_length(STRAIGHT) - 150.0).abs() < 1e-9);
        assert!((curve_length_at_parameter(STRAIGHT, 0.4) - 60.0).abs() < 1e-9);
        assert_eq!(curve_length_at_parameter(STRAIGHT, 0.0), 0.0);
        assert_eq!(
            curve_length_at_parameter(STRAIGHT, 1.5),
            curve_length(STRAIGHT)
        );
    }

    #[test]
    fn point_at_length_follows_the_arc() {
        let total = curve_length(STRAIGHT);
        let mid = curve_point_at_length(STRAIGHT, 0.5, total);
        assert!(
            (mid[0] - 45.0).abs() < 0.1 && (mid[1] - 60.0).abs() < 0.1,
            "{mid:?}"
        );
        assert_eq!(curve_point_at_length(STRAIGHT, 0.0, total), [0.0, 0.0]);
        assert_eq!(curve_point_at_length(STRAIGHT, 1.0, total), [90.0, 120.0]);
    }

    #[test]
    fn segment_crossing_a_curve_is_found_and_a_miss_is_not() {
        let arch = [[0.0, 0.0], [0.0, 100.0], [100.0, 100.0], [100.0, 0.0]];
        let hits = curve_intersect_line_segment(arch, [[-10.0, 60.0], [110.0, 60.0]], 4);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!((hits[0][1] - 60.0).abs() < 1e-2, "{hits:?}");
        assert!(curve_intersect_line_segment(arch, [[-10.0, 120.0], [110.0, 120.0]], 4).is_empty());
    }

    #[test]
    fn offset_curves_move_along_the_right_hand_normal() {
        let points = curve_offset_points([[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0]], 5.0);
        assert_eq!(points.len(), 51);
        assert!(points.iter().all(|p| (p[1] + 5.0).abs() < 1e-9));
        let curves = curve_catmull_rom_cubic_approx(&points);
        assert_eq!(curves.len(), 50);
        assert_eq!(curves[0][0], points[0]);
        assert_eq!(curves[49][3], points[50]);
    }
}
