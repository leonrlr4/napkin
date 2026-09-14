//! Port of the arrowhead generators in `packages/element/src/shape.ts`
//! (`generateArrowheadCardinalityOne`, `generateArrowheadLinesToTip`,
//! `getArrowheadLineOptions`, `generateArrowheadOutlineCircle`, `getArrowheadShapes`),
//! `getArrowheadPoints`/`getArrowheadSize`/`getArrowheadAngle` (`packages/element/src/
//! bounds.ts`) and `getCurvePathOps` (`packages/utils/src/shape.ts`), all at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. `pointRotateRads` comes from
//! `packages/math/src/point.ts`.

use rough::{Drawable, Op, OpSetType, Options, RoughGenerator};

use crate::element::LinearElement;

use super::options::{dark, dash_array_dotted};

/// `"start" | "end"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Position {
    Start,
    End,
}

/// `CARDINALITY_MARKER_SIZE` (`packages/element/src/bounds.ts`).
const CARDINALITY_MARKER_SIZE: f64 = 20.0;

/// `CROWFOOT_ARROWHEAD_SIZE`.
const CROWFOOT_ARROWHEAD_SIZE: f64 = 15.0;

/// `cardinalityOneOrManyOffset` (`getArrowheadShapes`).
const CARDINALITY_ONE_OR_MANY_OFFSET: f64 = -0.25;

/// `cardinalityZeroCircleScale`.
const CARDINALITY_ZERO_CIRCLE_SCALE: f64 = 0.8;

/// `getArrowheadSize`.
fn arrowhead_size(arrowhead: &str) -> f64 {
    match arrowhead {
        "arrow" => 25.0,
        "diamond" | "diamond_outline" => 12.0,
        "cardinality_many" | "cardinality_one_or_many" | "cardinality_zero_or_many" => {
            CROWFOOT_ARROWHEAD_SIZE
        }
        "cardinality_one" | "cardinality_exactly_one" | "cardinality_zero_or_one" => {
            CARDINALITY_MARKER_SIZE
        }
        _ => 15.0,
    }
}

/// `getArrowheadAngle`.
fn arrowhead_angle(arrowhead: &str) -> f64 {
    match arrowhead {
        "bar" => 90.0,
        "arrow" => 20.0,
        _ => 25.0,
    }
}

/// `degreesToRadians` (`packages/math/src/angle.ts`): `(degrees * Math.PI) / 180`. Every
/// caller here passes either `angle` or `-angle` in degrees, so one helper covers both
/// `getArrowheadPoints` call styles (`degreesToRadians(-angle)` and the inlined
/// `(-angle * Math.PI) / 180`, which are the same expression).
fn degrees_to_radians(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// `pointRotateRads`: `if (!angle) return point;` short-circuits for `0` (`+0`/`-0`) and
/// `NaN`, both falsy in JS.
fn point_rotate_rads(point: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    if angle == 0.0 || angle.is_nan() {
        return point;
    }
    let (x, y) = (point[0], point[1]);
    let (cx, cy) = (center[0], center[1]);
    [
        (x - cx) * angle.cos() - (y - cy) * angle.sin() + cx,
        (x - cx) * angle.sin() + (y - cy) * angle.cos() + cy,
    ]
}

/// `getCurvePathOps`. The JS falls back to `shape.sets[0].ops` when no `"path"` opset is
/// found, indexing unconditionally: `shape.sets[0]` is `undefined` when `sets` is empty, and
/// `.ops` on that throws a `TypeError` (`packages/utils/src/shape.ts:210`). That happens for
/// a real drawable: `strokeColor: "none"` makes `rough`'s `line`/`curve`/`polygon`/
/// `linear_path`/`path` all skip pushing their outline opset (`crates/rough/src/
/// generator.rs`), and a line/arrow with a transparent background has no fill opset either,
/// so `sets` is empty. Decision 7 extends here too: return an empty ops list instead of
/// panicking, which `arrowhead_points`'s `ops.is_empty()` check then treats exactly like the
/// JS's `ops.length < 1` short-circuit (skip this arrowhead).
fn curve_path_ops(drawable: &Drawable) -> Vec<Op> {
    for set in &drawable.sets {
        if set.kind == OpSetType::Path {
            return set.ops.clone();
        }
    }
    drawable
        .sets
        .first()
        .map_or_else(Vec::new, |set| set.ops.clone())
}

/// `getArrowheadPoints`. Returns `None` wherever the JS returns `null`, and also where the
/// JS would throw: `invariant(data.length === 6, ...)` failing (decision 7: skip this
/// arrowhead instead of aborting the whole shape) and any op-array indexing the JS does
/// without a bounds check (`ops[index]`, `ops[index - 1]`) landing outside the array. The
/// latter never happens for the 243 baseline cases (the op at `index` is only ever missing
/// when `index == 0`, which only occurs when `ops.len() == 1`; that lone op is always the
/// initial `Move`, so the `BCurveTo` check below already returns `None` first), but nothing
/// in the source rules it out for other inputs, so it is handled the same way.
///
/// `element.points` (not the `[[0, 0]]`-padded array `_generateElementShape` draws with) is
/// what the JS reads here. When `element.points` is empty, the shape drawn from the padded
/// `[[0, 0]]` can still have ops (e.g. a round line/arrow's `curve()` still emits a
/// `bcurveTo`, and an elbow arrow's degenerate `"M 0 0 L 0 0"` path still emits an op too),
/// so `ops.is_empty()` does not always catch this case. The JS then indexes
/// `element.points[element.points.length - 1]`, which is `undefined` on an empty array, and
/// destructuring that throws a `TypeError` (`packages/element/src/bounds.ts:818-820`).
/// Decision 7 extends here too: skip the arrowhead instead.
fn arrowhead_points(
    element: &LinearElement,
    shape: &[Drawable],
    position: Position,
    arrowhead: &str,
    offset_multiplier: f64,
) -> Option<Vec<f64>> {
    if shape.is_empty() {
        return None;
    }

    let ops = curve_path_ops(&shape[0]);
    if ops.is_empty() {
        return None;
    }

    let index = match position {
        Position::Start => 1,
        Position::End => ops.len() - 1,
    };
    let data = match ops.get(index) {
        Some(Op::BCurveTo(data)) => *data,
        _ => return None,
    };

    let p3 = [data[4], data[5]];
    let p2 = [data[2], data[3]];
    let p1 = [data[0], data[1]];

    // We need to find p0 of the bezier curve. It is typically the last point of the
    // previous curve; it can also be the position of the moveTo operation.
    let p0 = match index.checked_sub(1).and_then(|i| ops.get(i)) {
        Some(Op::Move(p)) => *p,
        Some(Op::BCurveTo(prev)) => [prev[4], prev[5]],
        _ => [0.0, 0.0],
    };

    // B(t) = p0 * (1-t)^3 + 3p1 * t * (1-t)^2 + 3p2 * t^2 * (1-t) + p3 * t^3
    let equation = |t: f64, idx: usize| -> f64 {
        (1.0 - t).powi(3) * p3[idx]
            + 3.0 * t * (1.0 - t).powi(2) * p2[idx]
            + 3.0 * t.powi(2) * (1.0 - t) * p1[idx]
            + p0[idx] * t.powi(3)
    };

    // We know the last point of the arrow (or the first, if start arrowhead).
    let [x2, y2] = if position == Position::Start { p0 } else { p3 };

    // By using the cubic bezier equation (B(t)) and the given parameters, we calculate a
    // point that is closer to the last point. The value 0.3 is chosen arbitrarily and it
    // works best for all the tested cases.
    let x1 = equation(0.3, 0);
    let y1 = equation(0.3, 1);

    // Find the normalized direction vector based on the previously calculated points.
    let distance = (x2 - x1).hypot(y2 - y1);
    let nx = (x2 - x1) / distance;
    let ny = (y2 - y1) / distance;

    let size = arrowhead_size(arrowhead);

    // Length for -> arrows is based on the length of the last section. The JS indexes
    // `element.points` unconditionally here and throws on an empty array (see this
    // function's doc comment); skip the arrowhead instead.
    if element.points.is_empty() {
        return None;
    }
    let last = element.points.len() - 1;
    let [cx, cy] = if position == Position::End {
        element.points[last]
    } else {
        element.points[0]
    };
    let [px, py] = if element.points.len() > 1 {
        if position == Position::End {
            element.points[last - 1]
        } else {
            element.points[1]
        }
    } else {
        [0.0, 0.0]
    };
    let length = (cx - px).hypot(cy - py);

    // Scale down the arrowhead until we hit a certain size so that it doesn't look weird.
    // This value is selected by minimizing a minimum size with the last segment of the
    // arrowhead.
    let length_multiplier = if arrowhead == "diamond" || arrowhead == "diamond_outline" {
        0.25
    } else {
        0.5
    };
    let min_size = size.min(length * length_multiplier);
    let tx = x2 - nx * min_size * offset_multiplier;
    let ty = y2 - ny * min_size * offset_multiplier;
    let xs = tx - nx * min_size;
    let ys = ty - ny * min_size;

    if arrowhead == "circle" || arrowhead == "circle_outline" {
        let diameter = (ys - ty).hypot(xs - tx) + element.base.stroke_width - 2.0;
        return Some(vec![tx, ty, diameter]);
    }

    let angle = arrowhead_angle(arrowhead);

    if arrowhead == "cardinality_many" || arrowhead == "cardinality_one_or_many" {
        // swap (xs, ys) with (x2, y2)
        let [x3, y3] = point_rotate_rads([tx, ty], [xs, ys], degrees_to_radians(-angle));
        let [x4, y4] = point_rotate_rads([tx, ty], [xs, ys], degrees_to_radians(angle));
        return Some(vec![xs, ys, x3, y3, x4, y4]);
    }

    // Return points
    let [x3, y3] = point_rotate_rads([xs, ys], [tx, ty], degrees_to_radians(-angle));
    let [x4, y4] = point_rotate_rads([xs, ys], [tx, ty], degrees_to_radians(angle));

    if arrowhead == "diamond" || arrowhead == "diamond_outline" {
        // point opposite to the arrowhead point
        let [ox, oy] = if position == Position::Start {
            let [px, py] = if element.points.len() > 1 {
                element.points[1]
            } else {
                [0.0, 0.0]
            };
            point_rotate_rads(
                [tx + min_size * 2.0, ty],
                [tx, ty],
                (py - ty).atan2(px - tx),
            )
        } else {
            let [px, py] = if element.points.len() > 1 {
                element.points[last - 1]
            } else {
                [0.0, 0.0]
            };
            point_rotate_rads(
                [tx - min_size * 2.0, ty],
                [tx, ty],
                (ty - py).atan2(tx - px),
            )
        };

        return Some(vec![tx, ty, x3, y3, ox, oy, x4, y4]);
    }

    Some(vec![tx, ty, x3, y3, x4, y4])
}

/// `generateArrowheadCardinalityOne`.
fn generate_cardinality_one(
    generator: &RoughGenerator,
    arrowhead_points: Option<Vec<f64>>,
    line_options: &Options,
) -> Vec<Drawable> {
    let Some(p) = arrowhead_points else {
        return Vec::new();
    };
    vec![generator.line(p[2], p[3], p[4], p[5], line_options)]
}

/// `generateArrowheadLinesToTip`.
fn generate_lines_to_tip(
    generator: &RoughGenerator,
    arrowhead_points: Option<Vec<f64>>,
    line_options: &Options,
) -> Vec<Drawable> {
    let Some(p) = arrowhead_points else {
        return Vec::new();
    };
    let (x2, y2) = (p[0], p[1]);
    vec![
        generator.line(p[2], p[3], x2, y2, line_options),
        generator.line(p[4], p[5], x2, y2, line_options),
    ]
}

/// `getArrowheadLineOptions`.
fn arrowhead_line_options(element: &LinearElement, options: &Options) -> Options {
    let mut line_options = options.clone();

    if element.base.stroke_style == "dotted" {
        // for dotted arrows caps, reduce gap to make it more legible
        let dash = dash_array_dotted(element.base.stroke_width - 1.0);
        line_options.stroke_line_dash = Some(vec![dash[0], dash[1] - 1.0]);
    } else {
        // for solid/dashed, keep solid arrow cap
        line_options.stroke_line_dash = None;
    }
    line_options.roughness = Some(line_options.roughness.unwrap_or(0.0).min(1.0));

    line_options
}

/// `generateArrowheadOutlineCircle`.
fn generate_outline_circle(
    generator: &RoughGenerator,
    options: &Options,
    stroke_color: &str,
    arrowhead_points: Option<Vec<f64>>,
    fill: &str,
    diameter_scale: f64,
) -> Vec<Drawable> {
    let Some(p) = arrowhead_points else {
        return Vec::new();
    };
    let (x, y, diameter) = (p[0], p[1], p[2]);

    let mut circle_options = options.clone();
    circle_options.fill = Some(fill.to_owned());
    circle_options.fill_style = Some("solid".to_owned());
    circle_options.stroke = Some(stroke_color.to_owned());
    circle_options.roughness = Some(options.roughness.unwrap_or(0.0).min(0.5));
    circle_options.stroke_line_dash = None;

    vec![generator.circle(x, y, diameter * diameter_scale, &circle_options)]
}

/// `getArrowheadShapes`. `arrowhead === null` is filtered by the caller (`linear::shape`),
/// which is the only place `startArrowhead`/`endArrowhead` are resolved to `Option<&str>`.
/// Unknown arrowhead names, including the legacy `"dot"`, fall through to the same branch
/// as `"bar"`/`"arrow"` (`getArrowheadShapes`' `default` case), matching `getArrowheadSize`/
/// `getArrowheadAngle`'s own `default` branches.
#[expect(clippy::too_many_arguments, reason = "mirrors getArrowheadShapes")]
pub(super) fn shapes(
    element: &LinearElement,
    shape: &[Drawable],
    position: Position,
    arrowhead: &str,
    generator: &RoughGenerator,
    options: &Options,
    canvas_background_color: &str,
    dark_mode: bool,
) -> Vec<Drawable> {
    let stroke_color = dark(&element.base.stroke_color, dark_mode);
    let background_fill_color = dark(canvas_background_color, dark_mode);

    match arrowhead {
        "circle" | "circle_outline" => generate_outline_circle(
            generator,
            options,
            &stroke_color,
            arrowhead_points(element, shape, position, arrowhead, 0.0),
            if arrowhead == "circle_outline" {
                &background_fill_color
            } else {
                &stroke_color
            },
            1.0,
        ),
        "triangle" | "triangle_outline" => {
            let Some(p) = arrowhead_points(element, shape, position, arrowhead, 0.0) else {
                return Vec::new();
            };
            let (x, y, x2, y2, x3, y3) = (p[0], p[1], p[2], p[3], p[4], p[5]);

            let mut triangle_options = options.clone();
            triangle_options.fill = Some(if arrowhead == "triangle_outline" {
                background_fill_color.clone()
            } else {
                stroke_color.clone()
            });
            triangle_options.fill_style = Some("solid".to_owned());
            triangle_options.roughness = Some(options.roughness.unwrap_or(0.0).min(1.0));
            // always use solid stroke for arrowhead
            triangle_options.stroke_line_dash = None;

            vec![generator.polygon(&[[x, y], [x2, y2], [x3, y3], [x, y]], &triangle_options)]
        }
        "diamond" | "diamond_outline" => {
            let Some(p) = arrowhead_points(element, shape, position, arrowhead, 0.0) else {
                return Vec::new();
            };
            let (x, y, x2, y2, x3, y3, x4, y4) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);

            let mut diamond_options = options.clone();
            diamond_options.fill = Some(if arrowhead == "diamond_outline" {
                background_fill_color.clone()
            } else {
                stroke_color.clone()
            });
            diamond_options.fill_style = Some("solid".to_owned());
            diamond_options.roughness = Some(options.roughness.unwrap_or(0.0).min(1.0));
            // always use solid stroke for arrowhead
            diamond_options.stroke_line_dash = None;

            vec![generator.polygon(
                &[[x, y], [x2, y2], [x3, y3], [x4, y4], [x, y]],
                &diamond_options,
            )]
        }
        "cardinality_one" => generate_cardinality_one(
            generator,
            arrowhead_points(element, shape, position, arrowhead, 0.0),
            &arrowhead_line_options(element, options),
        ),
        "cardinality_many" => generate_lines_to_tip(
            generator,
            arrowhead_points(element, shape, position, arrowhead, 0.0),
            &arrowhead_line_options(element, options),
        ),
        "cardinality_one_or_many" => {
            let line_options = arrowhead_line_options(element, options);
            let mut result = generate_lines_to_tip(
                generator,
                arrowhead_points(element, shape, position, "cardinality_many", 0.0),
                &line_options,
            );
            result.extend(generate_cardinality_one(
                generator,
                arrowhead_points(
                    element,
                    shape,
                    position,
                    "cardinality_one",
                    CARDINALITY_ONE_OR_MANY_OFFSET,
                ),
                &line_options,
            ));
            result
        }
        "cardinality_exactly_one" => {
            let line_options = arrowhead_line_options(element, options);
            let mut result = generate_cardinality_one(
                generator,
                arrowhead_points(element, shape, position, "cardinality_one", -0.5),
                &line_options,
            );
            result.extend(generate_cardinality_one(
                generator,
                arrowhead_points(element, shape, position, "cardinality_one", 0.0),
                &line_options,
            ));
            result
        }
        "cardinality_zero_or_one" => {
            let line_options = arrowhead_line_options(element, options);
            let mut result = generate_outline_circle(
                generator,
                options,
                &stroke_color,
                arrowhead_points(element, shape, position, "circle_outline", 1.5),
                &background_fill_color,
                CARDINALITY_ZERO_CIRCLE_SCALE,
            );
            result.extend(generate_cardinality_one(
                generator,
                arrowhead_points(element, shape, position, "cardinality_one", -0.5),
                &line_options,
            ));
            result
        }
        "cardinality_zero_or_many" => {
            let line_options = arrowhead_line_options(element, options);
            let mut result = generate_lines_to_tip(
                generator,
                arrowhead_points(element, shape, position, "cardinality_many", 0.0),
                &line_options,
            );
            result.extend(generate_outline_circle(
                generator,
                options,
                &stroke_color,
                arrowhead_points(element, shape, position, "circle_outline", 1.5),
                &background_fill_color,
                CARDINALITY_ZERO_CIRCLE_SCALE,
            ));
            result
        }
        // "bar" | "arrow" | default
        _ => generate_lines_to_tip(
            generator,
            arrowhead_points(element, shape, position, arrowhead, 0.0),
            &arrowhead_line_options(element, options),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use crate::element::{Element, ElementBase, LinearElement, Roundness};
    use crate::json::Slot;
    use crate::shape::{ElementShape, ShapeContext, generate_element_shape};

    /// A minimal but complete `ElementBase` for building typed arrows directly (bypassing
    /// `Element::from_value`'s JSON round-trip, which these tests have no need for).
    fn base(kind: &str) -> ElementBase {
        ElementBase {
            id: "e1".into(),
            kind: kind.into(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            angle: 0.0,
            stroke_color: "#1e1e1e".into(),
            background_color: "transparent".into(),
            fill_style: "solid".into(),
            stroke_width: 2.0,
            stroke_style: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            group_ids: Vec::new(),
            index: Slot::Missing,
            roundness: Slot::Missing,
            seed: 1.0,
            version: 1.0,
            version_nonce: 1.0,
            is_deleted: false,
            updated: Slot::Missing,
        }
    }

    fn round_roundness() -> Slot<Roundness> {
        Slot::Value(Roundness {
            kind: 3.0,
            value: Slot::Missing,
            extra: Map::new(),
        })
    }

    fn ctx() -> ShapeContext<'static> {
        ShapeContext {
            dark_mode: false,
            canvas_background_color: "#ffffff",
        }
    }

    /// Expects exactly the line/arrow's own drawable(s), with every arrowhead skipped: the
    /// shape must be `Drawables` of the given length, never a panic.
    fn assert_drawable_count(element: &Element, expected: usize) {
        match generate_element_shape(element, &ctx()) {
            ElementShape::Drawables(d) => assert_eq!(
                d.len(),
                expected,
                "expected {expected} drawable(s) (arrowheads skipped, not drawn), got {}",
                d.len()
            ),
            other => panic!("expected Drawables, got {other:?}"),
        }
    }

    #[test]
    fn empty_points_round_arrow_with_end_arrowhead_does_not_panic() {
        // Finding 1: rough's `curve()` still emits a `bcurveTo` for the `[[0, 0]]`-padded
        // single point, so `getArrowheadPoints` reaches `element.points[element.points.len()
        // - 1]` with an empty `element.points` (unlike `arrow/single`/`arrow/empty` in the
        // baseline, whose non-round shape has no ops at all).
        let l = LinearElement {
            base: ElementBase {
                roundness: round_roundness(),
                ..base("arrow")
            },
            points: vec![],
            start_arrowhead: Slot::Missing,
            end_arrowhead: Slot::Value("arrow".into()),
            elbowed: None,
            extra: Map::new(),
        };
        assert_drawable_count(&Element::Arrow(l), 1);
    }

    #[test]
    fn empty_points_elbow_arrow_does_not_panic() {
        // Finding 1: an elbow arrow's degenerate `"M 0 0 L 0 0"` path (from the padded
        // single point) also emits an op, reaching the same `element.points` indexing with
        // an empty array. `endArrowhead` defaults to `"arrow"` when absent.
        let l = LinearElement {
            base: base("arrow"),
            points: vec![],
            start_arrowhead: Slot::Missing,
            end_arrowhead: Slot::Missing,
            elbowed: Some(true),
            extra: Map::new(),
        };
        assert_drawable_count(&Element::Arrow(l), 1);
    }

    #[test]
    fn stroke_none_round_arrow_with_arrowhead_does_not_panic() {
        // Finding 2: `strokeColor: "none"` makes rough's `curve()` skip the outline opset
        // entirely (no fill either, since the background is transparent), so the line
        // drawable's `sets` is empty and `getCurvePathOps`'s indexing fallback would panic.
        let l = LinearElement {
            base: ElementBase {
                stroke_color: "none".into(),
                roundness: round_roundness(),
                ..base("arrow")
            },
            points: vec![[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]],
            start_arrowhead: Slot::Missing,
            end_arrowhead: Slot::Value("arrow".into()),
            elbowed: None,
            extra: Map::new(),
        };
        assert_drawable_count(&Element::Arrow(l), 1);
    }
}
