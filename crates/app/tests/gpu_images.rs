mod support;

use app::camera::Camera;
use base64::Engine;
use scene::sample;
use serde_json::json;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

/// A 2x2 PNG: red and green on top, blue and white below.
fn quadrant_png_data_url() -> String {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, //
                0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
        writer.finish().unwrap();
    }
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// One 100x100 image element at the origin showing the quadrant PNG, with `overrides` merged
/// into the element.
fn scene_with_image(overrides: serde_json::Value) -> scene::SceneFile {
    let element = sample::with(
        sample::with(
            sample::generic("image", "img", [0.0, 0.0, 100.0, 100.0]),
            json!({ "fileId": "quadrants", "status": "saved", "scale": [1, 1], "crop": null }),
        ),
        overrides,
    );
    let mut file = sample::file(vec![element]);
    file.add_missing_file(scene::file::FileData {
        id: "quadrants".to_owned(),
        mime_type: "image/png".to_owned(),
        data_url: quadrant_png_data_url(),
        created: 1.0,
        last_retrieved: 0.0,
    });
    file
}

/// Samples one pixel inside each quadrant, off the exact center: at a 2x2 texture's quadrant
/// center the bilinear filter would blend in about 1% of the neighbouring texel.
fn quadrants(file: scene::SceneFile) -> [[u8; 4]; 4] {
    let image = support::render(file, Camera::default(), 100, 100, false);
    [
        image.pixel(24, 24),
        image.pixel(75, 24),
        image.pixel(24, 75),
        image.pixel(75, 75),
    ]
}

fn assert_quadrants(actual: [[u8; 4]; 4], expected: [[u8; 3]; 4], tolerance: u8) {
    for (pixel, want) in actual.iter().zip(expected) {
        assert!(
            (0..3).all(|i| pixel[i].abs_diff(want[i]) <= tolerance),
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn quadrants_draw_in_place() {
    assert_quadrants(
        quadrants(scene_with_image(json!({}))),
        [RED, GREEN, BLUE, WHITE],
        2,
    );
}

#[test]
fn angle_pi_turns_the_picture_around() {
    assert_quadrants(
        quadrants(scene_with_image(json!({ "angle": std::f64::consts::PI }))),
        [WHITE, BLUE, GREEN, RED],
        2,
    );
}

#[test]
fn negative_scale_mirrors_the_picture() {
    assert_quadrants(
        quadrants(scene_with_image(json!({ "scale": [-1, 1] }))),
        [GREEN, RED, WHITE, BLUE],
        2,
    );
    assert_quadrants(
        quadrants(scene_with_image(json!({ "scale": [1, -1] }))),
        [BLUE, WHITE, RED, GREEN],
        2,
    );
}

#[test]
fn crop_selects_part_of_the_picture() {
    let crop = json!({
        "x": 1, "y": 0, "width": 1, "height": 1, "naturalWidth": 2, "naturalHeight": 2
    });
    assert_quadrants(
        quadrants(scene_with_image(json!({ "crop": crop }))),
        [GREEN; 4],
        2,
    );
}

#[test]
fn opacity_blends_with_the_background() {
    let pixels = quadrants(scene_with_image(json!({ "opacity": 50 })));
    assert!(
        pixels[0][0].abs_diff(255) <= 3
            && pixels[0][1].abs_diff(128) <= 3
            && pixels[0][2].abs_diff(128) <= 3,
        "{:?}",
        pixels[0]
    );
}

#[test]
fn a_missing_file_draws_the_placeholder_box() {
    let mut file = scene_with_image(json!({ "fileId": "absent" }));
    file.elements.truncate(1);
    // The dashed placeholder outlines the box in gray and leaves the inside the canvas color.
    let image = support::render(file, Camera::default(), 100, 100, false);
    assert_eq!(&image.pixel(50, 50)[..3], &WHITE);
    let lit = image.rgba.chunks(4).filter(|p| p[0] < 250).count();
    assert!(lit > 40, "no placeholder outline was drawn");
}
