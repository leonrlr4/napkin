//! Turning an [`Element`](crate::element::Element) into rough.js drawables and freedraw
//! outlines (`packages/element/src/shape.ts` at the pinned commit).

mod arrowhead;
mod freedraw;
mod generic;
mod linear;
mod options;

use rough::RoughGenerator;

use crate::element::{Element, ElementBase};

pub use freedraw::freedraw_outline_points;
pub use options::generate_rough_options;

/// One segment of a freedraw stroke outline, in element-local coordinates. Mirrors the SVG
/// path commands `getSvgPathFromStroke` emits (`M`/`Q`/`L`/`Z`).
#[derive(Clone, Debug, PartialEq)]
pub enum PathOp {
    Move([f64; 2]),
    Quad([f64; 4]),
    Line([f64; 2]),
    Close,
}

/// Rendering inputs `generateRoughOptions` and the shape generators need beyond the element
/// itself: the app's dark-mode state and the canvas background color (used for arrowhead
/// outline fills).
pub struct ShapeContext<'a> {
    pub dark_mode: bool,
    pub canvas_background_color: &'a str,
}

/// What an element draws: rough.js primitives for rectangle/diamond/ellipse/line/arrow,
/// a background fill plus stroke outline for freedraw, nothing for element types rough.js
/// never draws (text, image, frame, ...), or a placeholder for geometry napkin refuses to
/// hand to rough.js.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementShape {
    None,
    Drawables(Vec<rough::Drawable>),
    Freedraw {
        fill: Option<Box<rough::Drawable>>,
        stroke: Vec<PathOp>,
    },
    /// napkin cannot generate this element's shape: its geometry falls outside
    /// `GEOMETRY_BOUND`, its `roughness` is negative, or rough.js rejected the generated
    /// path. The renderer draws it as the dashed placeholder box used for [`Element::Raw`]
    /// (spec §1.2); the element's data is left untouched, so it round-trips through
    /// save/load and can draw normally again if the geometry later changes.
    Placeholder,
}

/// Geometry napkin hands to rough.js must stay within this bound (finite and `|v| <= 1e6`)
/// or the element draws as [`ElementShape::Placeholder`] instead. `1e6` is Excalidraw's own
/// bound for skipping elbow-arrow paths (`shape.ts`'s "temporary fix for extremely big arrow
/// shapes"); napkin applies it more broadly because past roughly `1e9` rough's curve
/// flattening recurses without end, its path parser rejects overflowed numbers, and hachure
/// fills stop terminating.
pub(crate) const GEOMETRY_BOUND: f64 = 1e6;

fn exceeds_geometry_bound(x: f64) -> bool {
    !x.is_finite() || x.abs() > GEOMETRY_BOUND
}

/// Whether `base`'s `roughness`, `strokeWidth` or `roundness.value` (when present) exceed
/// [`GEOMETRY_BOUND`], or `roughness` is negative. Excalidraw's own UI only ever writes `0`,
/// `1` or `2` for `roughness`; a negative value makes rough's path/curve simplification
/// distance `(1.0 + roughness) / 2.0` negative too, and `simplify_points`'s `epsilon` guard
/// (`points_on_curve.rs`) then never terminates its recursion (JS throws a `RangeError` for
/// the same input; Rust aborts the process instead, which no `Result`/`Option` can catch).
/// Applies to every drawable element type, elbow arrows included: `width`/`height` and
/// point coordinates are checked separately by [`geometry_exceeds_bound`], and only for
/// non-elbow shapes.
fn base_exceeds_geometry_bound(base: &ElementBase) -> bool {
    base.roughness < 0.0
        || exceeds_geometry_bound(base.roughness)
        || exceeds_geometry_bound(base.stroke_width)
        || base
            .roundness
            .value()
            .and_then(|r| r.value.value())
            .is_some_and(|&v| exceeds_geometry_bound(v))
}

/// Whether `element`'s geometry keeps `generate_element_shape` from calling into rough.js:
/// [`base_exceeds_geometry_bound`] for every drawable element type, plus `width`, `height`
/// and every point coordinate for line/arrow/freedraw, except elbow arrows.
fn geometry_exceeds_bound(element: &Element) -> bool {
    let base = match element {
        Element::Rectangle(g) | Element::Diamond(g) | Element::Ellipse(g) => &g.base,
        Element::Line(l) | Element::Arrow(l) => &l.base,
        Element::Freedraw(f) => &f.base,
        Element::Text(_) | Element::Raw(_) => return false,
    };
    if base_exceeds_geometry_bound(base) {
        return true;
    }

    let points: &[[f64; 2]] = match element {
        // Elbow arrows carry their own bound (`linear.rs`, ported from Excalidraw's
        // `generateElbowArrowShape`): once a point exceeds `GEOMETRY_BOUND` the arrow draws
        // as an empty (but valid) list of drawables, without ever handing the huge
        // coordinate to rough.js, and legitimate elbow arrows can be this large (see the
        // `arrow/elbow/extreme` baseline case). So elbow arrows are exempt from the
        // width/height/point-coordinate bound below, though not from
        // `base_exceeds_geometry_bound` above: `roughness`/`strokeWidth`/`roundness.value`
        // are not the "big coordinate" that exemption is for, and a negative `roughness`
        // aborts an elbow arrow's path the same way it does every other shape.
        Element::Arrow(l) if l.elbowed == Some(true) => return false,
        Element::Line(l) | Element::Arrow(l) => &l.points,
        Element::Freedraw(f) => &f.points,
        _ => &[],
    };

    exceeds_geometry_bound(base.width)
        || exceeds_geometry_bound(base.height)
        || points
            .iter()
            .any(|p| exceeds_geometry_bound(p[0]) || exceeds_geometry_bound(p[1]))
}

/// `_generateElementShape`, minus the `isExporting`/`embedsValidationStatus` parameters:
/// napkin has no iframe/embeddable rendering path (those load as `Element::Raw`), so
/// `modifyIframeLikeForRoughOptions` is never reached and its inputs are dropped.
/// `theme === THEME.DARK` is `ctx.dark_mode`.
pub fn generate_element_shape(element: &Element, ctx: &ShapeContext) -> ElementShape {
    if geometry_exceeds_bound(element) {
        return ElementShape::Placeholder;
    }
    let generator = RoughGenerator::new();
    match element {
        Element::Rectangle(g) => match generic::rectangle(&generator, element, g, ctx.dark_mode) {
            Some(d) => ElementShape::Drawables(vec![d]),
            None => ElementShape::Placeholder,
        },
        Element::Diamond(g) => match generic::diamond(&generator, element, g, ctx.dark_mode) {
            Some(d) => ElementShape::Drawables(vec![d]),
            None => ElementShape::Placeholder,
        },
        Element::Ellipse(g) => ElementShape::Drawables(vec![generic::ellipse(
            &generator,
            element,
            g,
            ctx.dark_mode,
        )]),
        Element::Line(l) | Element::Arrow(l) => ElementShape::Drawables(linear::shape(
            &generator,
            element,
            l,
            ctx.dark_mode,
            ctx.canvas_background_color,
        )),
        Element::Freedraw(f) => freedraw::shape(&generator, element, f, ctx.dark_mode),
        // `stickynote`/`frame`/`magicframe`/`text`/`image` all return `null` in the JS;
        // napkin has no typed stickynote/frame/magicframe/image element, so those load as
        // `Element::Raw` and land here too. `iframe`/`embeddable` also load as `Raw` here,
        // but JS draws a rough rectangle for them, not `null`: napkin instead draws the
        // dashed placeholder box spec §1.2 uses for every `Raw` element, so on screen they
        // look different from excalidraw.com by design (spec §1.2's table).
        Element::Text(_) | Element::Raw(_) => ElementShape::None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::element::Element;

    use super::{ElementShape, ShapeContext, generate_element_shape};

    fn ctx() -> ShapeContext<'static> {
        ShapeContext {
            dark_mode: false,
            canvas_background_color: "#ffffff",
        }
    }

    /// Asserts `element` loaded as a typed element (not `Raw`, which would make the test
    /// pass for the wrong reason) and that its shape is `Placeholder`.
    fn assert_placeholder(element: Element) {
        assert!(
            !matches!(element, Element::Raw(_)),
            "test element failed to load typed"
        );
        assert_eq!(
            generate_element_shape(&element, &ctx()),
            ElementShape::Placeholder
        );
    }

    #[test]
    fn huge_width_rectangle_with_extreme_roundness_value_is_placeholder() {
        let value = json!({
            "id": "r1", "type": "rectangle", "x": 0, "y": 0, "width": 1e308, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 3, "value": -1e308}, "seed": 1, "version": 1,
            "versionNonce": 1, "isDeleted": false, "boundElements": null, "updated": 1,
            "link": null, "locked": false
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn huge_negative_width_diamond_is_placeholder() {
        let value = json!({
            "id": "d1", "type": "diamond", "x": 0, "y": 0, "width": -1.79e308, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 2}, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn rounded_rectangle_above_bound_is_placeholder() {
        let value = json!({
            "id": "r2", "type": "rectangle", "x": 0, "y": 0, "width": 3.3e15, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 2}, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn freedraw_hachure_loop_around_extreme_coordinate_is_placeholder() {
        // Repro for the hachure loop-fill hang in Important 1: a closed freedraw loop with
        // points around 1.2e9, a non-transparent background and `fillStyle: "hachure"`.
        let c = 1.2e9;
        let value = json!({
            "id": "f1", "type": "freedraw", "x": c, "y": c, "width": 10, "height": 10,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "#ffd43b",
            "fillStyle": "hachure", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": null, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false,
            "points": [[c, c], [c + 10.0, c], [c + 10.0, c + 10.0], [c, c + 10.0], [c, c]],
            "pressures": [],
            "simulatePressure": true
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn negative_roughness_rounded_rectangle_is_placeholder() {
        let value = json!({
            "id": "r4", "type": "rectangle", "x": 0, "y": 0, "width": 200, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": -2,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 3}, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn negative_roughness_diamond_is_placeholder() {
        let value = json!({
            "id": "d2", "type": "diamond", "x": 0, "y": 0, "width": 200, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": -2,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 2}, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn negative_roughness_elbow_arrow_is_placeholder() {
        let value = json!({
            "id": "a1", "type": "arrow", "x": 0, "y": 0, "width": 200, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": -2,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": null, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false,
            "points": [[0, 0], [100, 0], [100, 100], [200, 100]],
            "startArrowhead": null, "endArrowhead": null, "elbowed": true
        });
        assert_placeholder(Element::from_value(value));
    }

    #[test]
    fn rounded_rectangle_at_bound_still_draws() {
        let value = json!({
            "id": "r3", "type": "rectangle", "x": 0, "y": 0, "width": 1e6, "height": 100,
            "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
            "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 0,
            "opacity": 100, "groupIds": [], "frameId": null, "index": "a0",
            "roundness": {"type": 2}, "seed": 1, "version": 1, "versionNonce": 1,
            "isDeleted": false, "boundElements": null, "updated": 1, "link": null,
            "locked": false
        });
        let element = Element::from_value(value);
        assert!(
            !matches!(element, Element::Raw(_)),
            "test element failed to load typed"
        );
        assert!(matches!(
            generate_element_shape(&element, &ctx()),
            ElementShape::Drawables(_)
        ));
    }
}
