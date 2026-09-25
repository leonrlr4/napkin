mod support;

use app::camera::Camera;
use app::control::handler::{Session, handle};
use app::control::{RenderTarget, Request};
use app::render::offscreen::GpuRasterizer;
use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;

#[test]
fn render_writes_a_png_of_the_scene() {
    let (_gpu, device, queue) = support::gpu();
    let mut renderer = None;
    let mut rasterizer = GpuRasterizer {
        device: &device,
        queue: &queue,
        renderer: &mut renderer,
    };
    let fill = sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
        json!({"roughness": 0, "strokeColor": "#1971c2", "backgroundColor": "#1971c2", "fillStyle": "solid"}),
    );
    let mut editor = Editor::new(sample::file(vec![fill]), scene::env::SystemEnv);
    let mut measure = CharWidthMeasure;
    let out = std::env::temp_dir().join(format!("napkin-render-{}.png", std::process::id()));
    let mut session = Session {
        editor: &mut editor,
        path: None,
        unsaved: false,
        save_error: None,
        readonly: None,
        camera: Camera::default(),
        canvas_size: [800.0, 600.0],
        measure: &mut measure,
        dark: false,
        rasterizer: &mut rasterizer,
    };
    let response = handle(
        &mut session,
        &Request::Render {
            out: out.clone(),
            target: RenderTarget::All,
        },
    );
    assert!(response.ok, "{}", response.output);

    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (232, 132));
    let center = ((66 * info.width + 116) * 4) as usize;
    assert_eq!(&buf[center..center + 3], &[0x19, 0x71, 0xc2]);
    let corner = &buf[0..3];
    assert_eq!(corner, &[255, 255, 255], "view background");
    std::fs::remove_file(out).ok();
}
