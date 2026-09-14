//! Turning an [`Element`](crate::element::Element) into rough.js drawables and freedraw
//! outlines (`packages/element/src/shape.ts` at the pinned commit). Task 8 ports
//! `generateRoughOptions`; the generators that build the shapes themselves are Tasks 9-12.

mod arrowhead;
mod freedraw;
mod generic;
mod linear;
mod options;

use rough::RoughGenerator;

use crate::element::Element;

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
/// outline fills, ported in a later task).
pub struct ShapeContext<'a> {
    pub dark_mode: bool,
    pub canvas_background_color: &'a str,
}

/// What an element draws: rough.js primitives for rectangle/diamond/ellipse/line/arrow,
/// a background fill plus stroke outline for freedraw, or nothing for element types rough.js
/// never draws (text, image, frame, ...).
#[derive(Clone, Debug, PartialEq)]
pub enum ElementShape {
    None,
    Drawables(Vec<rough::Drawable>),
    Freedraw {
        fill: Option<Box<rough::Drawable>>,
        stroke: Vec<PathOp>,
    },
}

/// `_generateElementShape`, minus the `isExporting`/`embedsValidationStatus` parameters:
/// napkin has no iframe/embeddable rendering path (those load as `Element::Raw`), so
/// `modifyIframeLikeForRoughOptions` is never reached and its inputs are dropped.
/// `theme === THEME.DARK` is `ctx.dark_mode`.
pub fn generate_element_shape(element: &Element, ctx: &ShapeContext) -> ElementShape {
    let generator = RoughGenerator::new();
    match element {
        Element::Rectangle(g) => ElementShape::Drawables(vec![generic::rectangle(
            &generator,
            element,
            g,
            ctx.dark_mode,
        )]),
        Element::Diamond(g) => ElementShape::Drawables(vec![generic::diamond(
            &generator,
            element,
            g,
            ctx.dark_mode,
        )]),
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
        Element::Freedraw(_) => todo!("freedraw shapes: Task 12"),
        // `stickynote`/`frame`/`magicframe`/`text`/`image` all return `null` in the JS;
        // napkin has no typed stickynote/frame/magicframe/image element, so those load as
        // `Element::Raw` and land here too.
        Element::Text(_) | Element::Raw(_) => ElementShape::None,
    }
}
