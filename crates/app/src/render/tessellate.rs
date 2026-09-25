//! Tessellating an element's [`scene::shape::ElementShape`] into a GPU-ready [`Mesh`]: lyon
//! fills rough.js's solid/pattern fills and strokes its outlines, canvas dashing splits dashed
//! strokes first, and every vertex is placed in scene coordinates by the element's placement
//! and rotation.

use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use scene::shape::ElementShape;
use scene::{Element, Placement};

use crate::camera::SceneRect;
use crate::render::color::{Rgba, render_color};
use crate::render::path;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Indices relative to the first vertex of this mesh.
    pub indices: Vec<u32>,
    /// One index range per drawn layer, bottom to top.
    pub parts: Vec<std::ops::Range<u32>>,
    pub bounds: Option<SceneRect>,
}

pub struct Style {
    pub dark: bool,
    /// Element opacity times frame opacity, 0 to 1.
    pub alpha: f32,
    /// Scene-unit tolerance for the zoom bucket.
    pub tolerance: f32,
}

/// The flattening/tessellation tolerance in logical (CSS) pixels for the nearest zoom bucket,
/// converted to scene units by [`tolerance_for_bucket`]. Excalidraw redraws on every frame at
/// screen resolution; napkin instead tessellates once per zoom bucket, so the tolerance only
/// needs to be tight enough that no bucket's approximation is visible.
pub const VIEW_TOLERANCE_PX: f32 = 0.25;

/// `VIEW_TOLERANCE_PX` converted from view pixels to scene units for zoom bucket `bucket`
/// (`Camera::bucket`'s `ceil(log2(zoom * pixels_per_point))`): a scene unit spans at least
/// `2^bucket` view pixels in that bucket, so `VIEW_TOLERANCE_PX` view pixels are at most
/// `VIEW_TOLERANCE_PX / 2^bucket` scene units.
pub fn tolerance_for_bucket(bucket: i32) -> f32 {
    VIEW_TOLERANCE_PX / 2f32.powi(bucket)
}

/// The local rotation center (decision 10): `(width/2, height/2)` for most elements, the
/// center of `points`'s bounding box for line, arrow and freedraw.
pub fn local_center(element: &Element) -> Option<[f64; 2]> {
    let placement = element.placement()?;
    let points: Option<&[[f64; 2]]> = match element {
        Element::Line(l) | Element::Arrow(l) => Some(&l.points),
        Element::Freedraw(f) => Some(&f.points),
        _ => None,
    };
    let bbox_center = points.and_then(|points| {
        let &first = points.first()?;
        let mut min = first;
        let mut max = first;
        for &p in points {
            min[0] = min[0].min(p[0]);
            min[1] = min[1].min(p[1]);
            max[0] = max[0].max(p[0]);
            max[1] = max[1].max(p[1]);
        }
        Some([(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0])
    });
    Some(bbox_center.unwrap_or([placement.width / 2.0, placement.height / 2.0]))
}

/// `[x, y] + center + R(angle)(local - center)`.
fn transform(local: [f32; 2], placement: &Placement, center: [f64; 2]) -> [f32; 2] {
    let (sin, cos) = placement.angle.sin_cos();
    let dx = local[0] as f64 - center[0];
    let dy = local[1] as f64 - center[1];
    let x = placement.x + center[0] + dx * cos - dy * sin;
    let y = placement.y + center[1] + dx * sin + dy * cos;
    [x as f32, y as f32]
}

/// The placement, rotation center and style shared by every part of one element's mesh.
struct Frame<'a> {
    placement: Placement,
    center: [f64; 2],
    style: &'a Style,
}

struct Builder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    parts: Vec<std::ops::Range<u32>>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            vertices: Vec::new(),
            indices: Vec::new(),
            parts: Vec::new(),
        }
    }

    fn absorb(&mut self, buffers: VertexBuffers<Vertex, u32>) {
        if buffers.indices.is_empty() {
            return;
        }
        let base = self.vertices.len() as u32;
        let start = self.indices.len() as u32;
        self.vertices.extend(buffers.vertices);
        self.indices
            .extend(buffers.indices.into_iter().map(|i| i + base));
        self.parts.push(start..self.indices.len() as u32);
    }

    fn fill(&mut self, shape: &Path, color: Rgba, rule: FillRule, frame: &Frame) {
        let color = [color[0], color[1], color[2], color[3] * frame.style.alpha];
        let mut buffers: VertexBuffers<Vertex, u32> = VertexBuffers::new();
        let ctor = |v: FillVertex| Vertex {
            position: transform(v.position().to_array(), &frame.placement, frame.center),
            color,
        };
        let options = FillOptions::tolerance(frame.style.tolerance).with_fill_rule(rule);
        match FillTessellator::new().tessellate_path(
            shape,
            &options,
            &mut BuffersBuilder::new(&mut buffers, ctor),
        ) {
            Ok(_) => self.absorb(buffers),
            // Tessellation runs once per zoom-bucket cache miss, not once per frame, so this
            // does not spam. The part is dropped rather than panicking: a crafted or buggy
            // file must not take napkin down, but a silently vanished part left nothing to
            // grep for.
            Err(error) => eprintln!("napkin: fill tessellation failed: {error}"),
        }
    }

    fn stroke(&mut self, shape: &Path, color: Rgba, width: f32, frame: &Frame) {
        let color = [color[0], color[1], color[2], color[3] * frame.style.alpha];
        let mut buffers: VertexBuffers<Vertex, u32> = VertexBuffers::new();
        let ctor = |v: StrokeVertex| Vertex {
            position: transform(v.position().to_array(), &frame.placement, frame.center),
            color,
        };
        let options = StrokeOptions::tolerance(frame.style.tolerance)
            .with_line_width(width)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round);
        match StrokeTessellator::new().tessellate_path(
            shape,
            &options,
            &mut BuffersBuilder::new(&mut buffers, ctor),
        ) {
            Ok(_) => self.absorb(buffers),
            // See the matching comment in `fill`: tessellation is cached per zoom bucket, and
            // the part is dropped (not a panic) so a crafted file cannot take napkin down.
            Err(error) => eprintln!("napkin: stroke tessellation failed: {error}"),
        }
    }

    fn finish(self) -> Mesh {
        let bounds = bounds_of(&self.vertices);
        Mesh {
            vertices: self.vertices,
            indices: self.indices,
            parts: self.parts,
            bounds,
        }
    }
}

fn bounds_of(vertices: &[Vertex]) -> Option<SceneRect> {
    let first = vertices.first()?;
    let mut min = [first.position[0] as f64, first.position[1] as f64];
    let mut max = min;
    for v in &vertices[1..] {
        min[0] = min[0].min(v.position[0] as f64);
        min[1] = min[1].min(v.position[1] as f64);
        max[0] = max[0].max(v.position[0] as f64);
        max[1] = max[1].max(v.position[1] as f64);
    }
    Some(SceneRect { min, max })
}

/// A rough.js color string as `Rgba`: `"none"` (rough's "do not draw this layer" sentinel)
/// skips the part; a color scene already dark-mode-filtered is read back literally
/// (`dark: false`).
fn drawable_color(value: &str) -> Option<Rgba> {
    if value == "none" {
        None
    } else {
        Some(render_color(value, false))
    }
}

/// `shape`'s fill rule (`bin/canvas.js`'s `draw`, `fillPath`'s `const fillRule = (drawable.shape
/// === 'curve' || drawable.shape === 'polygon' || drawable.shape === 'path') ? 'evenodd' :
/// 'nonzero'`).
fn fill_rule_for(shape: rough::Shape) -> FillRule {
    match shape {
        rough::Shape::Curve | rough::Shape::Polygon | rough::Shape::Path => FillRule::EvenOdd,
        _ => FillRule::NonZero,
    }
}

fn push_op_set(
    builder: &mut Builder,
    set: &rough::OpSet,
    options: &rough::ResolvedOptions,
    shape: rough::Shape,
    frame: &Frame,
) {
    match set.kind {
        rough::OpSetType::Path => {
            let Some(color) = drawable_color(&options.stroke) else {
                return;
            };
            let mut outline = path::ops_path(&set.ops);
            if let Some(dash) = options
                .stroke_line_dash
                .as_deref()
                .filter(|d| !d.is_empty())
            {
                let pattern: Vec<f32> = dash.iter().map(|&v| v as f32).collect();
                outline = path::dashed(&outline, &pattern, frame.style.tolerance);
            }
            builder.stroke(&outline, color, options.stroke_width as f32, frame);
        }
        rough::OpSetType::FillPath => {
            let Some(fill) = options.fill.as_deref() else {
                return;
            };
            let Some(color) = drawable_color(fill) else {
                return;
            };
            let filled = path::ops_path(&set.ops);
            builder.fill(&filled, color, fill_rule_for(shape), frame);
        }
        rough::OpSetType::FillSketch => {
            let Some(fill) = options.fill.as_deref() else {
                return;
            };
            let Some(color) = drawable_color(fill) else {
                return;
            };
            let width = if options.fill_weight < 0.0 {
                options.stroke_width / 2.0
            } else {
                options.fill_weight
            };
            let mut sketch = path::ops_path(&set.ops);
            if let Some(dash) = options.fill_line_dash.as_deref().filter(|d| !d.is_empty()) {
                let pattern: Vec<f32> = dash.iter().map(|&v| v as f32).collect();
                sketch = path::dashed(&sketch, &pattern, frame.style.tolerance);
            }
            builder.stroke(&sketch, color, width as f32, frame);
        }
    }
}

/// The dashed placeholder box drawn for [`ElementShape::Placeholder`] and [`Element::Raw`]
/// (spec §1.2): a `[6, 4]`-dashed `#868e96` rectangle over `(0, 0)`-`(width, height)`.
fn push_placeholder(builder: &mut Builder, frame: &Frame) {
    let color = render_color("#868e96", frame.style.dark);
    let mut box_builder = Path::builder();
    let width = frame.placement.width as f32;
    let height = frame.placement.height as f32;
    box_builder.begin(point(0.0, 0.0));
    box_builder.line_to(point(width, 0.0));
    box_builder.line_to(point(width, height));
    box_builder.line_to(point(0.0, height));
    box_builder.end(true);
    let outline = path::dashed(&box_builder.build(), &[6.0, 4.0], frame.style.tolerance);
    builder.stroke(&outline, color, 1.0, frame);
}

/// Tessellates `element`'s `shape` into scene-space triangles. Elements without a placement
/// (a malformed `Raw` element) tessellate to an empty mesh.
pub fn tessellate(element: &Element, shape: &ElementShape, style: &Style) -> Mesh {
    let mut builder = Builder::new();

    let Some(center) = local_center(element) else {
        return builder.finish();
    };
    let placement = element
        .placement()
        .expect("local_center returned Some, so placement is Some too");
    let frame = Frame {
        placement,
        center,
        style,
    };

    if matches!(element, Element::Raw(_)) || matches!(shape, ElementShape::Placeholder) {
        push_placeholder(&mut builder, &frame);
        return builder.finish();
    }

    match shape {
        ElementShape::None | ElementShape::Placeholder => {}
        ElementShape::Drawables(drawables) => {
            for drawable in drawables {
                for set in &drawable.sets {
                    push_op_set(&mut builder, set, &drawable.options, drawable.shape, &frame);
                }
            }
        }
        ElementShape::Freedraw { fill, stroke } => {
            if let Some(fill) = fill {
                for set in &fill.sets {
                    push_op_set(&mut builder, set, &fill.options, fill.shape, &frame);
                }
            }
            // Excalidraw applies the dark-mode filter to a freedraw stroke's color when it
            // draws the stroke, not when it builds the shape, unlike every other drawable
            // color above.
            let stroke_color = element
                .base()
                .map(|b| b.stroke_color.as_str())
                .unwrap_or("#000");
            let color = render_color(stroke_color, frame.style.dark);
            let outline = path::outline_path(stroke);
            builder.fill(&outline, color, FillRule::NonZero, &frame);
        }
    }

    builder.finish()
}

#[cfg(test)]
mod tests {
    use scene::Element;
    use scene::shape::{ShapeContext, generate_element_shape};
    use serde_json::json;

    use super::*;
    use scene::sample;

    /// `FillTessellator::tessellate_path` returns `Err(UnsupportedParamater::ToleranceIsNaN)`
    /// for a NaN tolerance without touching the path at all (verified against lyon_tessellation
    /// 1.0.22's `fill.rs`), so this drives `Builder::fill`'s error branch without needing a
    /// path lyon would reject outright (a NaN *coordinate* instead panics much earlier, in
    /// `lyon_path`'s own `debug_assert!(p.x.is_finite())`, before any tessellator runs).
    #[test]
    fn fill_tessellation_error_is_dropped_without_panicking() {
        let mut path_builder = lyon::path::Path::builder();
        path_builder.begin(lyon::math::point(0.0, 0.0));
        path_builder.line_to(lyon::math::point(10.0, 0.0));
        path_builder.line_to(lyon::math::point(10.0, 10.0));
        path_builder.end(true);
        let path = path_builder.build();

        let style = Style {
            dark: false,
            alpha: 1.0,
            tolerance: f32::NAN,
        };
        let frame = Frame {
            placement: Placement {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                angle: 0.0,
            },
            center: [5.0, 5.0],
            style: &style,
        };
        let mut builder = Builder::new();
        builder.fill(&path, [1.0, 0.0, 0.0, 1.0], FillRule::NonZero, &frame);
        let mesh = builder.finish();

        assert!(mesh.parts.is_empty());
        assert!(mesh.vertices.is_empty());
    }

    fn mesh_of(value: serde_json::Value, alpha: f32) -> Mesh {
        let element = Element::from_value(value);
        let ctx = ShapeContext {
            dark_mode: false,
            canvas_background_color: "#ffffff",
        };
        let shape = generate_element_shape(&element, &ctx);
        tessellate(
            &element,
            &shape,
            &Style {
                dark: false,
                alpha,
                tolerance: 0.25,
            },
        )
    }

    fn solid_rectangle(angle: f64) -> serde_json::Value {
        sample::with(
            sample::generic("rectangle", "r", [10.0, 20.0, 100.0, 50.0]),
            json!({ "roughness": 0, "backgroundColor": "#ffc9c9", "fillStyle": "solid", "angle": angle }),
        )
    }

    #[test]
    fn solid_rectangle_has_fill_then_stroke() {
        let mesh = mesh_of(solid_rectangle(0.0), 1.0);
        assert_eq!(mesh.parts.len(), 2);
        let first = mesh.vertices[mesh.indices[mesh.parts[0].start as usize] as usize];
        let fill = css_color_or_panic("#ffc9c9");
        assert_eq!(first.color, fill);
        let bounds = mesh.bounds.expect("non-empty mesh");
        assert!(bounds.min[0] <= 10.0 && bounds.min[0] >= 7.0, "{bounds:?}");
        assert!(bounds.max[1] >= 70.0 && bounds.max[1] <= 73.0, "{bounds:?}");
    }

    #[test]
    fn rotation_uses_the_element_center() {
        let mesh = mesh_of(solid_rectangle(std::f64::consts::FRAC_PI_2), 1.0);
        let bounds = mesh.bounds.expect("non-empty mesh");
        // Center (60, 45); a quarter turn swaps the 100x50 extents.
        assert!(
            (bounds.min[0] - 35.0).abs() < 3.0 && (bounds.max[0] - 85.0).abs() < 3.0,
            "{bounds:?}"
        );
        assert!(
            (bounds.min[1] + 5.0).abs() < 3.0 && (bounds.max[1] - 95.0).abs() < 3.0,
            "{bounds:?}"
        );
    }

    #[test]
    fn alpha_multiplies_vertex_colors() {
        let mesh = mesh_of(solid_rectangle(0.0), 0.5);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| (v.color[3] - 0.5).abs() < 1e-6)
        );
    }

    #[test]
    fn raw_elements_become_dashed_placeholder_boxes() {
        let image = json!({ "id": "i", "type": "image", "x": 0, "y": 0, "width": 40, "height": 30, "angle": 0 });
        let mesh = mesh_of(image, 1.0);
        assert!(!mesh.indices.is_empty());
        let bounds = mesh.bounds.expect("non-empty mesh");
        assert!(bounds.max[0] <= 41.0 && bounds.max[1] <= 31.0, "{bounds:?}");
    }

    #[test]
    fn freedraw_outline_uses_the_stroke_color() {
        let value = sample::with(
            sample::freedraw("f", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]]),
            json!({ "strokeColor": "#e03131" }),
        );
        let mesh = mesh_of(value, 1.0);
        let last = mesh.parts.last().expect("outline part");
        let color = mesh.vertices[mesh.indices[last.start as usize] as usize].color;
        assert_eq!(color, css_color_or_panic("#e03131"));
    }

    #[test]
    fn even_odd_fill_leaves_the_hole_uncovered() {
        let mut builder = lyon::path::Path::builder();
        for (lo, hi) in [(0.0, 100.0), (25.0, 75.0)] {
            builder.begin(lyon::math::point(lo, lo));
            builder.line_to(lyon::math::point(hi, lo));
            builder.line_to(lyon::math::point(hi, hi));
            builder.line_to(lyon::math::point(lo, hi));
            builder.end(true);
        }
        let path = builder.build();
        assert!(covers(
            &fill_for_test(&path, lyon::tessellation::FillRule::NonZero),
            [50.0, 50.0]
        ));
        assert!(!covers(
            &fill_for_test(&path, lyon::tessellation::FillRule::EvenOdd),
            [50.0, 50.0]
        ));
    }

    fn css_color_or_panic(value: &str) -> [f32; 4] {
        crate::render::color::css_color(value).expect("valid color")
    }

    /// Whether any triangle contains `p`.
    fn covers(buffers: &lyon::tessellation::VertexBuffers<[f32; 2], u32>, p: [f32; 2]) -> bool {
        buffers.indices.chunks(3).any(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| buffers.vertices[t[i] as usize]);
            let side = |u: [f32; 2], v: [f32; 2]| {
                (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
            };
            let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
            !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
        })
    }

    fn fill_for_test(
        path: &lyon::path::Path,
        rule: lyon::tessellation::FillRule,
    ) -> lyon::tessellation::VertexBuffers<[f32; 2], u32> {
        use lyon::tessellation::{
            BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers,
        };
        let mut buffers = VertexBuffers::new();
        FillTessellator::new()
            .tessellate_path(
                path,
                &FillOptions::tolerance(0.1).with_fill_rule(rule),
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
            )
            .expect("tessellates");
        buffers
    }
}
