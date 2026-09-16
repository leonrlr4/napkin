//! CSS colors, path conversion, canvas-style dashing and lyon tessellation of element shapes
//! into meshes the GPU canvas draws; shape/mesh caching, GPU buffer segment allocation and
//! per-frame draw list planning on top of them; the wgpu renderer and egui paint callback that
//! draw the planned meshes.

pub mod buffers;
pub mod cache;
pub mod callback;
pub mod color;
pub mod gpu;
pub mod path;
pub mod plan;
pub mod tessellate;
