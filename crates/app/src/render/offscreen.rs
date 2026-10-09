//! Offscreen GPU rendering: a fresh 4x MSAA texture cleared to the scene's view background,
//! painted through [`CanvasRenderer`], and read back as tightly packed RGBA8 rows. Backs both
//! the GPU test helpers under `tests/support` and [`GpuRasterizer`], the control protocol's
//! live `render` handler.

use crate::render::color::render_color;
use crate::render::gpu::{CanvasFrame, CanvasRenderer, STENCIL_FORMAT};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Draws `frame` into a fresh 4x MSAA texture cleared to the file's view background (dark mode
/// applied when `frame.dark`) and reads it back as tightly packed RGBA8 rows. `renderer` must
/// have been created with [`FORMAT`].
pub fn render_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut CanvasRenderer,
    frame: &CanvasFrame,
) -> Vec<u8> {
    // One pass draws everything, so every image must be decoded before `prepare` plans it.
    renderer.images().preload(&frame.file);
    let prepared = renderer.prepare(device, queue, frame);
    queue.submit(prepared);

    let [width, height] = frame.size_px;
    let background = render_color(frame.file.view_background_color(), frame.dark);
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
    let msaa = texture(
        "napkin offscreen msaa",
        4,
        FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let resolve = texture(
        "napkin offscreen resolve",
        1,
        FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let stencil = texture(
        "napkin offscreen stencil",
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
            label: Some("napkin offscreen render"),
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
        label: Some("napkin offscreen readback"),
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
    rgba
}

/// A [`Rasterize`](crate::control::render::Rasterize) over a live device; creates its renderer
/// on first use.
pub struct GpuRasterizer<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a mut Option<CanvasRenderer>,
}
