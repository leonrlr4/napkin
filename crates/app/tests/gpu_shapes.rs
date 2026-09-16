mod support;

use app::camera::Camera;
use app::render::gpu::{CanvasFrame, CanvasRenderer};
use app::sample;
use serde_json::json;

fn close(actual: [u8; 4], expected: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|i| actual[i].abs_diff(expected[i]) <= tolerance)
}

#[test]
fn solid_fill_and_background() {
    let rect = sample::with(
        sample::generic("rectangle", "r", [20.0, 20.0, 60.0, 40.0]),
        json!({ "roughness": 0, "backgroundColor": "#ffc9c9", "fillStyle": "solid" }),
    );
    let image = support::render(sample::file(vec![rect]), Camera::default(), 100, 80, false);
    assert!(
        close(image.pixel(50, 40), [0xff, 0xc9, 0xc9], 2),
        "{:?}",
        image.pixel(50, 40)
    );
    assert!(
        close(image.pixel(5, 5), [0xff, 0xff, 0xff], 0),
        "{:?}",
        image.pixel(5, 5)
    );
}

#[test]
fn translucent_double_stroke_never_darkens_twice() {
    // roughness 2 draws two offset strokes that cross; at opacity 50 a crossing pixel must
    // look like a single stroke over the background.
    let rect = sample::with(
        sample::generic("rectangle", "r", [20.0, 20.0, 160.0, 120.0]),
        json!({ "roughness": 2, "strokeWidth": 4, "opacity": 50 }),
    );
    let image = support::render(sample::file(vec![rect]), Camera::default(), 200, 160, false);
    // #1e1e1e at 50% over white is 0x8f; allow MSAA edge noise upwards only.
    let darkest = image.rgba.chunks(4).map(|p| p[0]).min().expect("pixels");
    assert!(darkest >= 0x8f - 2, "darkest channel {darkest:#x}");
    assert!(
        darkest <= 0x8f + 8,
        "stroke not drawn, darkest {darkest:#x}"
    );
}

#[test]
fn arrow_label_hole_shows_the_background() {
    let arrow = sample::with(
        sample::linear("arrow", "a", [10.0, 50.0], &[[0.0, 0.0], [180.0, 0.0]]),
        json!({ "roughness": 0, "strokeWidth": 4, "boundElements": [{ "id": "label", "type": "text" }] }),
    );
    let label = sample::text("label", [80.0, 38.0, 40.0, 25.0], "hi", Some("a"));
    let image = support::render(
        sample::file(vec![arrow, label]),
        Camera::default(),
        200,
        100,
        false,
    );
    // Inside the 5px padding left of the label box, on the arrow's line.
    assert!(
        close(image.pixel(77, 50), [0xff, 0xff, 0xff], 2),
        "{:?}",
        image.pixel(77, 50)
    );
    // Away from the label the arrow is drawn.
    assert!(
        close(image.pixel(40, 50), [0x1e, 0x1e, 0x1e], 8),
        "{:?}",
        image.pixel(40, 50)
    );
}

#[test]
fn camera_moves_content_and_culls() {
    let rect = sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 20.0, 20.0]),
        json!({ "roughness": 0, "backgroundColor": "#000000", "fillStyle": "solid" }),
    );
    let camera = Camera {
        scroll_x: 40.0,
        scroll_y: 30.0,
        zoom: 2.0,
    };
    let image = support::render(sample::file(vec![rect.clone()]), camera, 200, 200, false);
    // Scene (10, 10) lands at view ((10 + 40) * 2, (10 + 30) * 2).
    assert!(
        close(image.pixel(100, 80), [0, 0, 0], 2),
        "{:?}",
        image.pixel(100, 80)
    );
    let away = Camera {
        scroll_x: -5000.0,
        ..Camera::default()
    };
    let empty = support::render(sample::file(vec![rect]), away, 50, 50, false);
    assert!(empty.rgba.chunks(4).all(|p| p[0] == 0xff));
}

#[test]
fn duplicate_ids_render_their_own_fill_color() {
    let a = sample::with(
        sample::generic("rectangle", "dup", [0.0, 0.0, 100.0, 100.0]),
        json!({ "roughness": 0, "backgroundColor": "#e03131", "fillStyle": "solid" }),
    );
    let b = sample::with(
        sample::generic("rectangle", "dup", [100.0, 0.0, 100.0, 100.0]),
        json!({ "roughness": 0, "backgroundColor": "#1971c2", "fillStyle": "solid" }),
    );
    let image = support::render(sample::file(vec![a, b]), Camera::default(), 200, 100, false);
    assert!(
        close(image.pixel(50, 50), [0xe0, 0x31, 0x31], 2),
        "first duplicate-id rectangle: {:?}",
        image.pixel(50, 50)
    );
    assert!(
        close(image.pixel(150, 50), [0x19, 0x71, 0xc2], 2),
        "second duplicate-id rectangle: {:?}",
        image.pixel(150, 50)
    );
}

#[test]
fn sweeping_zoom_buckets_does_not_panic_and_caps_buffer_capacity() {
    // Regression for the mesh buffer's bump allocator never reclaiming evicted or off-bucket
    // segments: before the fix, capacity roughly doubled at every bucket change even when this
    // frame's own visible meshes would have fit in what was already allocated, eventually
    // asking wgpu for a buffer past `max_buffer_size` and panicking (`support::gpu`'s device
    // panics on any uncaptured validation error). The view stays small so the sweep is fast;
    // the bug is in how capacity *grows*, not in how much is visible at once.
    let (device, queue) = support::gpu();
    let mut renderer = CanvasRenderer::new(&device, &queue, support::FORMAT);
    let file = std::sync::Arc::new(app::fixture::generate(1, 1000));
    let max_buffer_size = device.limits().max_buffer_size;
    let vertex_size = std::mem::size_of::<app::render::tessellate::Vertex>() as u64;

    for bucket in -3..=6 {
        let zoom = 2f64.powi(bucket);
        let frame = CanvasFrame {
            file: file.clone(),
            camera: Camera {
                scroll_x: 2000.0,
                scroll_y: 1500.0,
                zoom,
            },
            size_px: [320, 240],
            pixels_per_point: 1.0,
            dark: false,
        };
        let prepared = renderer.prepare(&device, &queue, &frame);
        queue.submit(prepared);
        device.poll(wgpu::PollType::wait_indefinitely()).ok();

        let stats = renderer.stats();
        let capacity_bytes = u64::from(stats.buffer_vertices_capacity) * vertex_size;
        assert!(
            capacity_bytes <= max_buffer_size,
            "bucket {bucket}: vertex buffer capacity {capacity_bytes} bytes exceeds \
             max_buffer_size {max_buffer_size}"
        );
        assert!(stats.buffer_vertices_used <= stats.buffer_vertices_capacity);
    }
}

#[test]
fn every_corpus_file_renders_without_validation_errors() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scene/tests/corpus");
    for entry in std::fs::read_dir(dir).expect("corpus dir") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("read corpus");
        let load = || scene::SceneFile::from_json_str(&text).expect("corpus loads");
        let bounds = app::render::plan::content_bounds(&load()).expect("content");
        let camera = Camera::centered_on(bounds, [400.0, 300.0]);
        for dark in [false, true] {
            // Rendering must finish without a wgpu validation error (the device panics).
            support::render(load(), camera, 400, 300, dark);
        }
    }
}
