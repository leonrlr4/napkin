//! Port of the `"line"`/`"arrow"` branch of `_generateElementShape` and
//! `generateElbowArrowShape` (`packages/element/src/shape.ts`), `isElbowArrow`/
//! `isLinearElement` (`packages/element/src/typeChecks.ts`), and `vectorToHeading`/
//! `headingForPoint`/`headingForPointIsHorizontal` (`packages/element/src/heading.ts`), all
//! at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`. `pointDistance` comes from
//! `packages/math/src/point.ts` (`Math.hypot`); `vectorFromPoint` from
//! `packages/math/src/vector.ts`.

use rough::{Drawable, RoughGenerator};

use crate::element::{Element, LinearElement};
use crate::json::Slot;

use super::GEOMETRY_BOUND;
use super::arrowhead::{self, Position};
use super::options::generate_rough_options;

/// `generateElbowArrowShape`'s `radius` argument at its one call site.
const ELBOW_ARROW_RADIUS: f64 = 16.0;

/// `isElbowArrow`: `isArrowElement(element) && element.elbowed`. `isArrowElement` is
/// implied by matching `Element::Arrow` before calling this.
fn is_elbow_arrow(element: &LinearElement) -> bool {
    element.elbowed == Some(true)
}

/// `vectorFromPoint`/`vectorToHeading`/`headingForPoint`/`headingForPointIsHorizontal`
/// collapsed into the one boolean `generateElbowArrowShape` needs: whether the heading from
/// `o` to `p` is `HEADING_RIGHT` or `HEADING_LEFT` rather than up/down.
///
/// `vectorToHeading([x, y])` picks `RIGHT` when `x > |y|`, `LEFT` when `x <= -|y|`, and
/// otherwise `DOWN`/`UP`; `headingIsHorizontal` is true for the first two cases only, so it
/// reduces to `x > |y| || x <= -|y|` without needing the `Heading` type itself.
fn heading_for_point_is_horizontal(p: [f64; 2], o: [f64; 2]) -> bool {
    let x = p[0] - o[0];
    let y = p[1] - o[1];
    let abs_y = y.abs();
    x > abs_y || x <= -abs_y
}

/// `pointDistance`.
fn point_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    rough::js::hypot(b[0] - a[0], b[1] - a[1])
}

/// `generateElbowArrowShape`.
fn elbow_arrow_path(points: &[[f64; 2]], radius: f64) -> String {
    let mut subpoints: Vec<[f64; 2]> = Vec::new();

    for i in 1..points.len().saturating_sub(1) {
        let prev = points[i - 1];
        let next = points[i + 1];
        let point = points[i];
        let prev_is_horizontal = heading_for_point_is_horizontal(point, prev);
        let next_is_horizontal = heading_for_point_is_horizontal(next, point);
        let corner = radius
            .min(point_distance(points[i], next) / 2.0)
            .min(point_distance(points[i], prev) / 2.0);

        if prev_is_horizontal {
            if prev[0] < point[0] {
                // LEFT
                subpoints.push([points[i][0] - corner, points[i][1]]);
            } else {
                // RIGHT
                subpoints.push([points[i][0] + corner, points[i][1]]);
            }
        } else if prev[1] < point[1] {
            // UP
            subpoints.push([points[i][0], points[i][1] - corner]);
        } else {
            subpoints.push([points[i][0], points[i][1] + corner]);
        }

        subpoints.push(points[i]);

        if next_is_horizontal {
            if next[0] < point[0] {
                // LEFT
                subpoints.push([points[i][0] - corner, points[i][1]]);
            } else {
                // RIGHT
                subpoints.push([points[i][0] + corner, points[i][1]]);
            }
        } else if next[1] < point[1] {
            // UP
            subpoints.push([points[i][0], points[i][1] - corner]);
        } else {
            // DOWN
            subpoints.push([points[i][0], points[i][1] + corner]);
        }
    }

    let (x0, y0) = (points[0][0], points[0][1]);
    let mut d = vec![format!("M {x0} {y0}")];

    let mut i = 0;
    while i < subpoints.len() {
        let (lx, ly) = (subpoints[i][0], subpoints[i][1]);
        let (qx1, qy1) = (subpoints[i + 1][0], subpoints[i + 1][1]);
        let (qx2, qy2) = (subpoints[i + 2][0], subpoints[i + 2][1]);
        d.push(format!("L {lx} {ly}"));
        d.push(format!("Q {qx1} {qy1}, {qx2} {qy2}"));
        i += 3;
    }

    let (lx, ly) = (points[points.len() - 1][0], points[points.len() - 1][1]);
    d.push(format!("L {lx} {ly}"));

    d.join(" ")
}

/// Absent or `null`: no start arrowhead. `const { startArrowhead = null } = element`.
fn start_arrowhead(l: &LinearElement) -> Option<&str> {
    match &l.start_arrowhead {
        Slot::Value(v) => Some(v.as_str()),
        Slot::Missing | Slot::Null => None,
    }
}

/// Absent defaults to `"arrow"`; `null` stays `null`. `const { endArrowhead = "arrow" } =
/// element`: the destructuring default only applies when the key is absent.
fn end_arrowhead(l: &LinearElement) -> Option<&str> {
    match &l.end_arrowhead {
        Slot::Value(v) => Some(v.as_str()),
        Slot::Missing => Some("arrow"),
        Slot::Null => None,
    }
}

/// `_generateElementShape`'s `"line"`/`"arrow"` case.
pub(super) fn shape(
    generator: &RoughGenerator,
    element: &Element,
    l: &LinearElement,
    dark_mode: bool,
    canvas_background_color: &str,
) -> Vec<Drawable> {
    let options = generate_rough_options(element, false, dark_mode).expect("line/arrow draws");

    // points array can be empty in the beginning, so it is important to add initial
    // position to it
    let points: Vec<[f64; 2]> = if l.points.is_empty() {
        vec![[0.0, 0.0]]
    } else {
        l.points.clone()
    };

    let mut result: Vec<Drawable> = if is_elbow_arrow(l) {
        // NOTE (mtolmacs): Temporary fix for extremely big arrow shapes. Elbow arrows are
        // exempt from `generate_element_shape`'s upfront `GEOMETRY_BOUND` check (a
        // legitimate elbow arrow can be this large), so this is the only guard standing
        // between a huge coordinate and rough.js for them.
        if points
            .iter()
            .any(|p| p[0].abs() > GEOMETRY_BOUND || p[1].abs() > GEOMETRY_BOUND)
        {
            Vec::new()
        } else {
            let d = elbow_arrow_path(&points, ELBOW_ARROW_RADIUS);
            let elbow_options =
                generate_rough_options(element, true, dark_mode).expect("line/arrow draws");
            vec![
                generator
                    .path(&d, &elbow_options)
                    .expect("an elbow arrow's path is well-formed for finite element geometry"),
            ]
        }
    } else if matches!(l.base.roundness, Slot::Value(_)) {
        // curve is always the first element; this simplifies finding the curve for an
        // element
        vec![generator.curve(&points, &options)]
    } else if options.fill.is_some() {
        vec![generator.polygon(&points, &options)]
    } else {
        vec![generator.linear_path(&points, &options)]
    };

    // add lines only in arrow
    if matches!(element, Element::Arrow(_)) {
        if let Some(head) = start_arrowhead(l) {
            let heads = arrowhead::shapes(
                l,
                &result,
                Position::Start,
                head,
                generator,
                &options,
                canvas_background_color,
                dark_mode,
            );
            result.extend(heads);
        }

        if let Some(head) = end_arrowhead(l) {
            let heads = arrowhead::shapes(
                l,
                &result,
                Position::End,
                head,
                generator,
                &options,
                canvas_background_color,
                dark_mode,
            );
            result.extend(heads);
        }
    }

    result
}
