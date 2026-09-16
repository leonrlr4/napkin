mod support;

use app::camera::Camera;
use app::render::gpu::{CanvasFrame, CanvasRenderer};
use app::sample;
use serde_json::json;

fn white_pixels(image: &support::Image, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y)[0] > 200)
        .count()
}

fn black_bg(w: f64, h: f64) -> serde_json::Value {
    sample::with(
        sample::generic("rectangle", "bg", [0.0, 0.0, w, h]),
        json!({ "roughness": 0, "strokeColor": "#000000", "backgroundColor": "#000000", "fillStyle": "solid" }),
    )
}

fn rotated(id: &str, rect: [f64; 4], text: &str) -> serde_json::Value {
    sample::with(
        sample::text(id, rect, text, None),
        json!({ "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6, "angle": 0.000001 }),
    )
}

/// Runs `CanvasRenderer::prepare` (and submits what it returns) without a paint pass, for tests
/// that only care whether `prepare` panics.
fn prepare_only(file: scene::SceneFile, camera: Camera, size_px: [u32; 2], pixels_per_point: f32) {
    let (device, queue) = support::gpu();
    let mut renderer = CanvasRenderer::new(&device, &queue, support::FORMAT);
    let frame = CanvasFrame {
        file: std::sync::Arc::new(file),
        camera,
        size_px,
        pixels_per_point,
        dark: false,
    };
    let prepared = renderer.prepare(&device, &queue, &frame);
    queue.submit(prepared);
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
}

#[test]
fn text_interleaves_with_shapes() {
    let solid = |id: &str, rect: [f64; 4], color: &str| {
        sample::with(
            sample::generic("rectangle", id, rect),
            json!({
                "roughness": 0, "strokeColor": color, "backgroundColor": color, "fillStyle": "solid"
            }),
        )
    };
    let under = solid("under", [10.0, 10.0, 180.0, 60.0], "#000000");
    let label = sample::with(
        sample::text("t", [20.0, 20.0, 160.0, 40.0], "MMMMMMMM", None),
        json!({ "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6 }),
    );
    let over = solid("over", [100.0, 5.0, 95.0, 70.0], "#1971c2");
    let image = support::render(
        sample::file(vec![under, label, over]),
        Camera::default(),
        200,
        80,
        false,
    );
    assert!(
        white_pixels(&image, 20, 95, 20, 60) > 50,
        "text not drawn over the first rectangle"
    );
    assert_eq!(
        white_pixels(&image, 105, 190, 10, 70),
        0,
        "text drawn over the later rectangle"
    );
}

#[test]
fn rotated_text_is_drawn_rotated() {
    let background = sample::with(
        sample::generic("rectangle", "bg", [0.0, 0.0, 200.0, 200.0]),
        json!({ "roughness": 0, "strokeColor": "#000000", "backgroundColor": "#000000", "fillStyle": "solid" }),
    );
    // A wide single-line text turned a quarter: its glyphs must end up in a tall band
    // around the center, not in the horizontal band it would occupy unrotated.
    let text = sample::with(
        sample::text("t", [20.0, 80.0, 160.0, 40.0], "MMMMMMMM", None),
        json!({ "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6, "angle": std::f64::consts::FRAC_PI_2 }),
    );
    let image = support::render(
        sample::file(vec![background, text]),
        Camera::default(),
        200,
        200,
        false,
    );
    assert!(
        white_pixels(&image, 80, 120, 20, 180) > 50,
        "rotated glyphs missing"
    );
    assert_eq!(
        white_pixels(&image, 20, 60, 80, 120),
        0,
        "glyphs left in the unrotated position"
    );
    assert_eq!(
        white_pixels(&image, 140, 180, 80, 120),
        0,
        "glyphs left in the unrotated position"
    );
}

#[test]
fn rotated_text_wider_than_the_device_texture_limit_still_renders() {
    // `width * scale` here comfortably exceeds any real GPU's `max_texture_dimension_2d`
    // (8192 on this machine's Arc B390, and WebGPU guarantees only that much); without the
    // raster-scale clamp in `ensure_rotated_texture`, creating the offscreen texture would fail
    // a wgpu validation error and `support::gpu`'s device would panic.
    let text = sample::with(
        sample::text("t", [0.0, 0.0, 100_000.0, 100.0], "wide", None),
        json!({ "strokeColor": "#ffffff", "fontSize": 32, "fontFamily": 6, "angle": 0.3 }),
    );
    support::render(sample::file(vec![text]), Camera::default(), 50, 50, false);
}

#[test]
fn text_color_is_not_darkened_by_srgb_conversion() {
    // glyphon's default `ColorMode::Accurate` linearizes vertex colors before writing them into
    // a target that (unlike a real sRGB framebuffer) never converts back, darkening every
    // non-white glyph; `#808080` over black makes that error easy to catch in the red channel.
    let background = sample::with(
        sample::generic("rectangle", "bg", [0.0, 0.0, 300.0, 150.0]),
        json!({
            "roughness": 0, "strokeColor": "#000000", "backgroundColor": "#000000", "fillStyle": "solid"
        }),
    );
    let text = sample::with(
        sample::text("t", [10.0, 10.0, 280.0, 90.0], "MMMM", None),
        json!({ "strokeColor": "#808080", "fontSize": 64, "fontFamily": 6 }),
    );
    let image = support::render(
        sample::file(vec![background, text]),
        Camera::default(),
        300,
        150,
        false,
    );
    let brightest_red = image.rgba.chunks(4).map(|p| p[0]).max().expect("pixels");
    assert!(
        (0x78..=0x88).contains(&brightest_red),
        "brightest red channel in text region: {brightest_red:#x}"
    );
}

#[test]
fn two_rotated_texts_in_one_frame_each_render_their_own_content() {
    // Each `ensure_rotated_texture` call used to leave its command buffer unsubmitted, so the
    // second rotated text's `prepare`/`render` overwrote the shared glyphon renderer and
    // viewport before the first text's draw ever reached the GPU: rendering "a" with "b" in
    // the same frame used to make "a"'s texture show "b"'s glyphs instead.
    let alone = support::render(
        sample::file(vec![
            black_bg(400.0, 200.0),
            rotated("a", [20.0, 20.0, 160.0, 40.0], "MMMMMMMM"),
        ]),
        Camera::default(),
        400,
        200,
        false,
    );
    let with_b = support::render(
        sample::file(vec![
            black_bg(400.0, 200.0),
            rotated("a", [20.0, 20.0, 160.0, 40.0], "MMMMMMMM"),
            rotated("b", [220.0, 120.0, 160.0, 40.0], "iiiiiiii"),
        ]),
        Camera::default(),
        400,
        200,
        false,
    );
    let region_a_alone = white_pixels(&alone, 20, 180, 20, 60);
    let region_a_with_b = white_pixels(&with_b, 20, 180, 20, 60);
    let region_b_with_a = white_pixels(&with_b, 220, 380, 120, 160);
    assert!(
        region_a_with_b > 500,
        "first rotated text lost its glyphs: {region_a_with_b}"
    );
    assert!(
        region_b_with_a > 0,
        "second rotated text was not drawn at all"
    );
    // "a" ("MMMMMMMM", wide capitals) lights roughly the same pixel count whether or not a
    // later rotated text shares the frame; before the fix it lit far fewer (it showed "b"'s
    // glyphs, or a mix, instead of its own).
    let diff = region_a_with_b.abs_diff(region_a_alone);
    assert!(
        diff <= region_a_alone / 10 + 5,
        "first text's content changed when a later rotated text shared the frame: alone \
         {region_a_alone}, with a following text {region_a_with_b}"
    );
}

#[test]
fn a_later_long_rotated_text_does_not_destroy_an_earlier_texture() {
    let a = sample::with(
        sample::text("a", [0.0, 0.0, 40.0, 20.0], "i", None),
        json!({ "strokeColor": "#ffffff", "fontSize": 8, "fontFamily": 6, "angle": 0.000001 }),
    );
    // More than glyphon's default 4096-byte vertex buffer can hold (~146 glyphs): before the
    // fix, shaping this line grew and replaced that shared buffer while "a"'s still-unsubmitted
    // command buffer referenced the old one, and submitting it later panicked wgpu with
    // "Buffer with 'glyphon vertices' label has been destroyed".
    let long: String = "M".repeat(220);
    let b = sample::with(
        sample::text("b", [0.0, 30.0, 2000.0, 20.0], &long, None),
        json!({ "strokeColor": "#ffffff", "fontSize": 8, "fontFamily": 6, "angle": 0.000001 }),
    );
    support::render(sample::file(vec![a, b]), Camera::default(), 100, 100, false);
}

#[test]
fn latin_text_at_extreme_zoom_hidpi_still_draws_glyphs() {
    // fontSize 36 * zoom 30 * pixels_per_point 2 = a 2160px physical em size: before the fix,
    // glyphon's prepare returned `AtlasFull` and the `.expect` on it panicked.
    let line = "The quick brown fox jumps over the lazy dog, 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let text = sample::with(
        sample::text("t", [0.0, 0.0, 1800.0, 45.0], line, None),
        json!({ "fontSize": 36, "strokeColor": "#ffffff" }),
    );
    let camera = Camera {
        scroll_x: 0.0,
        scroll_y: 0.0,
        zoom: 30.0,
    };
    let image = support::render_with_ppp(
        sample::file(vec![black_bg(2560.0, 1600.0), text]),
        camera,
        2560,
        1600,
        2.0,
        false,
    );
    let lit = white_pixels(&image, 0, 2560, 0, 1600);
    assert!(lit > 0, "no glyph pixels drawn at extreme zoom");
}

#[test]
fn cjk_text_at_extreme_zoom_hidpi_still_draws_glyphs() {
    // fontSize 20 * zoom 30 * pixels_per_point 2 = a 1200px physical em size: also past the
    // atlas's capacity before the fix.
    let line = "天地玄黃宇宙洪荒日月盈昃辰宿列張寒來暑往秋收冬藏閏餘成歲律呂調陽雲騰致雨露結為霜金生麗水玉出崑岡劍號巨闕珠稱夜光果珍李柰菜重芥薑海鹹河淡鱗潛羽翔龍師火帝鳥官人皇始制文字乃服衣裳推位讓國有虞陶唐";
    let text = sample::with(
        sample::text("t", [0.0, 0.0, 1800.0, 25.0], line, None),
        json!({ "fontSize": 20, "strokeColor": "#ffffff", "fontFamily": 8 }),
    );
    let camera = Camera {
        scroll_x: 0.0,
        scroll_y: 0.0,
        zoom: 30.0,
    };
    let image = support::render_with_ppp(
        sample::file(vec![black_bg(2560.0, 1600.0), text]),
        camera,
        2560,
        1600,
        2.0,
        false,
    );
    let lit = white_pixels(&image, 0, 2560, 0, 1600);
    assert!(lit > 0, "no glyph pixels drawn at extreme zoom");
}

#[test]
fn wide_rotated_text_past_the_texture_limit_does_not_overshoot_by_one_pixel() {
    // `width * raster_scale` rounds to just over the device's max_texture_dimension_2d for
    // this particular width: before the fix `create_texture` was asked for 8193px and wgpu's
    // validation error panicked (`support::gpu`'s device panics on any uncaptured error).
    let text = sample::with(
        sample::text(
            "t",
            [0.0, 0.0, 508.6256466330513, 25.0],
            "wide rotated",
            None,
        ),
        json!({ "angle": 0.3 }),
    );
    let camera = Camera {
        scroll_x: -250.0,
        scroll_y: -10.0,
        zoom: 30.0,
    };
    prepare_only(sample::file(vec![text]), camera, [1920, 1080], 1.0);
}
