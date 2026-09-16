//! Per-element caches keyed by identity and the inputs that change what gets drawn: an
//! [`scene::shape::ElementShape`] depends only on the element's data and dark mode, while a
//! tessellated [`Mesh`] also depends on opacity (baked into vertex alpha) and the zoom bucket
//! (tessellation tolerance).

use std::collections::HashMap;
use std::sync::Arc;

use scene::shape::{ElementShape, ShapeContext, generate_element_shape};

use crate::render::buffers::Segment;
use crate::render::tessellate::{Mesh, Style, tessellate, tolerance_for_bucket};

/// Identifies one cached mesh: an element's tessellated appearance depends on its own data
/// (`id` + `version_bits`, the bit pattern of its `f64` version so equal versions hash equal),
/// dark mode, alpha (element opacity times frame opacity, as `f32::to_bits` since `f32` is not
/// `Eq`/`Hash`) and the zoom bucket's tessellation tolerance.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct MeshKey {
    pub id: String,
    pub version_bits: u64,
    pub dark: bool,
    pub alpha_bits: u32,
    pub bucket: i32,
}

pub struct CachedMesh {
    pub mesh: Arc<Mesh>,
    /// The mesh's location in the shared GPU buffers, if it has been uploaded since the last
    /// [`SceneCache::forget_segments`].
    pub segment: Option<Segment>,
}

/// A shape depends on the element's data and dark mode only, not on opacity or zoom.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ShapeKey {
    id: String,
    version_bits: u64,
    dark: bool,
}

struct MeshEntry {
    cached: CachedMesh,
    last_used_frame: u64,
}

/// Shape and mesh caches shared across frames, plus GPU segment bookkeeping.
pub struct SceneCache {
    shapes: HashMap<ShapeKey, Arc<ElementShape>>,
    meshes: HashMap<MeshKey, MeshEntry>,
    frame: u64,
}

impl SceneCache {
    pub fn new() -> SceneCache {
        SceneCache {
            shapes: HashMap::new(),
            meshes: HashMap::new(),
            frame: 0,
        }
    }

    /// Drops everything; call when a different file is loaded.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.meshes.clear();
        self.frame = 0;
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The element's cached shape, generating and caching it first if this is a miss.
    /// `plan_frame` calls this ahead of [`SceneCache::mesh`] to tell placeholder shapes apart
    /// from real ones without forcing a mesh key (which needs the alpha that decision feeds
    /// into) into existence first.
    pub(crate) fn shape(
        &mut self,
        element: &scene::Element,
        id: &str,
        version_bits: u64,
        dark: bool,
        canvas_background: &str,
    ) -> Arc<ElementShape> {
        let key = ShapeKey {
            id: id.to_owned(),
            version_bits,
            dark,
        };
        self.shapes
            .entry(key)
            .or_insert_with(|| {
                let ctx = ShapeContext {
                    dark_mode: dark,
                    canvas_background_color: canvas_background,
                };
                Arc::new(generate_element_shape(element, &ctx))
            })
            .clone()
    }

    pub fn mesh(
        &mut self,
        element: &scene::Element,
        key: &MeshKey,
        canvas_background: &str,
    ) -> &mut CachedMesh {
        let shape = self.shape(
            element,
            &key.id,
            key.version_bits,
            key.dark,
            canvas_background,
        );
        let frame = self.frame;
        let entry = self.meshes.entry(key.clone()).or_insert_with(|| {
            let style = Style {
                dark: key.dark,
                alpha: f32::from_bits(key.alpha_bits),
                tolerance: tolerance_for_bucket(key.bucket),
            };
            let mesh = tessellate(element, &shape, &style);
            MeshEntry {
                cached: CachedMesh {
                    mesh: Arc::new(mesh),
                    segment: None,
                },
                last_used_frame: frame,
            }
        });
        entry.last_used_frame = frame;
        &mut entry.cached
    }

    /// Forgets GPU segments after the shared buffers were recreated.
    pub fn forget_segments(&mut self) {
        for entry in self.meshes.values_mut() {
            entry.cached.segment = None;
        }
    }

    /// Drops meshes not used during the last `frames` frames.
    pub fn evict(&mut self, frames: u64) {
        let current = self.frame;
        self.meshes
            .retain(|_, entry| current.saturating_sub(entry.last_used_frame) < frames);
    }

    /// The number of meshes currently cached, for [`crate::render::gpu::RenderStats`].
    pub(crate) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
}

impl Default for SceneCache {
    fn default() -> SceneCache {
        SceneCache::new()
    }
}
