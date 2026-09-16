//! The wgpu canvas renderer: pipelines for the four stencil-based draw kinds `plan_frame`
//! produces, a shared bump-allocated vertex/index buffer for element meshes, a small per-frame
//! buffer for arrow-label stencil holes, and the uniform that carries the camera into clip
//! space. `CanvasRenderer::prepare` plans the frame and uploads whatever changed;
//! `CanvasRenderer::paint` only issues draw calls, so every decision (which meshes are visible,
//! where they live in the shared buffers) must already be resolved by the time `paint` runs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::camera::Camera;
use crate::render::buffers::{Segment, SegmentAllocator};
use crate::render::cache::SceneCache;
use crate::render::color::render_color;
use crate::render::plan::{DrawItem, ElementDraw, TextDraw, View, plan_frame};
use crate::render::tessellate::{Mesh, Vertex, local_center};
use crate::render::text;

pub const SAMPLE_COUNT: u32 = 4;
pub const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

const VERTEX_SIZE: u64 = std::mem::size_of::<Vertex>() as u64;
const INDEX_SIZE: u64 = std::mem::size_of::<u32>() as u64;

/// Shared element-mesh buffers start at 1M vertices / 3M indices (spec: enough headroom that a
/// typical scene never triggers a mid-session regrow).
const INITIAL_VERTEX_CAPACITY: u32 = 1_000_000;
const INITIAL_INDEX_CAPACITY: u32 = 3_000_000;

/// The per-frame hole buffer starts small: arrow labels are rare, and it regrows on demand.
const INITIAL_HOLE_QUADS: u32 = 64;

/// The per-frame rotated-text quad buffer starts small for the same reason as the hole buffer:
/// rotated text is rare, and it regrows on demand.
const INITIAL_ROTATED_QUADS: u32 = 16;

/// The offscreen texture a rotated text element is shaped into before `paint` draws it as a
/// rotated quad (Task 7); chosen independently of the canvas's own target format so a single
/// `glyphon::TextAtlas` (tied to one format) can serve every such texture regardless of what
/// format the canvas itself renders to.
const ROTATED_TEXT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Padding added on every side of a rotated-text texture, in physical pixels: room for glyph
/// antialiasing to bleed past the tight text bounds without being clipped.
const ROTATED_TEXT_PADDING_PX: u32 = 1;

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

const TEX_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

const SHADER: &str = r#"
struct Uniforms {
    scroll: vec2<f32>,
    zoom_px: f32,
    _pad0: f32,
    viewport_px: vec2<f32>,
    _pad1: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> VertexOutput {
    let clip = ((position + uniforms.scroll) * uniforms.zoom_px / uniforms.viewport_px)
        * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    var out: VertexOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.color = color;
    return out;
}

// StencilReset draws a fixed full-canvas quad directly in clip space, ignoring the camera.
@vertex
fn vs_reset(@builtin(vertex_index) index: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(corners[index], 0.0, 1.0);
    out.color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// The rotated-text quad: same camera transform as vs_main, carrying a UV instead of a color.
struct TexVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_textured(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>) -> TexVertexOutput {
    let clip = ((position + uniforms.scroll) * uniforms.zoom_px / uniforms.viewport_px)
        * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    var out: TexVertexOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(1) @binding(0) var rotated_text_texture: texture_2d<f32>;
@group(1) @binding(1) var rotated_text_sampler: sampler;

// The texture already holds premultiplied alpha (glyphon drew straight-alpha glyphs over a
// transparent background with ALPHA_BLENDING, and that combination premultiplies the result),
// so this is sampled as-is; the `textured` pipeline blends it with PREMULTIPLIED_ALPHA_BLENDING.
@fragment
fn fs_textured(in: TexVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(rotated_text_texture, rotated_text_sampler, in.uv);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scroll: [f32; 2],
    zoom_px: f32,
    _pad0: f32,
    viewport_px: [f32; 2],
    _pad1: [f32; 2],
}

/// One corner of a rotated-text quad: `position` in scene units, `uv` into that text's texture.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TexVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

const TEX_VERTEX_SIZE: u64 = std::mem::size_of::<TexVertex>() as u64;

#[derive(Clone)]
pub struct CanvasFrame {
    pub file: Arc<scene::SceneFile>,
    pub camera: Camera,
    /// Canvas size in physical pixels.
    pub size_px: [u32; 2],
    pub pixels_per_point: f32,
    pub dark: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStats {
    pub drawn_elements: usize,
    pub cached_meshes: usize,
    pub buffer_vertices: u32,
}

/// A `DrawItem` resolved to the GPU buffer segments and stencil state `paint` needs; built once
/// per frame in `prepare` so `paint` (which only has `&self`) has nothing left to decide.
enum PreparedItem {
    Meshes(Vec<(Arc<Mesh>, Segment)>),
    Isolated {
        mesh: Arc<Mesh>,
        segment: Segment,
        stencil: u8,
        /// The hole's location in the (per-frame) hole buffer, when this item has one.
        hole: Option<Segment>,
    },
    StencilReset,
    /// One batch of unrotated text; `renderer_index` selects which pooled `glyphon::TextRenderer`
    /// already has this batch's glyphs from `prepare`.
    Text {
        renderer_index: usize,
    },
    /// A rotated text element's offscreen texture, drawn as a quad in the (per-frame) rotated
    /// quad buffer.
    RotatedText {
        bind_group: Arc<wgpu::BindGroup>,
        quad: Segment,
    },
}

/// A rotated text element's cached offscreen render: `id` + `version_bits`, the render scale,
/// dark mode and alpha, matching `MeshKey`'s field set (a mesh's appearance depends on the same
/// five things; text's `scale` plays the role `MeshKey::bucket` plays for a mesh's tessellation
/// tolerance). Any entry not requested during a `prepare` call is evicted at the end of it (see
/// `CanvasRenderer::build_prepared`), so a changing `scale_bits` from continuous zooming does not
/// accumulate textures across frames.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RotatedTextKey {
    id: String,
    version_bits: u64,
    scale_bits: u32,
    dark: bool,
    alpha_bits: u32,
}

/// A rotated text element's rendered texture, kept alive for as long as its bind group is
/// referenced by a `PreparedItem` (the texture itself is never read back, only sampled).
struct RotatedTextEntry {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: Arc<wgpu::BindGroup>,
    width_px: u32,
    height_px: u32,
    /// The scale actually used to rasterize this texture, clamped below `scale_bits`' value
    /// when the requested scale would have exceeded `device.limits().max_texture_dimension_2d`;
    /// `rotated_quad_vertices` sizes the on-canvas quad against this, not the requested scale,
    /// so an extreme zoom only softens the text instead of shrinking it.
    raster_scale: f32,
}

struct PipelineSpec<'a> {
    label: &'a str,
    vs_entry: &'a str,
    write_mask: wgpu::ColorWrites,
    face: wgpu::StencilFaceState,
    with_vertex_buffer: bool,
}

fn stencil_face(
    compare: wgpu::CompareFunction,
    pass_op: wgpu::StencilOperation,
) -> wgpu::StencilFaceState {
    wgpu::StencilFaceState {
        compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op,
    }
}

fn depth_stencil_state(face: wgpu::StencilFaceState) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The depth/stencil state `plain` (and every in-pass `glyphon::TextRenderer`) uses: the
/// stencil buffer is neither tested nor written, only along for the ride because the pass has a
/// stencil attachment.
fn plain_depth_stencil() -> wgpu::DepthStencilState {
    depth_stencil_state(stencil_face(
        wgpu::CompareFunction::Always,
        wgpu::StencilOperation::Keep,
    ))
}

fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    spec: PipelineSpec,
) -> wgpu::RenderPipeline {
    let vertex_layout = Some(wgpu::VertexBufferLayout {
        array_stride: VERTEX_SIZE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    });
    let buffers: &[Option<wgpu::VertexBufferLayout>] = if spec.with_vertex_buffer {
        std::slice::from_ref(&vertex_layout)
    } else {
        &[]
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(spec.vs_entry),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: spec.write_mask,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            // lyon's triangle winding is not guaranteed, so nothing is culled.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_stencil_state(spec.face)),
        multisample: wgpu::MultisampleState {
            count: SAMPLE_COUNT,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// The rotated-text quad pipeline: samples a premultiplied-alpha texture instead of taking a
/// vertex color, and blends accordingly (spec §6.3's rotated-text step).
fn make_textured_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("napkin canvas textured"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_textured"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: TEX_VERTEX_SIZE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &TEX_VERTEX_ATTRIBUTES,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_textured"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(plain_depth_stencil()),
        multisample: wgpu::MultisampleState {
            count: SAMPLE_COUNT,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// Every `ElementDraw` a frame's plan references, in the order `paint` will need them (`Text`
/// and `RotatedText` carry no mesh and are skipped here; `build_prepared` handles them through
/// `glyphon` instead of the shared mesh buffers).
fn element_draws(items: &[DrawItem]) -> Vec<&ElementDraw> {
    let mut draws = Vec::new();
    for item in items {
        match item {
            DrawItem::Meshes(list) => draws.extend(list.iter()),
            DrawItem::Isolated { draw, .. } => draws.push(draw),
            DrawItem::StencilReset | DrawItem::Text(_) | DrawItem::RotatedText(_) => {}
        }
    }
    draws
}

pub struct CanvasRenderer {
    plain: wgpu::RenderPipeline,
    isolated: wgpu::RenderPipeline,
    mask: wgpu::RenderPipeline,
    reset: wgpu::RenderPipeline,
    textured: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,

    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    vertex_capacity: u32,
    index_capacity: u32,
    allocator: SegmentAllocator,

    /// Rebuilt from scratch every frame (arrow labels are rare, and the geometry depends on
    /// the current camera, so nothing here is cached across frames).
    hole_vertex_buffer: wgpu::Buffer,
    hole_index_buffer: wgpu::Buffer,
    hole_vertex_capacity: u32,
    hole_index_capacity: u32,

    /// Rebuilt from scratch every frame, one quad per `RotatedText` item, for the same reason
    /// as the hole buffer.
    rotated_quad_vertex_buffer: wgpu::Buffer,
    rotated_quad_index_buffer: wgpu::Buffer,
    rotated_quad_vertex_capacity: u32,
    rotated_quad_index_capacity: u32,

    cache: SceneCache,

    text_font_system: glyphon::FontSystem,
    text_swash_cache: glyphon::SwashCache,
    /// In-pass text: shares the canvas's own target format, sample count and stencil state so
    /// it draws directly into the same MSAA + stencil pass as everything else.
    text_atlas: glyphon::TextAtlas,
    text_viewport: glyphon::Viewport,
    /// One `glyphon::TextRenderer` per `Text` batch position in the current (or a past, larger)
    /// frame; `prepare` grows this pool but never shrinks it.
    text_renderers: Vec<glyphon::TextRenderer>,
    /// Shaped lines, keyed by the source element's `id` + `version_bits`: independent of
    /// rotation, so a `RotatedText` element's lines are shaped once and reused for every zoom
    /// level's offscreen texture.
    text_lines: HashMap<(String, u64), Vec<text::ShapedLine>>,

    /// Rotated text: its own atlas (a fixed offscreen format, sample count 1, no stencil) and
    /// a single renderer reused sequentially, since each rotated element's texture is rendered
    /// and submitted to completion before the next one starts.
    rotated_atlas: glyphon::TextAtlas,
    rotated_viewport: glyphon::Viewport,
    rotated_renderer: glyphon::TextRenderer,
    rotated_texture_bind_group_layout: wgpu::BindGroupLayout,
    rotated_texture_sampler: wgpu::Sampler,
    rotated_text_cache: HashMap<RotatedTextKey, RotatedTextEntry>,

    prepared: Vec<PreparedItem>,
    stats: RenderStats,
}

impl CanvasRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> CanvasRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("napkin canvas"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("napkin canvas uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("napkin canvas"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let plain = make_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            PipelineSpec {
                label: "napkin canvas plain",
                vs_entry: "vs_main",
                write_mask: wgpu::ColorWrites::ALL,
                face: stencil_face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Keep),
                with_vertex_buffer: true,
            },
        );
        let isolated = make_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            PipelineSpec {
                label: "napkin canvas isolated",
                vs_entry: "vs_main",
                write_mask: wgpu::ColorWrites::ALL,
                face: stencil_face(
                    wgpu::CompareFunction::NotEqual,
                    wgpu::StencilOperation::Replace,
                ),
                with_vertex_buffer: true,
            },
        );
        let mask = make_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            PipelineSpec {
                label: "napkin canvas mask",
                vs_entry: "vs_main",
                write_mask: wgpu::ColorWrites::empty(),
                face: stencil_face(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                ),
                with_vertex_buffer: true,
            },
        );
        let reset = make_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            PipelineSpec {
                label: "napkin canvas reset",
                vs_entry: "vs_reset",
                write_mask: wgpu::ColorWrites::empty(),
                face: stencil_face(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                ),
                with_vertex_buffer: false,
            },
        );

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("napkin canvas uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("napkin canvas uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let vertex_buffer = create_buffer(
            device,
            "napkin canvas vertices",
            u64::from(INITIAL_VERTEX_CAPACITY) * VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let index_buffer = create_buffer(
            device,
            "napkin canvas indices",
            u64::from(INITIAL_INDEX_CAPACITY) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        let hole_vertex_capacity = INITIAL_HOLE_QUADS * 4;
        let hole_index_capacity = INITIAL_HOLE_QUADS * 6;
        let hole_vertex_buffer = create_buffer(
            device,
            "napkin canvas hole vertices",
            u64::from(hole_vertex_capacity) * VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let hole_index_buffer = create_buffer(
            device,
            "napkin canvas hole indices",
            u64::from(hole_index_capacity) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );

        let rotated_quad_vertex_capacity = INITIAL_ROTATED_QUADS * 4;
        let rotated_quad_index_capacity = INITIAL_ROTATED_QUADS * 6;
        let rotated_quad_vertex_buffer = create_buffer(
            device,
            "napkin canvas rotated text vertices",
            u64::from(rotated_quad_vertex_capacity) * TEX_VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let rotated_quad_index_buffer = create_buffer(
            device,
            "napkin canvas rotated text indices",
            u64::from(rotated_quad_index_capacity) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );

        let rotated_texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("napkin canvas rotated text texture"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let textured_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("napkin canvas textured"),
                bind_group_layouts: &[
                    Some(&bind_group_layout),
                    Some(&rotated_texture_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let textured =
            make_textured_pipeline(device, &textured_pipeline_layout, &shader, target_format);
        let rotated_texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("napkin canvas rotated text sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // `ColorMode::Web`: `Accurate` (the default) treats the glyph atlas as sRGB and linearizes
        // vertex colors in the shader, which is correct for a target that itself gets an sRGB ->
        // linear conversion on write. Our targets (the canvas's own `target_format`, and the
        // rotated-text offscreen `Rgba8Unorm`) don't get that conversion -- they're blended in
        // gamma space like a browser canvas, same as every other pipeline in this file (`plain`,
        // `isolated`, ...) -- so `Accurate` would silently darken every non-white glyph.
        let text_gpu_cache = glyphon::Cache::new(device);
        let text_atlas = glyphon::TextAtlas::with_color_mode(
            device,
            queue,
            &text_gpu_cache,
            target_format,
            glyphon::ColorMode::Web,
        );
        let text_viewport = glyphon::Viewport::new(device, &text_gpu_cache);
        let mut rotated_atlas = glyphon::TextAtlas::with_color_mode(
            device,
            queue,
            &text_gpu_cache,
            ROTATED_TEXT_FORMAT,
            glyphon::ColorMode::Web,
        );
        let rotated_viewport = glyphon::Viewport::new(device, &text_gpu_cache);
        let rotated_renderer = glyphon::TextRenderer::new(
            &mut rotated_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        CanvasRenderer {
            plain,
            isolated,
            mask,
            reset,
            textured,
            uniform_buffer,
            bind_group,
            vertex_buffer,
            index_buffer,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            index_capacity: INITIAL_INDEX_CAPACITY,
            allocator: SegmentAllocator::new(INITIAL_VERTEX_CAPACITY, INITIAL_INDEX_CAPACITY),
            hole_vertex_buffer,
            hole_index_buffer,
            hole_vertex_capacity,
            hole_index_capacity,
            rotated_quad_vertex_buffer,
            rotated_quad_index_buffer,
            rotated_quad_vertex_capacity,
            rotated_quad_index_capacity,
            cache: SceneCache::new(),
            text_font_system: text::font_system(),
            text_swash_cache: glyphon::SwashCache::new(),
            text_atlas,
            text_viewport,
            text_renderers: Vec::new(),
            text_lines: HashMap::new(),
            rotated_atlas,
            rotated_viewport,
            rotated_renderer,
            rotated_texture_bind_group_layout,
            rotated_texture_sampler,
            rotated_text_cache: HashMap::new(),
            prepared: Vec::new(),
            stats: RenderStats::default(),
        }
    }

    /// Clears caches; call when another file is shown.
    pub fn reset(&mut self) {
        self.cache.clear();
        self.allocator
            .reset(self.vertex_capacity, self.index_capacity);
        self.text_lines.clear();
        self.rotated_text_cache.clear();
        self.prepared.clear();
        self.stats = RenderStats::default();
    }

    /// Plans the frame and uploads what it needs. The returned buffers (rotated text's offscreen
    /// renders) must be submitted before the pass that calls `paint`.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &CanvasFrame,
    ) -> Vec<wgpu::CommandBuffer> {
        let view_size = [
            f64::from(frame.size_px[0]) / f64::from(frame.pixels_per_point),
            f64::from(frame.size_px[1]) / f64::from(frame.pixels_per_point),
        ];
        let view = View {
            visible: frame.camera.visible_rect(view_size),
            dark: frame.dark,
            bucket: frame.camera.bucket(f64::from(frame.pixels_per_point)),
        };
        let items = plan_frame(&frame.file, &mut self.cache, &view);
        let background = frame.file.view_background_color();
        let draws = element_draws(&items);

        self.ensure_mesh_segments(device, queue, &frame.file, background, &draws);
        self.text_viewport.update(
            queue,
            glyphon::Resolution {
                width: frame.size_px[0],
                height: frame.size_px[1],
            },
        );
        let (prepared, drawn_elements, rotated_text_commands) =
            self.build_prepared(device, queue, frame, background, &items);
        self.text_atlas.trim();

        let uniforms = Uniforms {
            scroll: [frame.camera.scroll_x as f32, frame.camera.scroll_y as f32],
            zoom_px: frame.camera.zoom as f32 * frame.pixels_per_point,
            _pad0: 0.0,
            viewport_px: [frame.size_px[0] as f32, frame.size_px[1] as f32],
            _pad1: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        self.prepared = prepared;
        self.stats = RenderStats {
            drawn_elements,
            cached_meshes: self.cache.mesh_count(),
            buffer_vertices: self.allocator.used().0,
        };
        self.cache.evict(600);

        rotated_text_commands
    }

    /// Draws the prepared frame into a pass with a 4x MSAA color target and a Stencil8
    /// attachment whose viewport is the canvas.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        for item in &self.prepared {
            match item {
                PreparedItem::Meshes(meshes) => {
                    pass.set_pipeline(&self.plain);
                    for (mesh, segment) in meshes {
                        // Bottom to top: later (higher) parts blend over earlier ones.
                        for part in &mesh.parts {
                            pass.draw_indexed(
                                segment.index_start + part.start..segment.index_start + part.end,
                                segment.vertex_start as i32,
                                0..1,
                            );
                        }
                    }
                }
                PreparedItem::Isolated {
                    mesh,
                    segment,
                    stencil,
                    hole,
                } => {
                    if let Some(hole_segment) = hole {
                        pass.set_pipeline(&self.mask);
                        pass.set_stencil_reference(u32::from(*stencil));
                        pass.set_vertex_buffer(0, self.hole_vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            self.hole_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(
                            hole_segment.index_start
                                ..hole_segment.index_start + hole_segment.index_count,
                            hole_segment.vertex_start as i32,
                            0..1,
                        );
                        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            self.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                    }
                    pass.set_pipeline(&self.isolated);
                    pass.set_stencil_reference(u32::from(*stencil));
                    // Top to bottom: whichever part should visually win claims each pixel's
                    // stencil slot first, blocking the parts drawn under it there.
                    for part in mesh.parts.iter().rev() {
                        pass.draw_indexed(
                            segment.index_start + part.start..segment.index_start + part.end,
                            segment.vertex_start as i32,
                            0..1,
                        );
                    }
                }
                PreparedItem::StencilReset => {
                    pass.set_pipeline(&self.reset);
                    pass.set_stencil_reference(0);
                    pass.draw(0..6, 0..1);
                }
                PreparedItem::Text { renderer_index } => {
                    self.text_renderers[*renderer_index]
                        .render(&self.text_atlas, &self.text_viewport, pass)
                        .expect("glyphon render for in-pass text");
                    // glyphon's render() leaves its own pipeline, bind group 0 and vertex
                    // buffer bound; restore ours so the next item (which, unless it is
                    // `Isolated` with a hole, does not rebind these itself) finds them intact.
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                    pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                }
                PreparedItem::RotatedText { bind_group, quad } => {
                    pass.set_pipeline(&self.textured);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_bind_group(1, bind_group.as_ref(), &[]);
                    pass.set_vertex_buffer(0, self.rotated_quad_vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        self.rotated_quad_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(
                        quad.index_start..quad.index_start + quad.index_count,
                        quad.vertex_start as i32,
                        0..1,
                    );
                    pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                    pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                }
            }
        }
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Allocates a shared-buffer segment for every draw in `draws` that does not already have
    /// one, growing (and re-uploading everything visible this frame into) the shared buffers
    /// first if the current capacity cannot fit them.
    fn ensure_mesh_segments(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        file: &scene::SceneFile,
        background: &str,
        draws: &[&ElementDraw],
    ) {
        let mut need_vertices = 0u32;
        let mut need_indices = 0u32;
        for draw in draws {
            let cached = self
                .cache
                .mesh(&file.elements[draw.element], &draw.key, background);
            if cached.segment.is_none() {
                need_vertices += cached.mesh.vertices.len() as u32;
                need_indices += cached.mesh.indices.len() as u32;
            }
        }

        let (used_vertices, used_indices) = self.allocator.used();
        let fits = used_vertices
            .checked_add(need_vertices)
            .is_some_and(|v| v <= self.vertex_capacity)
            && used_indices
                .checked_add(need_indices)
                .is_some_and(|i| i <= self.index_capacity);
        if !fits {
            // The buffers are recreated, so every mesh currently visible needs a fresh segment,
            // not just the ones that were missing one before.
            let mut total_vertices = 0u32;
            let mut total_indices = 0u32;
            for draw in draws {
                let cached = self
                    .cache
                    .mesh(&file.elements[draw.element], &draw.key, background);
                total_vertices += cached.mesh.vertices.len() as u32;
                total_indices += cached.mesh.indices.len() as u32;
            }
            let new_vertex_capacity = (self.vertex_capacity * 2).max(total_vertices);
            let new_index_capacity = (self.index_capacity * 2).max(total_indices);
            self.grow_mesh_buffers(device, new_vertex_capacity, new_index_capacity);
            self.cache.forget_segments();
        }

        for draw in draws {
            let cached = self
                .cache
                .mesh(&file.elements[draw.element], &draw.key, background);
            if cached.segment.is_some() {
                continue;
            }
            let vertex_count = cached.mesh.vertices.len() as u32;
            let index_count = cached.mesh.indices.len() as u32;
            let segment = self
                .allocator
                .allocate(vertex_count, index_count)
                .expect("buffers were sized to fit every mesh visible this frame");
            queue.write_buffer(
                &self.vertex_buffer,
                u64::from(segment.vertex_start) * VERTEX_SIZE,
                bytemuck::cast_slice(&cached.mesh.vertices),
            );
            queue.write_buffer(
                &self.index_buffer,
                u64::from(segment.index_start) * INDEX_SIZE,
                bytemuck::cast_slice(&cached.mesh.indices),
            );
            cached.segment = Some(segment);
        }
    }

    fn grow_mesh_buffers(
        &mut self,
        device: &wgpu::Device,
        vertex_capacity: u32,
        index_capacity: u32,
    ) {
        self.vertex_buffer = create_buffer(
            device,
            "napkin canvas vertices",
            u64::from(vertex_capacity) * VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        self.index_buffer = create_buffer(
            device,
            "napkin canvas indices",
            u64::from(index_capacity) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        self.vertex_capacity = vertex_capacity;
        self.index_capacity = index_capacity;
        self.allocator.reset(vertex_capacity, index_capacity);
    }

    /// Grows the hole buffers, if needed, to fit `quad_count` quads.
    fn ensure_hole_capacity(&mut self, device: &wgpu::Device, quad_count: u32) {
        let need_vertices = quad_count * 4;
        let need_indices = quad_count * 6;
        if need_vertices <= self.hole_vertex_capacity && need_indices <= self.hole_index_capacity {
            return;
        }
        let vertex_capacity = (self.hole_vertex_capacity * 2).max(need_vertices);
        let index_capacity = (self.hole_index_capacity * 2).max(need_indices);
        self.hole_vertex_buffer = create_buffer(
            device,
            "napkin canvas hole vertices",
            u64::from(vertex_capacity) * VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        self.hole_index_buffer = create_buffer(
            device,
            "napkin canvas hole indices",
            u64::from(index_capacity) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        self.hole_vertex_capacity = vertex_capacity;
        self.hole_index_capacity = index_capacity;
    }

    /// Grows the rotated-text quad buffers, if needed, to fit `quad_count` quads.
    fn ensure_rotated_quad_capacity(&mut self, device: &wgpu::Device, quad_count: u32) {
        let need_vertices = quad_count * 4;
        let need_indices = quad_count * 6;
        if need_vertices <= self.rotated_quad_vertex_capacity
            && need_indices <= self.rotated_quad_index_capacity
        {
            return;
        }
        let vertex_capacity = (self.rotated_quad_vertex_capacity * 2).max(need_vertices);
        let index_capacity = (self.rotated_quad_index_capacity * 2).max(need_indices);
        self.rotated_quad_vertex_buffer = create_buffer(
            device,
            "napkin canvas rotated text vertices",
            u64::from(vertex_capacity) * TEX_VERTEX_SIZE,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        self.rotated_quad_index_buffer = create_buffer(
            device,
            "napkin canvas rotated text indices",
            u64::from(index_capacity) * INDEX_SIZE,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        self.rotated_quad_vertex_capacity = vertex_capacity;
        self.rotated_quad_index_capacity = index_capacity;
    }

    /// Shapes and caches `file.elements[index]`'s lines, unless a cache entry from an earlier
    /// frame already covers this `id` + `version`. `label` draws `placeholder_label` in
    /// napkin-sans 12 instead of the element's own text (used for a placeholder's type label,
    /// which is not itself a `TextElement`).
    fn ensure_shaped(&mut self, file: &scene::SceneFile, index: usize, label: bool) {
        let element = &file.elements[index];
        let key = (
            element.id().unwrap_or_default().to_owned(),
            element.version().to_bits(),
        );
        if self.text_lines.contains_key(&key) {
            return;
        }
        let shaped = if label {
            let lines = [text::placeholder_label(element.kind())];
            text::shape_lines(
                &mut self.text_font_system,
                &lines,
                "napkin-sans",
                12.0,
                12.0,
                f32::MAX,
                Some(glyphon::cosmic_text::Align::Left),
            )
        } else {
            let scene::Element::Text(text_element) = element else {
                debug_assert!(
                    false,
                    "plan_frame only emits label=false for a Text element"
                );
                return;
            };
            let lines = text::layout_lines(text_element);
            let family = text::bundled_family(text_element.font_family);
            let line_height = text_element.line_height.unwrap_or(1.25);
            let line_height_px = (text_element.font_size * line_height) as f32;
            let align = Some(match text_element.text_align.as_str() {
                "center" => glyphon::cosmic_text::Align::Center,
                "right" => glyphon::cosmic_text::Align::Right,
                _ => glyphon::cosmic_text::Align::Left,
            });
            text::shape_lines(
                &mut self.text_font_system,
                &lines,
                family,
                text_element.font_size as f32,
                line_height_px,
                text_element.base.width as f32,
                align,
            )
        };
        self.text_lines.insert(key, shaped);
    }

    /// Grows the in-pass `TextRenderer` pool, if needed, so index `index` exists.
    fn ensure_text_renderer(&mut self, device: &wgpu::Device, index: usize) {
        while self.text_renderers.len() <= index {
            let renderer = glyphon::TextRenderer::new(
                &mut self.text_atlas,
                device,
                wgpu::MultisampleState {
                    count: SAMPLE_COUNT,
                    ..Default::default()
                },
                Some(plain_depth_stencil()),
            );
            self.text_renderers.push(renderer);
        }
    }

    /// Renders `draw`'s element into its cached offscreen texture at `scale`, unless a cache
    /// entry for this `id` + `version` + `scale` + `dark` + `draw.alpha` already exists. Returns
    /// the command buffer the render was submitted through, when this was a cache miss (`None`
    /// on a hit means there is nothing new to submit).
    fn ensure_rotated_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        file: &scene::SceneFile,
        draw: &TextDraw,
        scale: f32,
        dark: bool,
    ) -> Option<wgpu::CommandBuffer> {
        let element = &file.elements[draw.element];
        let key = rotated_text_key(file, draw, scale, dark);
        if self.rotated_text_cache.contains_key(&key) {
            return None;
        }
        self.ensure_shaped(file, draw.element, false);
        let placement = element
            .placement()
            .expect("RotatedText only wraps a Text element, which always has a placement");

        // A device has a maximum 2D texture dimension; a large text at extreme zoom could ask
        // for more than that. Lower the raster scale (never raise it) just enough that both
        // dimensions fit, so the quad -- sized from `width_px` / `raster_scale` below, not from
        // the requested `scale` -- keeps its correct scene-space size and only loses sharpness.
        let max_dimension = device.limits().max_texture_dimension_2d;
        let padding_px = 2 * ROTATED_TEXT_PADDING_PX;
        let max_raster_scale_for = |extent: f64| {
            if extent > 0.0 {
                (max_dimension.saturating_sub(padding_px).max(1)) as f32 / extent as f32
            } else {
                scale
            }
        };
        let raster_scale = scale
            .min(max_raster_scale_for(placement.width))
            .min(max_raster_scale_for(placement.height));

        let width_px = ((placement.width as f32 * raster_scale).ceil() as u32 + padding_px).max(1);
        let height_px =
            ((placement.height as f32 * raster_scale).ceil() as u32 + padding_px).max(1);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("napkin canvas rotated text"),
            size: wgpu::Extent3d {
                width: width_px,
                height: height_px,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ROTATED_TEXT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.rotated_viewport.update(
            queue,
            glyphon::Resolution {
                width: width_px,
                height: height_px,
            },
        );
        let bounds = glyphon::TextBounds {
            left: 0,
            top: 0,
            right: width_px as i32,
            bottom: height_px as i32,
        };
        let color = text_draw_color(file, draw, dark);
        let origin = ROTATED_TEXT_PADDING_PX as f32;
        let shaped = shaped_lines_for(&self.text_lines, element);
        let areas: Vec<glyphon::TextArea> = shaped
            .iter()
            .map(|line| glyphon::TextArea {
                buffer: &line.buffer,
                left: origin,
                top: text::text_area_top(
                    origin,
                    line.baseline,
                    line.baseline_in_buffer,
                    raster_scale,
                ),
                scale: raster_scale,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            })
            .collect();
        self.rotated_renderer
            .prepare(
                device,
                queue,
                &mut self.text_font_system,
                &mut self.rotated_atlas,
                &self.rotated_viewport,
                areas,
                &mut self.text_swash_cache,
            )
            .expect("glyphon prepare for rotated text");

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("napkin canvas rotated text"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("napkin canvas rotated text"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rotated_renderer
                .render(&self.rotated_atlas, &self.rotated_viewport, &mut pass)
                .expect("glyphon render for rotated text");
        }

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("napkin canvas rotated text"),
            layout: &self.rotated_texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.rotated_texture_sampler),
                },
            ],
        });
        self.rotated_text_cache.insert(
            key,
            RotatedTextEntry {
                texture,
                bind_group: Arc::new(bind_group),
                width_px,
                height_px,
                raster_scale,
            },
        );
        Some(encoder.finish())
    }

    /// Resolves `items` into `PreparedItem`s, uploading this frame's stencil-hole quads and
    /// rotated-text quads (every element mesh must already have a segment, via
    /// `ensure_mesh_segments`) and shaping/rendering whatever text those items need. Returns the
    /// prepared list, the number of elements drawn, and the command buffers rotated text's
    /// offscreen renders were submitted through (the caller must submit these before `paint`).
    fn build_prepared(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &CanvasFrame,
        background: &str,
        items: &[DrawItem],
    ) -> (Vec<PreparedItem>, usize, Vec<wgpu::CommandBuffer>) {
        let file = &frame.file;
        // Matches `zoom_px` in the vertex shader's `Uniforms` exactly, so text and geometry
        // agree on where a scene point lands in physical pixels.
        let scale = frame.camera.zoom as f32 * frame.pixels_per_point;
        let scroll = [frame.camera.scroll_x as f32, frame.camera.scroll_y as f32];

        let holes: Vec<[[f64; 2]; 4]> = items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Isolated {
                    hole: Some(hole), ..
                } => Some(*hole),
                _ => None,
            })
            .collect();
        self.ensure_hole_capacity(device, holes.len() as u32);
        for (index, corners) in holes.iter().enumerate() {
            let vertices: [Vertex; 4] = corners.map(|[x, y]| Vertex {
                position: [x as f32, y as f32],
                color: [0.0, 0.0, 0.0, 0.0],
            });
            let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
            let vertex_start = index as u32 * 4;
            let index_start = index as u32 * 6;
            queue.write_buffer(
                &self.hole_vertex_buffer,
                u64::from(vertex_start) * VERTEX_SIZE,
                bytemuck::cast_slice(&vertices),
            );
            queue.write_buffer(
                &self.hole_index_buffer,
                u64::from(index_start) * INDEX_SIZE,
                bytemuck::cast_slice(&indices),
            );
        }

        // Rotated text: render (or reuse) each element's offscreen texture and compute its
        // on-canvas quad, in `items` order, before the main loop below hands out matching
        // indices into the buffer this uploads to. Any cache entry not touched here (a text
        // whose key -- id, version, scale, dark or alpha -- no longer matches anything on
        // screen this frame) is evicted right after, so a continuously changing `scale` from
        // zooming never accumulates textures across frames.
        let mut rotated_text_commands = Vec::new();
        let mut rotated_quads: Vec<([TexVertex; 4], Arc<wgpu::BindGroup>)> = Vec::new();
        let mut used_rotated_text_keys = HashSet::new();
        for item in items {
            if let DrawItem::RotatedText(draw) = item {
                if let Some(command) =
                    self.ensure_rotated_texture(device, queue, file, draw, scale, frame.dark)
                {
                    rotated_text_commands.push(command);
                }
                let element = &file.elements[draw.element];
                let key = rotated_text_key(file, draw, scale, frame.dark);
                let entry = self
                    .rotated_text_cache
                    .get(&key)
                    .expect("ensure_rotated_texture populated this entry");
                let vertices = rotated_quad_vertices(
                    element,
                    entry.width_px,
                    entry.height_px,
                    entry.raster_scale,
                );
                rotated_quads.push((vertices, entry.bind_group.clone()));
                used_rotated_text_keys.insert(key);
            }
        }
        self.rotated_text_cache
            .retain(|key, _| used_rotated_text_keys.contains(key));
        self.ensure_rotated_quad_capacity(device, rotated_quads.len() as u32);
        for (index, (vertices, _)) in rotated_quads.iter().enumerate() {
            let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
            let vertex_start = index as u32 * 4;
            let index_start = index as u32 * 6;
            queue.write_buffer(
                &self.rotated_quad_vertex_buffer,
                u64::from(vertex_start) * TEX_VERTEX_SIZE,
                bytemuck::cast_slice(vertices),
            );
            queue.write_buffer(
                &self.rotated_quad_index_buffer,
                u64::from(index_start) * INDEX_SIZE,
                bytemuck::cast_slice(&indices),
            );
        }
        if !rotated_quads.is_empty() {
            self.rotated_atlas.trim();
        }

        let canvas_bounds = glyphon::TextBounds {
            left: 0,
            top: 0,
            right: frame.size_px[0] as i32,
            bottom: frame.size_px[1] as i32,
        };

        let mut prepared = Vec::with_capacity(items.len());
        let mut drawn_elements = 0usize;
        let mut next_hole = 0u32;
        let mut next_text_renderer = 0usize;
        let mut next_rotated_quad = 0u32;
        for item in items {
            match item {
                DrawItem::Meshes(draws) => {
                    let mut meshes = Vec::with_capacity(draws.len());
                    for draw in draws {
                        let cached =
                            self.cache
                                .mesh(&file.elements[draw.element], &draw.key, background);
                        let segment = cached
                            .segment
                            .expect("ensure_mesh_segments uploaded every visible mesh");
                        meshes.push((cached.mesh.clone(), segment));
                    }
                    drawn_elements += meshes.len();
                    prepared.push(PreparedItem::Meshes(meshes));
                }
                DrawItem::Isolated {
                    draw,
                    stencil,
                    hole,
                } => {
                    let cached =
                        self.cache
                            .mesh(&file.elements[draw.element], &draw.key, background);
                    let segment = cached
                        .segment
                        .expect("ensure_mesh_segments uploaded every visible mesh");
                    let hole_segment = hole.is_some().then(|| {
                        let segment = Segment {
                            vertex_start: next_hole * 4,
                            index_start: next_hole * 6,
                            index_count: 6,
                        };
                        next_hole += 1;
                        segment
                    });
                    drawn_elements += 1;
                    prepared.push(PreparedItem::Isolated {
                        mesh: cached.mesh.clone(),
                        segment,
                        stencil: *stencil,
                        hole: hole_segment,
                    });
                }
                DrawItem::StencilReset => prepared.push(PreparedItem::StencilReset),
                DrawItem::Text(draws) => {
                    for draw in draws {
                        self.ensure_shaped(file, draw.element, draw.label);
                    }
                    let renderer_index = next_text_renderer;
                    next_text_renderer += 1;
                    self.ensure_text_renderer(device, renderer_index);
                    let areas = build_text_areas(
                        &self.text_lines,
                        file,
                        draws,
                        scroll,
                        scale,
                        canvas_bounds,
                        frame.dark,
                    );
                    self.text_renderers[renderer_index]
                        .prepare(
                            device,
                            queue,
                            &mut self.text_font_system,
                            &mut self.text_atlas,
                            &self.text_viewport,
                            areas,
                            &mut self.text_swash_cache,
                        )
                        .expect("glyphon prepare for in-pass text");
                    drawn_elements += draws.len();
                    prepared.push(PreparedItem::Text { renderer_index });
                }
                DrawItem::RotatedText(_) => {
                    let (_, bind_group) = &rotated_quads[next_rotated_quad as usize];
                    let quad = Segment {
                        vertex_start: next_rotated_quad * 4,
                        index_start: next_rotated_quad * 6,
                        index_count: 6,
                    };
                    next_rotated_quad += 1;
                    drawn_elements += 1;
                    prepared.push(PreparedItem::RotatedText {
                        bind_group: bind_group.clone(),
                        quad,
                    });
                }
            }
        }
        (prepared, drawn_elements, rotated_text_commands)
    }
}

/// `draw`'s rotated-text cache key at the current `scale` and `dark` mode.
fn rotated_text_key(
    file: &scene::SceneFile,
    draw: &TextDraw,
    scale: f32,
    dark: bool,
) -> RotatedTextKey {
    let element = &file.elements[draw.element];
    RotatedTextKey {
        id: element.id().unwrap_or_default().to_owned(),
        version_bits: element.version().to_bits(),
        scale_bits: scale.to_bits(),
        dark,
        alpha_bits: draw.alpha.to_bits(),
    }
}

/// The color a `TextDraw`'s glyphs render in: the element's own `strokeColor` (dark-mode
/// filtered) for real text, `#868e96` (matching the placeholder's dashed box) for a type label,
/// alpha multiplied by `draw.alpha`.
fn text_draw_color(file: &scene::SceneFile, draw: &TextDraw, dark: bool) -> glyphon::Color {
    let element = &file.elements[draw.element];
    let stroke = if draw.label {
        "#868e96"
    } else {
        element
            .base()
            .map(|base| base.stroke_color.as_str())
            .unwrap_or("#000000")
    };
    let [r, g, b, a] = render_color(stroke, dark);
    let a = (a * draw.alpha).clamp(0.0, 1.0);
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    glyphon::Color::rgba(channel(r), channel(g), channel(b), channel(a))
}

/// `element`'s cached shaped lines; panics if `ensure_shaped` was not called for it first.
fn shaped_lines_for<'a>(
    lines: &'a HashMap<(String, u64), Vec<text::ShapedLine>>,
    element: &scene::Element,
) -> &'a [text::ShapedLine] {
    let key = (
        element.id().unwrap_or_default().to_owned(),
        element.version().to_bits(),
    );
    lines
        .get(&key)
        .expect("ensure_shaped populated this element's cache entry")
        .as_slice()
}

/// The `glyphon::TextArea`s for one `Text` batch: every line of every draw, positioned so line
/// i's baseline lands at physical pixel `(element origin + scroll) * scale`, offset down by that
/// line's own `baseline`; a placeholder label is additionally offset 4 scene units right.
fn build_text_areas<'a>(
    lines: &'a HashMap<(String, u64), Vec<text::ShapedLine>>,
    file: &scene::SceneFile,
    draws: &[TextDraw],
    scroll: [f32; 2],
    scale: f32,
    bounds: glyphon::TextBounds,
    dark: bool,
) -> Vec<glyphon::TextArea<'a>> {
    let mut areas = Vec::new();
    for draw in draws {
        let element = &file.elements[draw.element];
        let Some(placement) = element.placement() else {
            continue;
        };
        let origin_x = (placement.x as f32 + scroll[0]) * scale;
        let origin_y = (placement.y as f32 + scroll[1]) * scale;
        let left_local = if draw.label { 4.0 } else { 0.0 };
        let color = text_draw_color(file, draw, dark);
        let shaped = shaped_lines_for(lines, element);
        areas.extend(shaped.iter().map(move |line| glyphon::TextArea {
            buffer: &line.buffer,
            left: origin_x + left_local * scale,
            top: text::text_area_top(origin_y, line.baseline, line.baseline_in_buffer, scale),
            scale,
            bounds,
            default_color: color,
            custom_glyphs: &[],
        }));
    }
    areas
}

/// The four corners (top-left, top-right, bottom-right, bottom-left, matching the hole quads'
/// winding) of a rotated text element's on-canvas quad: sized so its texture maps one texel to
/// one physical pixel at `scale`, centered and rotated exactly like a mesh vertex would be
/// (`tessellate::transform`'s `[x, y] + center + R(angle)(local - center)`).
fn rotated_quad_vertices(
    element: &scene::Element,
    width_px: u32,
    height_px: u32,
    scale: f32,
) -> [TexVertex; 4] {
    let placement = element
        .placement()
        .expect("RotatedText only wraps a Text element, which always has a placement");
    let center = local_center(element).unwrap_or([placement.width / 2.0, placement.height / 2.0]);
    let half_width = f64::from(width_px) / (2.0 * f64::from(scale));
    let half_height = f64::from(height_px) / (2.0 * f64::from(scale));
    let local_corners = [
        [center[0] - half_width, center[1] - half_height],
        [center[0] + half_width, center[1] - half_height],
        [center[0] + half_width, center[1] + half_height],
        [center[0] - half_width, center[1] + half_height],
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let (sin, cos) = placement.angle.sin_cos();
    std::array::from_fn(|i| {
        let [lx, ly] = local_corners[i];
        let dx = lx - center[0];
        let dy = ly - center[1];
        let x = placement.x + center[0] + dx * cos - dy * sin;
        let y = placement.y + center[1] + dx * sin + dy * cos;
        TexVertex {
            position: [x as f32, y as f32],
            uv: uvs[i],
        }
    })
}

fn create_buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}
