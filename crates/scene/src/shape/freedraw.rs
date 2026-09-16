//! Port of the freedraw outline functions in `packages/element/src/shape.ts`
//! (`getFreedrawOutlinePoints`, `getVariableWidthFreedrawOutline`,
//! `getConstantWidthFreedrawOutline`, `createLaserPointer`, `getFreedrawStreamline`,
//! `getFreeDrawSvgPath`, `getSvgPathFromStroke`, `med`, `TO_FIXED_PRECISION`,
//! `VARIABLE_WIDTH_FREEDRAW`, `CONSTANT_WIDTH_FREEDRAW`, and `_generateElementShape`'s
//! `"freedraw"` case) at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//! `getFreedrawStrokeCenterPoints` (bucket-fill boundary helper) has no napkin caller yet
//! and is not ported.

use rough::{RoughGenerator, points_on_curve};

use crate::element::{Element, FreedrawElement};
use crate::laser_pointer::{self, LaserPointer};
use crate::perfect_freehand;

use super::options::{generate_rough_options, is_path_a_loop};
use super::{ElementShape, PathOp};

/// `VARIABLE_WIDTH_FREEDRAW.SIZE_FACTOR`.
const VARIABLE_WIDTH_SIZE_FACTOR: f64 = 4.25;
/// `VARIABLE_WIDTH_FREEDRAW.THINNING`.
const VARIABLE_WIDTH_THINNING: f64 = 0.6;
/// `VARIABLE_WIDTH_FREEDRAW.SMOOTHING`.
const VARIABLE_WIDTH_SMOOTHING: f64 = 0.5;
/// `CONSTANT_WIDTH_FREEDRAW.SIZE_FACTOR`.
const CONSTANT_WIDTH_SIZE_FACTOR: f64 = 1.4;
/// `DEFAULT_STROKE_STREAMLINE` (`packages/common/src/constants.ts`).
const DEFAULT_STROKE_STREAMLINE: f64 = 0.5;

/// `easing: (t) => Math.sin((t * Math.PI) / 2)` (`https://easings.net/#easeOutSine`).
/// `sin` is `f64`'s own, not ported: node's V8 build uses a glibc-derived large-table
/// `Math.sin`, not a portable implementation (see `crates/rough/src/js.rs`'s module docs
/// and `laser_pointer`'s `rot`), so this stroke radius can differ from Excalidraw's by up
/// to ~1 ULP.
fn ease_out_sine(t: f64) -> f64 {
    (t * std::f64::consts::PI / 2.0).sin()
}

/// `getFreedrawStreamline`: `element.strokeOptions?.streamline ?? DEFAULT_STROKE_STREAMLINE`.
fn freedraw_streamline(element: &FreedrawElement) -> f64 {
    element
        .stroke_options
        .value()
        .and_then(|options| options.streamline.value())
        .copied()
        .unwrap_or(DEFAULT_STROKE_STREAMLINE)
}

/// `getVariableWidthFreedrawOutline`.
fn variable_width_freedraw_outline(element: &FreedrawElement) -> Vec<[f64; 2]> {
    // `element.simulatePressure ? A : B`: only `true` is truthy, so a missing field (`None`)
    // takes the `B` branch here, same as an explicit `false`.
    let input_points: Vec<[f64; 3]> = if element.simulate_pressure == Some(true) {
        element
            .points
            .iter()
            .map(|&[x, y]| [x, y, f64::NAN])
            .collect()
    } else if !element.points.is_empty() {
        element
            .points
            .iter()
            .enumerate()
            .map(|(i, &[x, y])| [x, y, element.pressures.get(i).copied().unwrap_or(f64::NAN)])
            .collect()
    } else {
        vec![[0.0, 0.0, 0.5]]
    };

    perfect_freehand::get_stroke(
        &input_points,
        &perfect_freehand::Options {
            // `simulatePressure: element.simulatePressure` — the destructure default
            // (`= true`) only fires when the field is absent, unlike the `? :` above which
            // treats absent and explicit `false` alike.
            simulate_pressure: element.simulate_pressure.unwrap_or(true),
            size: element.base.stroke_width * VARIABLE_WIDTH_SIZE_FACTOR,
            thinning: VARIABLE_WIDTH_THINNING,
            smoothing: VARIABLE_WIDTH_SMOOTHING,
            streamline: freedraw_streamline(element),
            easing: ease_out_sine,
            last: true,
        },
    )
}

/// `createLaserPointer` plus the `addPoint` loop in `getConstantWidthFreedrawOutline`.
fn constant_width_freedraw_outline(element: &FreedrawElement) -> Vec<[f64; 2]> {
    let mut pointer = LaserPointer::new(laser_pointer::Options {
        size: element.base.stroke_width * CONSTANT_WIDTH_SIZE_FACTOR,
        streamline: freedraw_streamline(element),
        size_mapping: |pressure| pressure.max(0.1),
    });

    for &[x, y] in &element.points {
        pointer.add_point([x, y, 1.0]);
    }

    pointer
        .get_stroke_outline()
        .into_iter()
        .map(|[x, y, _]| [x, y])
        .collect()
}

/// `getFreedrawOutlinePoints`: unknown/absent `strokeOptions.variability` falls back to
/// the variable-width (perfect-freehand) rendering.
pub fn freedraw_outline_points(element: &FreedrawElement) -> Vec<[f64; 2]> {
    let is_constant = element
        .stroke_options
        .value()
        .and_then(|options| options.variability.value())
        .is_some_and(|v| v == "constant");

    if is_constant {
        constant_width_freedraw_outline(element)
    } else {
        variable_width_freedraw_outline(element)
    }
}

/// `med`: midpoint of two stroke-outline points. Computed from the untruncated
/// coordinates; only the numbers written into the resulting [`PathOp`]s go through
/// [`truncate_path_number`], not this intermediate.
fn med(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// One coordinate as it comes out of `getSvgPathFromStroke`: JS prints the number, then the
/// TO_FIXED_PRECISION regex keeps at most two decimals and deletes the digits, `e` and `-`
/// that follow. Truncation, not rounding; and a mantissa with a decimal point loses its
/// exponent, so 1.4999e-7 becomes 1.49. Excalidraw draws that path, so napkin must too.
/// Assumes |x| < 1e21, where JS switches to exponent form with a "+" the regex keeps.
pub(crate) fn truncate_path_number(x: f64) -> f64 {
    // JS prints |x| < 1e-6 in exponent form; `{:e}` and `{}` give the same shortest digits.
    let text = if x != 0.0 && x.abs() < 1e-6 {
        format!("{x:e}")
    } else {
        format!("{x}")
    };
    match text.split_once('.') {
        None => x,
        Some((int_part, frac)) => {
            let kept: String = frac
                .chars()
                .take_while(char::is_ascii_digit)
                .take(2)
                .collect();
            format!("{int_part}.{kept}")
                .parse()
                .expect("decimal literal")
        }
    }
}

/// A point's coordinates individually run through [`truncate_path_number`], matching the
/// SVG-path regex applied to each number in the joined path string.
fn truncated_point(p: [f64; 2]) -> [f64; 2] {
    [truncate_path_number(p[0]), truncate_path_number(p[1])]
}

fn truncated_quad(control: [f64; 2], end: [f64; 2]) -> PathOp {
    let control = truncated_point(control);
    let end = truncated_point(end);
    PathOp::Quad([control[0], control[1], end[0], end[1]])
}

/// `getSvgPathFromStroke`, as [`PathOp`]s instead of an SVG path string: napkin's renderer
/// (M3) consumes structured ops rather than reparsing SVG.
fn svg_path_from_stroke(points: &[[f64; 2]]) -> Vec<PathOp> {
    let Some((&first, _)) = points.split_first() else {
        return Vec::new();
    };
    let max = points.len() - 1;

    let mut ops = vec![PathOp::Move(truncated_point(first))];
    for (i, &point) in points.iter().enumerate() {
        if i == max {
            ops.push(truncated_quad(point, med(point, first)));
            ops.push(PathOp::Line(truncated_point(first)));
            ops.push(PathOp::Close);
        } else {
            ops.push(truncated_quad(point, med(point, points[i + 1])));
        }
    }
    ops
}

/// `_generateElementShape`'s `"freedraw"` case: an optional rough fill when the stroke
/// outlines a loop, plus the stroke outline itself (`getFreeDrawSvgPath`).
pub(super) fn shape(
    generator: &RoughGenerator,
    element: &Element,
    freedraw: &FreedrawElement,
    dark_mode: bool,
) -> ElementShape {
    let fill = is_path_a_loop(&freedraw.points).then(|| {
        let simplified = points_on_curve::simplify(&freedraw.points, 0.75);
        let mut options =
            generate_rough_options(element, false, dark_mode).expect("freedraw draws");
        options.stroke = Some("none".to_owned());
        Box::new(generator.curve(&simplified, &options))
    });

    ElementShape::Freedraw {
        fill,
        stroke: svg_path_from_stroke(&freedraw_outline_points(freedraw)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_like_the_svg_path_regex() {
        // Expected values from node: the TO_FIXED_PRECISION regex applied to `${x}`.
        for (x, expected) in [
            (1.4999999997655777e-7, 1.49),
            (1.2e-7, 1.2),
            (1e-7, 1e-7),
            (-1.2e-7, -1.2),
            (-2.6789, -2.67),
            (0.29, 0.29),
            (2.675, 2.67),
            (123.0, 123.0),
            (0.000001, 0.0),
            (-0.000001, 0.0),
            (12.5, 12.5),
            (0.30000000000000004, 0.3),
            (5e-324, 5e-324),
            (-0.0000015, 0.0),
        ] {
            assert_eq!(truncate_path_number(x), expected, "{x:e}");
        }
    }
}
