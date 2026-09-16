//! Building one frame's draw list from the scene: file-order traversal, coarse then exact
//! culling against the visible rect, grouping neighbouring opaque elements into batches,
//! isolating translucent or label-holed elements behind the stencil buffer, and drawing
//! placeholders (spec §1.2) as an opaque box plus a type label.

use std::collections::HashMap;

use scene::Element;
use scene::shape::ElementShape;

use crate::camera::SceneRect;
use crate::render::cache::{MeshKey, SceneCache};
use crate::render::tessellate::local_center;

/// Coarse-cull margin added to an element's rotated bounds on top of `8 * strokeWidth`, in
/// scene units: generous enough that no drawable's stroke, rough.js overshoot or freedraw
/// outline is ever coarse-culled while still visible.
pub const COARSE_MARGIN: f64 = 64.0;

/// How far an arrow label's stencil hole extends past the label's own rect on every side,
/// matching Excalidraw's `clearRect` padding for bound text (`bin/canvas.js`).
pub const BOUND_TEXT_PADDING: f64 = 5.0;

/// Physical em size (device pixels: `fontSize * zoom * pixels_per_point`) above which an
/// unrotated text element routes through the same offscreen-texture path rotated text uses,
/// instead of glyphon's in-pass renderer. Continuous zoom times `fontSize` is otherwise
/// unbounded, and a single very large line can ask the shared glyph
/// atlas (capped at the device's `max_texture_dimension_2d`, typically 8192px) for more space
/// than it has; an 8192px atlas holds roughly 1000 glyphs at this size, comfortably more than
/// one frame's visible glyphs ever need.
pub const MAX_TEXT_EM_PX: f32 = 256.0;

/// Geometry this far from the origin is already excluded from every other shape (matches
/// `scene::shape::GEOMETRY_BOUND`, which is private to that crate); text uses the same bound
/// since nothing upstream clips a `TextElement`'s own placement.
const GEOMETRY_BOUND: f64 = 1e6;

fn in_bounds(v: f64) -> bool {
    v.is_finite() && v.abs() <= GEOMETRY_BOUND
}

/// False for a text element whose font metrics or geometry would panic or hang glyphon's
/// shaper: a non-finite or non-positive `fontSize` or effective line height
/// (`fontSize * lineHeight`), or a placement coordinate outside [`GEOMETRY_BOUND`].
fn text_is_drawable(text: &scene::element::TextElement) -> bool {
    let line_height = text.line_height.unwrap_or(1.25);
    let line_height_px = text.font_size * line_height;
    text.font_size.is_finite()
        && text.font_size > 0.0
        && line_height_px.is_finite()
        && line_height_px > 0.0
        && in_bounds(text.base.x)
        && in_bounds(text.base.y)
        && in_bounds(text.base.width)
        && in_bounds(text.base.height)
}

/// Whether a text element should render through the offscreen-texture path: any rotation, or a
/// physical em size past [`MAX_TEXT_EM_PX`].
fn needs_offscreen_text(font_size: f64, angle: f64, pixel_scale: f32) -> bool {
    angle != 0.0 || (font_size as f32 * pixel_scale) > MAX_TEXT_EM_PX
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementDraw {
    pub element: usize,
    pub key: MeshKey,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawItem {
    Meshes(Vec<ElementDraw>),
    /// Drawn top layer first with stencil reference `stencil`; `hole` is the scene-space
    /// quad masked out before drawing (an arrow label).
    Isolated {
        draw: ElementDraw,
        stencil: u8,
        hole: Option<[[f64; 2]; 4]>,
    },
    /// Writes 0 to the whole stencil buffer before references wrap around.
    StencilReset,
    /// Consecutive unrotated texts; `label` marks a placeholder's type label.
    Text(Vec<TextDraw>),
    RotatedText(TextDraw),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextDraw {
    pub element: usize,
    pub label: bool,
    /// The element's own opacity times its frame's opacity (0-1), exactly like a mesh's alpha;
    /// 1.0 for a placeholder's type label, which (like its dashed box) is napkin's own hint and
    /// always opaque.
    pub alpha: f32,
}

pub struct View {
    pub visible: SceneRect,
    pub dark: bool,
    pub bucket: i32,
    /// Physical pixels per scene unit at the current zoom (`camera.zoom * pixels_per_point`):
    /// how large a text element's glyphs actually rasterize at, used to route very large text
    /// through the offscreen path instead of glyphon's in-pass renderer.
    pub pixel_scale: f32,
}

/// A batch of consecutive same-kind draws waiting to be flushed into a single `DrawItem`, so
/// unrelated neighbours stay in the same draw call instead of alternating one-by-one.
enum Pending {
    None,
    Meshes(Vec<ElementDraw>),
    Text(Vec<TextDraw>),
}

impl Pending {
    fn flush(&mut self, items: &mut Vec<DrawItem>) {
        match std::mem::replace(self, Pending::None) {
            Pending::None => {}
            Pending::Meshes(draws) => items.push(DrawItem::Meshes(draws)),
            Pending::Text(draws) => items.push(DrawItem::Text(draws)),
        }
    }

    fn push_mesh(&mut self, items: &mut Vec<DrawItem>, draw: ElementDraw) {
        match self {
            Pending::Meshes(draws) => draws.push(draw),
            _ => {
                self.flush(items);
                *self = Pending::Meshes(vec![draw]);
            }
        }
    }

    fn push_text(&mut self, items: &mut Vec<DrawItem>, draw: TextDraw) {
        match self {
            Pending::Text(draws) => draws.push(draw),
            _ => {
                self.flush(items);
                *self = Pending::Text(vec![draw]);
            }
        }
    }
}

/// The bounding box (`local_min`, `local_max`) of `element`'s points for line/arrow/freedraw,
/// or `(0, 0)`-`(width, height)` for every other element type, in element-local coordinates
/// (relative to the placement origin, matching `SceneRect::of_rotated`).
fn local_extent(element: &Element, width: f64, height: f64) -> ([f64; 2], [f64; 2]) {
    let points: Option<&[[f64; 2]]> = match element {
        Element::Line(l) | Element::Arrow(l) => Some(&l.points),
        Element::Freedraw(f) => Some(&f.points),
        _ => None,
    };
    let Some(points) = points else {
        return ([0.0, 0.0], [width, height]);
    };
    let Some(&first) = points.first() else {
        return ([0.0, 0.0], [0.0, 0.0]);
    };
    let mut min = first;
    let mut max = first;
    for &p in points {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    (min, max)
}

/// `element`'s rotated bounds with no margin: the union `content_bounds` reduces over.
fn local_bounds(element: &Element) -> Option<SceneRect> {
    let placement = element.placement()?;
    // `local_center` returns `Some` whenever `placement()` does (it re-derives the same
    // placement internally), so this is infallible here.
    let center = local_center(element)?;
    let (local_min, local_max) = local_extent(element, placement.width, placement.height);
    Some(SceneRect::of_rotated(
        &placement, local_min, local_max, center,
    ))
}

/// `local_bounds` expanded by [`COARSE_MARGIN`] plus `8 * strokeWidth`, for the coarse cull
/// pass. `None` for an element with no placement (a malformed `Raw` element).
fn coarse_bounds(element: &Element) -> Option<SceneRect> {
    let rect = local_bounds(element)?;
    let stroke_width = element.base().map(|b| b.stroke_width).unwrap_or(0.0);
    Some(rect.expand(COARSE_MARGIN + 8.0 * stroke_width))
}

/// The scene-space hole an arrow's bound label punches through the arrow's fill/stroke:
/// `label`'s unrotated placement rect, expanded by [`BOUND_TEXT_PADDING`] on every side.
/// Corners are top-left, top-right, bottom-right, bottom-left, ignoring the label's own
/// rotation (Excalidraw clears the same axis-aligned rect regardless of label angle).
fn label_hole(label: &scene::Placement) -> [[f64; 2]; 4] {
    let min = [label.x - BOUND_TEXT_PADDING, label.y - BOUND_TEXT_PADDING];
    let max = [
        label.x + label.width + BOUND_TEXT_PADDING,
        label.y + label.height + BOUND_TEXT_PADDING,
    ];
    [
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ]
}

/// `element`'s own opacity times its containing frame's opacity (each 0-100, so the product is
/// divided by 10000), matching Excalidraw's compounded rendering alpha.
fn element_alpha(element: &Element, frame_opacity: &HashMap<&str, f64>) -> f32 {
    let own = element.opacity() / 100.0;
    let frame = element
        .frame_id()
        .and_then(|frame_id| frame_opacity.get(frame_id))
        .copied()
        .unwrap_or(100.0)
        / 100.0;
    (own * frame) as f32
}

/// Every non-deleted frame-like element's `id` -> its own `opacity` (0-100), which multiplies
/// into its children's alpha. Excalidraw's `getRenderOpacity` looks up whatever element
/// `frameId` points to, not only `type: "frame"`: a `magicframe` is a valid frame container too.
fn frame_opacities(elements: &[Element]) -> HashMap<&str, f64> {
    elements
        .iter()
        .filter(|e| !e.is_deleted() && matches!(e.kind(), "frame" | "magicframe"))
        .filter_map(|e| Some((e.id()?, e.opacity())))
        .collect()
}

/// Every non-deleted text element's `containerId` -> that text's index, restricted to the
/// arrow labels `plan_frame` needs (an arrow's bound text is the only label that punches a
/// stencil hole; container labels for rectangle/diamond/ellipse are plain clipped text with
/// no hole).
fn arrow_labels(elements: &[Element]) -> HashMap<&str, usize> {
    elements
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.is_deleted())
        .filter_map(|(i, e)| match e {
            Element::Text(t) => Some((t.container_id.value()?.as_str(), i)),
            _ => None,
        })
        .collect()
}

/// The draw list for one frame, in file order.
pub fn plan_frame(file: &scene::SceneFile, cache: &mut SceneCache, view: &View) -> Vec<DrawItem> {
    cache.begin_frame();
    let elements = &file.elements;
    let frame_opacity = frame_opacities(elements);
    let arrow_label = arrow_labels(elements);
    let canvas_background = file.view_background_color();

    let mut items = Vec::new();
    let mut pending = Pending::None;
    let mut stencil = 0u8;

    for (index, element) in elements.iter().enumerate() {
        if element.is_deleted() {
            continue;
        }

        if let Element::Text(text) = element {
            if !text_is_drawable(text) {
                cache.log_invalid_text_once(&text.base.id);
                continue;
            }
            let Some(coarse) = coarse_bounds(element) else {
                continue;
            };
            if !coarse.intersects(&view.visible) {
                continue;
            }
            let draw = TextDraw {
                element: index,
                label: false,
                alpha: element_alpha(element, &frame_opacity),
            };
            if needs_offscreen_text(text.font_size, text.base.angle, view.pixel_scale) {
                pending.flush(&mut items);
                items.push(DrawItem::RotatedText(draw));
            } else {
                pending.push_text(&mut items, draw);
            }
            continue;
        }

        let Some(coarse) = coarse_bounds(element) else {
            continue;
        };
        if !coarse.intersects(&view.visible) {
            continue;
        }

        let is_raw = matches!(element, Element::Raw(_));
        let id = element.id().unwrap_or_default();
        let version_bits = element.version().to_bits();
        let is_placeholder = if is_raw {
            true
        } else {
            matches!(
                *cache.shape(
                    element,
                    index,
                    id,
                    version_bits,
                    view.dark,
                    canvas_background
                ),
                ElementShape::Placeholder
            )
        };

        let alpha: f32 = if is_placeholder {
            1.0
        } else {
            element_alpha(element, &frame_opacity)
        };

        let key = MeshKey {
            index,
            id: id.to_owned(),
            version_bits,
            dark: view.dark,
            alpha_bits: alpha.to_bits(),
            bucket: view.bucket,
        };
        let cached = cache.mesh(element, &key, canvas_background);
        let Some(bounds) = cached.mesh.bounds else {
            continue;
        };
        if !bounds.intersects(&view.visible) {
            continue;
        }
        let draw = ElementDraw {
            element: index,
            key,
        };

        if is_placeholder {
            // The dashed box and its type label are napkin's own hint, not the element's
            // content: always opaque (alpha was forced to 1 above) and never isolated.
            pending.push_mesh(&mut items, draw);
            pending.push_text(
                &mut items,
                TextDraw {
                    element: index,
                    label: true,
                    alpha: 1.0,
                },
            );
            continue;
        }

        let label = if matches!(element, Element::Arrow(_)) {
            arrow_label.get(id).copied()
        } else {
            None
        };

        if alpha < 1.0 || label.is_some() {
            pending.flush(&mut items);
            if stencil == 255 {
                items.push(DrawItem::StencilReset);
                stencil = 0;
            }
            stencil += 1;
            let hole = label.map(|label_index| {
                let placement = elements[label_index]
                    .placement()
                    .expect("a Text element always has a placement");
                label_hole(&placement)
            });
            items.push(DrawItem::Isolated {
                draw,
                stencil,
                hole,
            });
        } else {
            pending.push_mesh(&mut items, draw);
        }
    }

    pending.flush(&mut items);
    items
}

/// Union of the coarse bounds of all non-deleted elements.
pub fn content_bounds(file: &scene::SceneFile) -> Option<SceneRect> {
    file.elements
        .iter()
        .filter(|e| !e.is_deleted())
        .filter_map(local_bounds)
        .reduce(|a, b| a.union(&b))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample;

    fn view() -> View {
        View {
            visible: SceneRect {
                min: [-1000.0, -1000.0],
                max: [1000.0, 1000.0],
            },
            dark: false,
            bucket: 0,
            pixel_scale: 1.0,
        }
    }

    fn kinds(items: &[DrawItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                DrawItem::Meshes(draws) => format!("meshes{}", draws.len()),
                DrawItem::Isolated { stencil, hole, .. } => {
                    format!(
                        "isolated{stencil}{}",
                        if hole.is_some() { "+hole" } else { "" }
                    )
                }
                DrawItem::StencilReset => "reset".to_owned(),
                DrawItem::Text(texts) => format!("text{}", texts.len()),
                DrawItem::RotatedText(_) => "rotated".to_owned(),
            })
            .collect()
    }

    #[test]
    fn keeps_file_order_and_groups_neighbours() {
        let file = sample::file(vec![
            sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]),
            sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 10.0]),
            sample::text("t", [0.0, 20.0, 40.0, 25.0], "hi", None),
            sample::with(
                sample::text("r", [0.0, 60.0, 40.0, 25.0], "turned", None),
                json!({ "angle": 1.0 }),
            ),
            sample::generic("diamond", "c", [40.0, 0.0, 10.0, 10.0]),
            sample::with(
                sample::generic("rectangle", "gone", [0.0, 0.0, 5.0, 5.0]),
                json!({ "isDeleted": true }),
            ),
        ]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes2", "text1", "rotated", "meshes1"]);
    }

    #[test]
    fn culls_elements_outside_the_view() {
        let file = sample::file(vec![
            sample::generic("rectangle", "near", [0.0, 0.0, 10.0, 10.0]),
            sample::generic("rectangle", "far", [5000.0, 5000.0, 10.0, 10.0]),
        ]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes1"]);
    }

    #[test]
    fn translucent_elements_are_isolated_and_references_wrap() {
        let translucent = |i: usize| {
            sample::with(
                sample::generic("rectangle", &format!("t{i}"), [0.0, 0.0, 10.0, 10.0]),
                json!({ "opacity": 50 }),
            )
        };
        let file = sample::file((0..256).map(translucent).collect());
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        let names = kinds(&items);
        assert_eq!(names[0], "isolated1");
        assert_eq!(names[254], "isolated255");
        assert_eq!(names[255], "reset");
        assert_eq!(names[256], "isolated1");
        let DrawItem::Isolated { draw, .. } = &items[0] else {
            panic!("isolated")
        };
        assert_eq!(draw.key.alpha_bits, 0.5f32.to_bits());
    }

    #[test]
    fn frame_opacity_multiplies_and_frames_are_placeholders() {
        let frame = json!({ "id": "f", "type": "frame", "x": -5, "y": -5, "width": 100, "height": 100, "angle": 0, "opacity": 50, "version": 1 });
        let child = sample::with(
            sample::generic("rectangle", "c", [0.0, 0.0, 10.0, 10.0]),
            json!({ "opacity": 50, "frameId": "f" }),
        );
        let file = sample::file(vec![frame, child]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes1", "text1", "isolated1"]);
        let DrawItem::Text(labels) = &items[1] else {
            panic!("label")
        };
        assert!(labels[0].label);
        let DrawItem::Isolated { draw, .. } = &items[2] else {
            panic!("isolated")
        };
        assert_eq!(draw.key.alpha_bits, 0.25f32.to_bits());
    }

    #[test]
    fn magicframe_opacity_multiplies_its_children_too() {
        let frame = json!({ "id": "f", "type": "magicframe", "x": -5, "y": -5, "width": 100, "height": 100, "angle": 0, "opacity": 40, "version": 1 });
        let child = sample::with(
            sample::generic("rectangle", "c", [0.0, 0.0, 10.0, 10.0]),
            json!({ "opacity": 50, "frameId": "f" }),
        );
        let file = sample::file(vec![frame, child]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["meshes1", "text1", "isolated1"]);
        let DrawItem::Isolated { draw, .. } = &items[2] else {
            panic!("isolated")
        };
        // 40% frame opacity * 50% element opacity = 20%.
        assert_eq!(draw.key.alpha_bits, 0.2f32.to_bits());
    }

    #[test]
    fn invalid_text_metrics_are_skipped_not_drawn() {
        let zero_font_size = sample::with(
            sample::text("a", [0.0, 0.0, 100.0, 25.0], "zero", None),
            json!({ "fontSize": 0 }),
        );
        let negative_font_size = sample::with(
            sample::text("b", [0.0, 30.0, 100.0, 25.0], "neg", None),
            json!({ "fontSize": -20 }),
        );
        let zero_line_height = sample::with(
            sample::text("c", [0.0, 60.0, 100.0, 25.0], "zero-lh", None),
            json!({ "lineHeight": 0 }),
        );
        let huge_x = sample::with(
            sample::text("d", [1e308, 0.0, 100.0, 25.0], "far", None),
            json!({}),
        );
        let file = sample::file(vec![
            zero_font_size,
            negative_font_size,
            zero_line_height,
            huge_x,
        ]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(items, Vec::new(), "an invalid text element was still drawn");
    }

    #[test]
    fn oversized_text_routes_through_the_offscreen_path() {
        let mut view = view();
        // fontSize 20 * pixel_scale 20 = 400px, past MAX_TEXT_EM_PX (256), with no rotation.
        view.pixel_scale = 20.0;
        let file = sample::file(vec![sample::text(
            "t",
            [0.0, 0.0, 200.0, 25.0],
            "big",
            None,
        )]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view);
        assert_eq!(kinds(&items), ["rotated"]);
    }

    #[test]
    fn labelled_arrows_get_a_padded_hole() {
        let arrow = sample::with(
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [200.0, 0.0]]),
            json!({ "boundElements": [{ "id": "label", "type": "text" }] }),
        );
        let label = sample::text("label", [80.0, -12.0, 40.0, 25.0], "hi", Some("a"));
        let file = sample::file(vec![arrow, label]);
        let items = plan_frame(&file, &mut SceneCache::new(), &view());
        assert_eq!(kinds(&items), ["isolated1+hole", "text1"]);
        let DrawItem::Isolated {
            hole: Some(hole), ..
        } = &items[0]
        else {
            panic!("hole")
        };
        assert_eq!(hole[0], [75.0, -17.0]);
        assert_eq!(hole[2], [125.0, 18.0]);
    }
}
