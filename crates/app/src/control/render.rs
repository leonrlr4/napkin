//! Render-region planning for the `render` control request: where the camera sits and how big
//! the output image is for each [`RenderTarget`], and PNG encoding of the RGBA8 pixels a
//! [`Rasterize`] produces. `handler::render` chains `plan` -> `Rasterize::rasterize` ->
//! `encode_png` -> a file write.

use std::sync::Arc;

use scene::SceneFile;
use scene::geometry::GeometryCache;
use scene::selection::Selection;

use crate::camera::Camera;
use crate::control::RenderTarget;
use crate::render::gpu::{CanvasFrame, CanvasRenderer};
use crate::render::offscreen::{self, GpuRasterizer};

/// Turns a planned scene into RGBA8 pixels: a live GPU renderer in the app
/// ([`GpuRasterizer`]), or a fake that records calls in tests.
pub trait Rasterize {
    /// RGBA8 rows of `size_px`, `scene` drawn with `camera` at pixels-per-point 1.
    fn rasterize(
        &mut self,
        scene: Arc<SceneFile>,
        camera: Camera,
        size_px: [u32; 2],
        dark: bool,
    ) -> Result<Vec<u8>, String>;
}

impl Rasterize for GpuRasterizer<'_> {
    fn rasterize(
        &mut self,
        scene: Arc<SceneFile>,
        camera: Camera,
        size_px: [u32; 2],
        dark: bool,
    ) -> Result<Vec<u8>, String> {
        let renderer = self
            .renderer
            .get_or_insert_with(|| CanvasRenderer::new(self.device, self.queue, offscreen::FORMAT));
        let frame = CanvasFrame {
            file: scene,
            camera,
            size_px,
            pixels_per_point: 1.0,
            dark,
            generation: 0,
        };
        Ok(offscreen::render_rgba(
            self.device,
            self.queue,
            renderer,
            &frame,
        ))
    }
}

/// The empty margin kept around the content on every side of an `all` or `selection` render.
pub const PADDING_PX: f64 = 16.0;
/// The content's long edge, in pixels, an `all` or `selection` render is scaled down to when
/// it would otherwise be larger; never scaled up past 2x.
pub const LONG_EDGE_PX: f64 = 1536.0;
/// The largest width or height a render is ever allowed to reach.
pub const MAX_SIDE_PX: u32 = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub scene: SceneFile,
    pub camera: Camera,
    pub size_px: [u32; 2],
}

/// `value` rounded toward `u32`, clamped to `1..=MAX_SIDE_PX`; a negative or `NaN` input
/// saturates to `0` before the clamp lifts it to `1`.
fn clamp_side(value: f64) -> u32 {
    (value as u32).clamp(1, MAX_SIDE_PX)
}

/// The camera and size that fit `bounds` with `PADDING_PX` on every side, shrunk (never
/// enlarged past 2x) so the long edge is at most `LONG_EDGE_PX`.
fn from_bounds(bounds: [f64; 4], scene: SceneFile) -> Plan {
    let [x1, y1, x2, y2] = bounds;
    let (content_w, content_h) = (x2 - x1, y2 - y1);
    let scale = (2.0_f64).min(LONG_EDGE_PX / content_w.max(content_h).max(1.0));
    let size_px = [
        clamp_side((content_w * scale).ceil() + 2.0 * PADDING_PX),
        clamp_side((content_h * scale).ceil() + 2.0 * PADDING_PX),
    ];
    let camera = Camera {
        zoom: scale,
        scroll_x: PADDING_PX / scale - x1,
        scroll_y: PADDING_PX / scale - y1,
    };
    Plan {
        scene,
        camera,
        size_px,
    }
}

/// Positions, in file order, of `selection`'s own elements plus any text elements whose
/// `containerId` points at one of them (a selected container's label).
fn selection_positions(file: &SceneFile, selection: &Selection) -> Vec<usize> {
    file.elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            !element.is_deleted()
                && (element.id().is_some_and(|id| selection.contains(id))
                    || element
                        .container_id()
                        .is_some_and(|container_id| selection.contains(container_id)))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Where and how big to render `file` for `target`. `all` and `selection` fit their content
/// with `PADDING_PX` of margin, scaled to at most `LONG_EDGE_PX` on the long edge; `view` uses
/// `camera` as given and rounds `canvas_size` (logical points) to a pixel size. Errors when
/// there is nothing to render: no elements at all, or nothing selected.
pub fn plan(
    file: &SceneFile,
    selection: &Selection,
    target: RenderTarget,
    camera: Camera,
    canvas_size: [f64; 2],
) -> Result<Plan, String> {
    match target {
        RenderTarget::View => Ok(Plan {
            scene: file.clone(),
            camera,
            size_px: [
                clamp_side(canvas_size[0].round()),
                clamp_side(canvas_size[1].round()),
            ],
        }),
        RenderTarget::All => GeometryCache::default()
            .common_bounds(file.elements.iter())
            .map(|bounds| from_bounds(bounds, file.clone()))
            .ok_or_else(|| "nothing to render: the scene has no elements".to_string()),
        RenderTarget::Selection => {
            let positions = selection_positions(file, selection);
            GeometryCache::default()
                .common_bounds(positions.iter().map(|&index| &file.elements[index]))
                .map(|bounds| {
                    let mut scene = file.clone();
                    scene.elements = positions
                        .iter()
                        .map(|&index| file.elements[index].clone())
                        .collect();
                    from_bounds(bounds, scene)
                })
                .ok_or_else(|| "nothing to render: nothing is selected".to_string())
        }
    }
}

/// Encodes tightly packed RGBA8 `rgba` (`size_px[0] * size_px[1] * 4` bytes) as PNG bytes.
pub fn encode_png(rgba: &[u8], size_px: [u32; 2]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size_px[0], size_px[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a PNG header always encodes");
    writer
        .write_image_data(rgba)
        .expect("RGBA8 data of the declared size always encodes");
    writer.finish().expect("a PNG trailer always encodes");
    bytes
}

#[cfg(test)]
mod tests {
    use scene::sample;
    use serde_json::json;

    use super::*;

    #[test]
    fn all_fits_the_content_with_padding() {
        let file = sample::file(vec![
            sample::generic("rectangle", "a", [0.0, 0.0, 100.0, 50.0]),
            sample::generic("rectangle", "b", [200.0, 100.0, 100.0, 50.0]),
        ]);
        let plan = plan(
            &file,
            &Selection::new(),
            RenderTarget::All,
            Camera::default(),
            [800.0, 600.0],
        )
        .unwrap();
        // Bounds include the stroke: element_bounds of a rectangle is its box.
        assert_eq!(plan.camera.zoom, 2.0);
        assert_eq!(plan.size_px, [632, 332]);
        assert_eq!(plan.camera.scene_to_view([0.0, 0.0]), [16.0, 16.0]);
    }

    #[test]
    fn selection_renders_only_selected_elements_and_their_labels() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "a", [0.0, 0.0, 100.0, 50.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::text("t", [20.0, 12.5, 60.0, 25.0], "hi", Some("a")),
            sample::generic("rectangle", "b", [5000.0, 0.0, 10.0, 10.0]),
        ]);
        let result = plan(
            &file,
            &Selection::from_ids(["a"]),
            RenderTarget::Selection,
            Camera::default(),
            [800.0, 600.0],
        )
        .unwrap();
        let ids: Vec<_> = result
            .scene
            .elements
            .iter()
            .filter_map(|e| e.id())
            .collect();
        assert_eq!(ids, vec!["a", "t"]);
        assert!(result.size_px[0] < 300);
        assert!(
            plan(
                &file,
                &Selection::new(),
                RenderTarget::Selection,
                Camera::default(),
                [800.0, 600.0]
            )
            .is_err()
        );
    }

    #[test]
    fn large_scenes_shrink_to_the_long_edge_and_view_uses_the_camera() {
        let file = sample::file(vec![sample::generic(
            "rectangle",
            "a",
            [0.0, 0.0, 10000.0, 100.0],
        )]);
        let all = plan(
            &file,
            &Selection::new(),
            RenderTarget::All,
            Camera::default(),
            [800.0, 600.0],
        )
        .unwrap();
        // 10000 * (1536 / 10000) may land a hair above 1536 and round up.
        assert!(
            (1536 + 32..=1537 + 32).contains(&all.size_px[0]),
            "{:?}",
            all.size_px
        );
        let camera = Camera {
            scroll_x: 3.0,
            scroll_y: 4.0,
            zoom: 1.5,
        };
        let view = plan(
            &file,
            &Selection::new(),
            RenderTarget::View,
            camera,
            [800.4, 600.0],
        )
        .unwrap();
        assert_eq!((view.camera, view.size_px), (camera, [800, 600]));
        assert!(
            plan(
                &sample::file(vec![]),
                &Selection::new(),
                RenderTarget::All,
                camera,
                [800.0, 600.0]
            )
            .is_err()
        );
    }

    #[test]
    fn png_round_trips() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let bytes = encode_png(&rgba, [2, 1]);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&buf[..8], &rgba[..]);
    }
}
