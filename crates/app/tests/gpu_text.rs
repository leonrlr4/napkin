mod support;

use app::camera::Camera;
use app::sample;
use serde_json::json;

fn white_pixels(image: &support::Image, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y)[0] > 200)
        .count()
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
