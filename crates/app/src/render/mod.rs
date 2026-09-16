//! CSS colors, path conversion, canvas-style dashing and lyon tessellation of element shapes
//! into meshes the GPU canvas draws; shape/mesh caching, GPU buffer segment allocation and
//! per-frame draw list planning on top of them.

pub mod buffers;
pub mod cache;
pub mod color;
pub mod path;
pub mod plan;
pub mod tessellate;
