//! M0 spike (throwaway): inside a single egui_wgpu paint callback, is MSAA active, and
//! can glyphon text be drawn *between* two custom mesh draws (shape, text, shape)?
//! Findings are recorded in docs/decisions/; this file is deleted once they are.

use eframe::egui;
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};

const MSAA_SAMPLES: u32 = 4;
/// The first two quads (diagonal line, red box) are drawn before the text.
const VERTICES_UNDER_TEXT: u32 = 12;
/// The third quad (blue box) is drawn after the text and must hide part of it.
const VERTICES_TOTAL: u32 = 18;

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct CanvasResources {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    font_system: FontSystem,
    swash_cache: SwashCache,
    _glyph_cache: Cache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    label: Buffer,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin-spike")
            .with_inner_size([1000.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        multisampling: MSAA_SAMPLES as u16,
        ..Default::default()
    };
    eframe::run_native(
        "napkin spike: canvas",
        options,
        Box::new(|cc| {
            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .expect("eframe must run with the wgpu renderer");
            println!("adapter: {:?}", render_state.adapter.get_info());
            println!(
                "target format: {:?}, requested MSAA: {MSAA_SAMPLES}",
                render_state.target_format
            );
            let resources = create_resources(render_state);
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(resources);
            Ok(Box::new(CanvasProbe::default()))
        }),
    )
}

fn create_resources(render_state: &egui_wgpu::RenderState) -> CanvasResources {
    let device = &render_state.device;
    let multisample = wgpu::MultisampleState {
        count: MSAA_SAMPLES,
        ..Default::default()
    };

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spike shapes"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spike shapes"),
        ..Default::default()
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("spike shapes"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: render_state.target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample,
        multiview_mask: None,
        cache: None,
    });
    let vertices = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spike shapes"),
        size: u64::from(VERTICES_TOTAL) * size_of::<Vertex>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut font_system = FontSystem::new();
    let glyph_cache = Cache::new(device);
    let viewport = Viewport::new(device, &glyph_cache);
    let mut atlas = TextAtlas::new(
        device,
        &render_state.queue,
        &glyph_cache,
        render_state.target_format,
    );
    let text_renderer = TextRenderer::new(&mut atlas, device, multisample, None);
    let mut label = Buffer::new(&mut font_system, Metrics::new(64.0, 80.0));
    label.set_text(
        "Hello 手繪白板 napkin",
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );

    CanvasResources {
        pipeline,
        vertices,
        font_system,
        swash_cache: SwashCache::new(),
        _glyph_cache: glyph_cache,
        viewport,
        atlas,
        text_renderer,
        label,
    }
}

/// Where the probe writes its own screenshot. egui captures the frame on the GPU before
/// the compositor sees it, so Hyprland's window opacity and blur cannot skew the pixels.
const SCREENSHOT_PATH: &str = "spikes/m0/out/canvas.ppm";
/// Hyprland may resize the window right after mapping it; wait for the layout to settle.
const STABLE_FRAMES_BEFORE_SCREENSHOT: u32 = 30;

#[derive(Default)]
struct CanvasProbe {
    reported_rect_px: Option<[u32; 4]>,
    stable_frames: u32,
    screenshot_requested: bool,
}

impl eframe::App for CanvasProbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let screenshot = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = screenshot {
            write_ppm(SCREENSHOT_PATH, &image).expect("write screenshot");
            println!("screenshot saved to {SCREENSHOT_PATH}");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(
                "Expect: a smooth thin diagonal line, the text on top of the red box, \
                 and the blue box hiding the right half of the text.",
            );
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            let ppp = ctx.pixels_per_point();
            let rect_px =
                [rect.min.x, rect.min.y, rect.width(), rect.height()].map(|v| (v * ppp) as u32);
            if self.reported_rect_px == Some(rect_px) {
                self.stable_frames += 1;
            } else {
                let [x, y, w, h] = rect_px;
                println!("callback rect px {x} {y} {w} {h}");
                self.reported_rect_px = Some(rect_px);
                self.stable_frames = 0;
            }
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                CanvasCallback {
                    size_px: [rect.width() * ppp, rect.height() * ppp],
                },
            ));
        });

        if !self.screenshot_requested {
            if self.stable_frames >= STABLE_FRAMES_BEFORE_SCREENSHOT {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.screenshot_requested = true;
            }
            // egui only repaints on input; keep frames coming until the capture is taken.
            ctx.request_repaint();
        }
    }
}

fn write_ppm(path: &str, image: &egui::ColorImage) -> std::io::Result<()> {
    let [width, height] = image.size;
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in &image.pixels {
        bytes.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
    }
    std::fs::write(path, bytes)
}

struct CanvasCallback {
    size_px: [f32; 2],
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let res: &mut CanvasResources = callback_resources
            .get_mut()
            .expect("resources are inserted at startup");
        let [w, h] = self.size_px;

        queue.write_buffer(
            &res.vertices,
            0,
            bytemuck::cast_slice(&scene_vertices(w, h)),
        );

        res.viewport.update(
            queue,
            Resolution {
                width: w as u32,
                height: h as u32,
            },
        );
        res.label.set_size(Some(w), Some(h));
        res.label.shape_until_scroll(&mut res.font_system, false);
        res.text_renderer
            .prepare(
                device,
                queue,
                &mut res.font_system,
                &mut res.atlas,
                &res.viewport,
                [TextArea {
                    buffer: &res.label,
                    left: w * 0.10,
                    top: h * 0.44,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: w as i32,
                        bottom: h as i32,
                    },
                    default_color: Color::rgb(240, 240, 240),
                    custom_glyphs: &[],
                }],
                &mut res.swash_cache,
            )
            .expect("glyphon prepare");
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let res: &CanvasResources = callback_resources
            .get()
            .expect("resources are inserted at startup");

        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_vertex_buffer(0, res.vertices.slice(..));
        render_pass.draw(0..VERTICES_UNDER_TEXT, 0..1);

        res.text_renderer
            .render(&res.atlas, &res.viewport, render_pass)
            .expect("glyphon render");

        // glyphon leaves its own pipeline and bind groups bound; restore ours.
        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_vertex_buffer(0, res.vertices.slice(..));
        render_pass.draw(VERTICES_UNDER_TEXT..VERTICES_TOTAL, 0..1);
    }
}

/// Positions are laid out in physical pixels relative to the callback rect, then mapped
/// to NDC: egui sets the render pass viewport to that rect before calling `paint`.
fn scene_vertices(w: f32, h: f32) -> Vec<Vertex> {
    let ndc = |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
    let quad = |a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2], color: [f32; 4]| {
        [a, b, c, a, c, d].map(|p| Vertex {
            position: ndc(p[0], p[1]),
            color,
        })
    };

    // A 1.5px-wide nearly diagonal line: jagged without MSAA, smooth with it.
    let (x0, y0, x1, y1) = (w * 0.05, h * 0.92, w * 0.95, h * 0.08);
    let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    let (nx, ny) = (-(y1 - y0) / len * 0.75, (x1 - x0) / len * 0.75);
    let line = quad(
        [x0 + nx, y0 + ny],
        [x1 + nx, y1 + ny],
        [x1 - nx, y1 - ny],
        [x0 - nx, y0 - ny],
        [0.9, 0.9, 0.9, 1.0],
    );
    let rect = |l: f32, t: f32, r: f32, b: f32, color| quad([l, t], [r, t], [r, b], [l, b], color);
    let red_under = rect(w * 0.08, h * 0.40, w * 0.50, h * 0.62, [0.8, 0.2, 0.2, 1.0]);
    let blue_over = rect(
        w * 0.45,
        h * 0.38,
        w * 0.92,
        h * 0.64,
        [0.2, 0.35, 0.85, 1.0],
    );

    [line, red_under, blue_over].concat()
}
