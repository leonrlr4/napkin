use app::camera::Camera;
use app::render::gpu::{CanvasFrame, CanvasRenderer};
use app::render::offscreen;

pub use app::render::offscreen::FORMAT;

// `gpu_render.rs` compiles this module too but only calls `gpu()` directly, so every item
// below that only `gpu_shapes.rs`/`gpu_text.rs` reach through `render*` would otherwise warn
// as dead code there.
#[allow(dead_code)]
pub struct Image {
    pub width: u32,
    // No current test reads `height` directly (`pixel` and the full-buffer scans in
    // gpu_shapes.rs only need `width`), but it documents the image's shape alongside `rgba`.
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    #[allow(dead_code)]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().expect("four channels")
    }
}

/// Serializes GPU setup and teardown across every test in this binary. Both the Vulkan loader
/// (`vkEnumerateInstanceExtensionProperties`, entered while creating a `wgpu::Instance`) and
/// wgpu-hal's EGL context teardown crash when several test threads create or drop GPU instances
/// at once, so GPU tests run one at a time instead of in parallel.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires the GPU lock and returns a fresh instance, adapter and device. Bind the guard first
/// so it outlives the device and queue, e.g. `let (_gpu, device, queue) = support::gpu();`:
/// binding it to `_` drops it immediately and defeats the lock.
pub fn gpu() -> (
    std::sync::MutexGuard<'static, ()>,
    wgpu::Device,
    wgpu::Queue,
) {
    let guard = GPU
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU tests need a wgpu adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("wgpu device");
    device.on_uncaptured_error(std::sync::Arc::new(|error: wgpu::Error| {
        panic!("wgpu validation error: {error}")
    }));
    (guard, device, queue)
}

/// Renders `file` at pixels-per-point 1 over its own view background.
#[allow(dead_code)]
pub fn render(
    file: scene::SceneFile,
    camera: Camera,
    width: u32,
    height: u32,
    dark: bool,
) -> Image {
    render_with_ppp(file, camera, width, height, 1.0, dark)
}

/// Like [`render`], but at an explicit `pixels_per_point` (`width`/`height` are still the
/// physical-pixel size of the output image).
#[allow(dead_code)]
pub fn render_with_ppp(
    file: scene::SceneFile,
    camera: Camera,
    width: u32,
    height: u32,
    pixels_per_point: f32,
    dark: bool,
) -> Image {
    render_sequence_with_ppp(
        vec![(file, 0)],
        camera,
        width,
        height,
        pixels_per_point,
        dark,
    )
}

/// Runs the same `CanvasRenderer` through `prepare` for each `(SceneFile, generation)` in
/// order, at pixels-per-point 1, and reads back only the last one: exercises a generation
/// change (or the lack of one) exactly as a live renderer sees it across frames, instead of the
/// fresh renderer every other `render*` helper here starts from.
///
/// Only `gpu_shapes.rs` currently calls this directly (the other test binaries compile this
/// module too, so an unused `pub fn` here would warn in each of them).
#[allow(dead_code)]
pub fn render_sequence(
    files: Vec<(scene::SceneFile, u64)>,
    camera: Camera,
    width: u32,
    height: u32,
    dark: bool,
) -> Image {
    render_sequence_with_ppp(files, camera, width, height, 1.0, dark)
}

/// Like [`render_sequence`], but at an explicit `pixels_per_point`.
pub fn render_sequence_with_ppp(
    files: Vec<(scene::SceneFile, u64)>,
    camera: Camera,
    width: u32,
    height: u32,
    pixels_per_point: f32,
    dark: bool,
) -> Image {
    let (_gpu, device, queue) = gpu();
    let mut renderer = CanvasRenderer::new(&device, &queue, FORMAT);

    let mut frames = files.into_iter().map(|(file, generation)| CanvasFrame {
        file: std::sync::Arc::new(file),
        camera,
        size_px: [width, height],
        pixels_per_point,
        dark,
        generation,
    });
    let mut current = frames.next().expect("at least one file");
    for next in frames {
        let prepared = renderer.prepare(&device, &queue, &current);
        queue.submit(prepared);
        current = next;
    }

    let rgba = offscreen::render_rgba(&device, &queue, &mut renderer, &current);
    Image {
        width,
        height,
        rgba,
    }
}
