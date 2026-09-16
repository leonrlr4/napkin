//! Per-element caches keyed by identity and the inputs that change what gets drawn: an
//! [`scene::shape::ElementShape`] depends only on the element's data and dark mode, while a
//! tessellated [`Mesh`] also depends on opacity (baked into vertex alpha) and the zoom bucket
//! (tessellation tolerance).

use std::collections::{HashMap, HashSet};
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
    /// The element's position in `SceneFile::elements`. Excalidraw renames a duplicate id when
    /// it loads a file (`restore.ts`); napkin does not rewrite files, so two elements can share
    /// every other field here, and only the file index tells them apart.
    pub index: usize,
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

/// A shape depends on the element's data and dark mode only, not on opacity or zoom. `index`
/// disambiguates a duplicate id exactly like [`MeshKey::index`].
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ShapeKey {
    index: usize,
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
    /// Ids `plan_frame` already logged a "text element skipped" warning for, so a malformed
    /// element sitting on screen does not spam a line every frame.
    logged_invalid_text: HashSet<String>,
}

impl SceneCache {
    pub fn new() -> SceneCache {
        SceneCache {
            shapes: HashMap::new(),
            meshes: HashMap::new(),
            frame: 0,
            logged_invalid_text: HashSet::new(),
        }
    }

    /// Drops everything; call when a different file is loaded.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.meshes.clear();
        self.frame = 0;
        self.logged_invalid_text.clear();
    }

    /// Logs (to stderr, once per `id`) that a text element is being skipped because its font
    /// metrics or geometry are unusable.
    pub(crate) fn log_invalid_text_once(&mut self, id: &str) {
        if self.logged_invalid_text.insert(id.to_owned()) {
            eprintln!(
                "napkin: text element {id:?} has a non-finite or out-of-range font size, line \
                 height or position and will not be drawn"
            );
        }
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
        index: usize,
        id: &str,
        version_bits: u64,
        dark: bool,
        canvas_background: &str,
    ) -> Arc<ElementShape> {
        let key = ShapeKey {
            index,
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
            key.index,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    fn rect() -> scene::Element {
        scene::Element::from_value(sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]))
    }

    fn key(element: &scene::Element, index: usize) -> MeshKey {
        MeshKey {
            index,
            id: element.id().unwrap_or_default().to_owned(),
            version_bits: element.version().to_bits(),
            dark: false,
            alpha_bits: 1.0f32.to_bits(),
            bucket: 0,
        }
    }

    #[test]
    fn cache_hit_reuses_the_same_mesh_arc_and_keeps_the_segment() {
        let mut cache = SceneCache::new();
        let element = rect();
        let k = key(&element, 0);

        let first_mesh = cache.mesh(&element, &k, "#ffffff").mesh.clone();
        cache.mesh(&element, &k, "#ffffff").segment = Some(Segment {
            vertex_start: 3,
            index_start: 5,
            index_count: 6,
        });

        let second = cache.mesh(&element, &k, "#ffffff");
        assert!(
            Arc::ptr_eq(&first_mesh, &second.mesh),
            "cache miss re-tessellated"
        );
        assert_eq!(
            second.segment,
            Some(Segment {
                vertex_start: 3,
                index_start: 5,
                index_count: 6
            })
        );
    }

    #[test]
    fn forget_segments_clears_segments_but_keeps_meshes() {
        let mut cache = SceneCache::new();
        let element = rect();
        let k = key(&element, 0);

        let first_mesh = cache.mesh(&element, &k, "#ffffff").mesh.clone();
        cache.mesh(&element, &k, "#ffffff").segment = Some(Segment {
            vertex_start: 1,
            index_start: 2,
            index_count: 3,
        });

        cache.forget_segments();

        let entry = cache.mesh(&element, &k, "#ffffff");
        assert_eq!(entry.segment, None, "forget_segments left a stale segment");
        assert!(
            Arc::ptr_eq(&first_mesh, &entry.mesh),
            "forget_segments should not re-tessellate"
        );
    }

    #[test]
    fn evict_drops_meshes_unused_past_the_window() {
        let mut cache = SceneCache::new();
        let element = rect();
        let k = key(&element, 0);

        cache.begin_frame();
        cache.mesh(&element, &k, "#ffffff");
        assert_eq!(cache.mesh_count(), 1);

        for _ in 0..5 {
            cache.begin_frame();
        }
        cache.evict(3);
        assert_eq!(cache.mesh_count(), 0, "stale mesh survived eviction");
    }

    #[test]
    fn evict_keeps_meshes_touched_within_the_window() {
        let mut cache = SceneCache::new();
        let element = rect();
        let k = key(&element, 0);

        cache.begin_frame();
        cache.mesh(&element, &k, "#ffffff");
        cache.begin_frame();
        cache.mesh(&element, &k, "#ffffff"); // touched again: refreshes last_used_frame

        cache.evict(3);
        assert_eq!(cache.mesh_count(), 1, "recently used mesh was evicted");
    }

    #[test]
    fn clear_drops_shapes_and_meshes() {
        let mut cache = SceneCache::new();
        let element = rect();
        let k = key(&element, 0);

        cache.mesh(&element, &k, "#ffffff");
        assert_eq!(cache.mesh_count(), 1);

        cache.clear();
        assert_eq!(cache.mesh_count(), 0);
        assert!(cache.shapes.is_empty());
    }

    #[test]
    fn duplicate_ids_at_different_indices_cache_independently() {
        let mut cache = SceneCache::new();
        let a = rect();
        let b = rect(); // same id "a", same version: only the index differs
        let key_a = key(&a, 0);
        let key_b = key(&b, 1);
        assert_ne!(key_a, key_b);

        cache.mesh(&a, &key_a, "#ffffff");
        cache.mesh(&b, &key_b, "#ffffff");
        assert_eq!(
            cache.mesh_count(),
            2,
            "duplicate ids collapsed into one cache entry"
        );
    }
}
