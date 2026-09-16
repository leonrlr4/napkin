//! The wgpu canvas renderer: pipelines for the four stencil-based draw kinds `plan_frame`
//! produces, a shared bump-allocated vertex/index buffer for element meshes, a small per-frame
//! buffer for arrow-label stencil holes, and the uniform that carries the camera into clip
//! space. `CanvasRenderer::prepare` plans the frame and uploads whatever changed;
//! `CanvasRenderer::paint` only issues draw calls, so every decision (which meshes are visible,
//! where they live in the shared buffers) must already be resolved by the time `paint` runs.

use std::sync::Arc;

use crate::camera::Camera;
use crate::render::buffers::{Segment, SegmentAllocator};
use crate::render::cache::SceneCache;
use crate::render::plan::{DrawItem, ElementDraw, View, plan_frame};
use crate::render::tessellate::{Mesh, Vertex};

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

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: STENCIL_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: spec.face,
                back: spec.face,
                read_mask: 0xff,
                write_mask: 0xff,
            },
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: SAMPLE_COUNT,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// Every `ElementDraw` a frame's plan references, in the order `paint` will need them (`Text`
/// and `RotatedText` carry no mesh and are skipped; Task 7 draws them).
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

    cache: SceneCache,
    prepared: Vec<PreparedItem>,
    stats: RenderStats,
}

impl CanvasRenderer {
    pub fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
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

        CanvasRenderer {
            plain,
            isolated,
            mask,
            reset,
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
            cache: SceneCache::new(),
            prepared: Vec::new(),
            stats: RenderStats::default(),
        }
    }

    /// Clears caches; call when another file is shown.
    pub fn reset(&mut self) {
        self.cache.clear();
        self.allocator
            .reset(self.vertex_capacity, self.index_capacity);
        self.prepared.clear();
        self.stats = RenderStats::default();
    }

    /// Plans the frame and uploads what it needs. The returned buffers (offscreen text in
    /// Task 7) must be submitted before the pass that calls `paint`.
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
        let (prepared, drawn_elements) =
            self.build_prepared(device, queue, &frame.file, background, &items);

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

        Vec::new()
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

    /// Resolves `items` into `PreparedItem`s, uploading this frame's stencil-hole quads (every
    /// element mesh must already have a segment, via `ensure_mesh_segments`). Returns the
    /// prepared list and the number of elements drawn.
    fn build_prepared(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        file: &scene::SceneFile,
        background: &str,
        items: &[DrawItem],
    ) -> (Vec<PreparedItem>, usize) {
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

        let mut prepared = Vec::with_capacity(items.len());
        let mut drawn_elements = 0usize;
        let mut next_hole = 0u32;
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
                DrawItem::Text(_) | DrawItem::RotatedText(_) => {}
            }
        }
        (prepared, drawn_elements)
    }
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
