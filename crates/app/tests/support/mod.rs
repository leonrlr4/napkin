use app::camera::Camera;
use app::render::gpu::{CanvasFrame, CanvasRenderer, STENCIL_FORMAT};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Image {
    pub width: u32,
    // No current test reads `height` directly (`pixel` and the full-buffer scans in
    // gpu_shapes.rs only need `width`), but it documents the image's shape alongside `rgba`.
    #[allow(dead_code)]
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().expect("four channels")
    }
}

pub fn gpu() -> (wgpu::Device, wgpu::Queue) {
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
    (device, queue)
}

/// Renders `file` at pixels-per-point 1 over its own view background.
pub fn render(
    file: scene::SceneFile,
    camera: Camera,
    width: u32,
    height: u32,
    dark: bool,
) -> Image {
    let (device, queue) = gpu();
    let background = app::render::color::render_color(file.view_background_color(), dark);
    let mut renderer = CanvasRenderer::new(&device, &queue, FORMAT);
    let frame = CanvasFrame {
        file: std::sync::Arc::new(file),
        camera,
        size_px: [width, height],
        pixels_per_point: 1.0,
        dark,
    };
    let prepared = renderer.prepare(&device, &queue, &frame);
    queue.submit(prepared);

    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = |label, samples, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let msaa = texture("msaa", 4, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let resolve = texture(
        "resolve",
        1,
        FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let stencil = texture(
        "stencil",
        4,
        STENCIL_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let (msaa_view, resolve_view, stencil_view) = (
        msaa.create_view(&Default::default()),
        resolve.create_view(&Default::default()),
        stencil.create_view(&Default::default()),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let clear = wgpu::Color {
            r: f64::from(background[0]),
            g: f64::from(background[1]),
            b: f64::from(background[2]),
            a: f64::from(background[3]),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test canvas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view,
                depth_slice: None,
                resolve_target: Some(&resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &stencil_view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.paint(&mut pass);
    }
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = (width * 4).div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        resolve.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = readback
        .slice(..)
        .get_mapped_range()
        .expect("mapped readback");
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        rgba.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    Image {
        width,
        height,
        rgba,
    }
}
