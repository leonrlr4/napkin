//! Port of the freedraw outline functions in `packages/element/src/shape.ts`
//! (`getFreedrawOutlinePoints`, `getVariableWidthFreedrawOutline`,
//! `getConstantWidthFreedrawOutline`, `createLaserPointer`, `getFreedrawStreamline`,
//! `VARIABLE_WIDTH_FREEDRAW`, `CONSTANT_WIDTH_FREEDRAW`) at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. `getFreedrawStrokeCenterPoints` (bucket-fill
//! boundary helper) has no napkin caller yet and is not ported.

use crate::element::FreedrawElement;
use crate::laser_pointer::{self, LaserPointer};
use crate::perfect_freehand;

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
        keep_head: false,
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
