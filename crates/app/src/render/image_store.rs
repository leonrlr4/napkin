//! Decoded image cache for image elements: `files[fileId]` data URLs decoded on background
//! threads into premultiplied RGBA8, keyed by `fileId` (a content hash, so an entry stays valid
//! across reloaded scenes). The planner and the GPU renderer ask [`ImageStore::request`] every
//! frame; a decode that is still running reads as [`ImageState::Loading`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use scene::SceneFile;

/// Decoded images with a side longer than this are shrunk before they reach the GPU, which
/// keeps one texture well under the device's `max_texture_dimension_2d` (stored images are
/// already at most 1440 px; this only matters for files authored elsewhere).
pub const MAX_SIDE_PX: u32 = 4096;

/// Entries kept before the least recently requested finished one is dropped.
const MAX_ENTRIES: usize = 128;

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8.
    pub pixels: Vec<u8>,
}

#[derive(Clone)]
pub enum ImageState {
    Loading,
    Ready(Arc<DecodedImage>),
    Failed,
}

enum Slot {
    Loading,
    Ready(Arc<DecodedImage>),
    Failed,
}

struct Entry {
    slot: Slot,
    last_request: u64,
}

#[derive(Default)]
struct Shared {
    entries: HashMap<String, Entry>,
    tick: u64,
    finished: bool,
}

/// Decodes `files` data URLs on a background thread, keyed by `fileId`. Every method takes
/// `&self`: the state lives behind a mutex shared with the decode threads, so the planner can
/// hold a plain reference while it asks for images.
#[derive(Default)]
pub struct ImageStore {
    shared: Arc<Mutex<Shared>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `wake` runs on the decode thread each time a decode finishes (the app uses it to
    /// request a repaint).
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = Some(wake);
    }

    /// The current state of `file_id`, starting a background decode on first request. A
    /// `file_id` with no usable entry in `file.files` is `Failed` and not remembered, so a
    /// file that arrives later is picked up.
    pub fn request(&self, file_id: &str, file: &SceneFile) -> ImageState {
        let mut shared = lock(&self.shared);
        shared.tick += 1;
        let tick = shared.tick;
        if let Some(entry) = shared.entries.get_mut(file_id) {
            entry.last_request = tick;
            return match &entry.slot {
                Slot::Loading => ImageState::Loading,
                Slot::Ready(image) => ImageState::Ready(image.clone()),
                Slot::Failed => ImageState::Failed,
            };
        }
        let Some(data) = file.file_data(file_id) else {
            return ImageState::Failed;
        };
        evict_oldest(&mut shared);
        shared.entries.insert(
            file_id.to_owned(),
            Entry {
                slot: Slot::Loading,
                last_request: tick,
            },
        );
        drop(shared);

        let shared = self.shared.clone();
        let wake = self.wake.clone();
        let id = file_id.to_owned();
        let spawned = std::thread::Builder::new()
            .name("napkin-image-decode".to_owned())
            .spawn(move || {
                let slot = finish(decode_data_url(&data.data_url));
                let mut shared = lock(&shared);
                if let Some(entry) = shared.entries.get_mut(&id) {
                    entry.slot = slot;
                }
                shared.finished = true;
                drop(shared);
                if let Some(wake) = wake {
                    wake();
                }
            });
        if spawned.is_err() {
            if let Some(entry) = lock(&self.shared).entries.get_mut(file_id) {
                entry.slot = Slot::Failed;
            }
            return ImageState::Failed;
        }
        ImageState::Loading
    }

    /// Whether any decode finished since the last call.
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut lock(&self.shared).finished)
    }

    /// Decodes every image `file` references that is not already cached, on the calling
    /// thread. An offscreen render draws in one go, so it needs every image ready first.
    pub fn preload(&self, file: &SceneFile) {
        for element in &file.elements {
            let scene::Element::Image(image) = element else {
                continue;
            };
            if element.is_deleted() {
                continue;
            }
            let Some(file_id) = image.file_id.value() else {
                continue;
            };
            if lock(&self.shared).entries.contains_key(file_id) {
                continue;
            }
            let Some(data) = file.file_data(file_id) else {
                continue;
            };
            let slot = finish(decode_data_url(&data.data_url));
            let mut shared = lock(&self.shared);
            shared.tick += 1;
            let last_request = shared.tick;
            evict_oldest(&mut shared);
            shared
                .entries
                .insert(file_id.to_owned(), Entry { slot, last_request });
        }
    }
}

fn finish(decoded: Option<DecodedImage>) -> Slot {
    match decoded {
        Some(image) => Slot::Ready(Arc::new(image)),
        None => Slot::Failed,
    }
}

/// Drops the least recently requested finished entry once the cache is full.
fn evict_oldest(shared: &mut Shared) {
    if shared.entries.len() < MAX_ENTRIES {
        return;
    }
    let oldest = shared
        .entries
        .iter()
        .filter(|(_, entry)| !matches!(entry.slot, Slot::Loading))
        .min_by_key(|(_, entry)| entry.last_request)
        .map(|(id, _)| id.clone());
    if let Some(id) = oldest {
        shared.entries.remove(&id);
    }
}

/// Decodes the base64 payload of a `data:<mime>;base64,<payload>` URL.
fn decode_data_url(data_url: &str) -> Option<DecodedImage> {
    let (header, payload) = data_url.strip_prefix("data:")?.split_once(',')?;
    if !header.ends_with(";base64") {
        return None;
    }
    let bytes = STANDARD.decode(payload.trim()).ok()?;
    decode_bytes(&bytes)
}

fn decode_bytes(bytes: &[u8]) -> Option<DecodedImage> {
    let mut decoded = image::load_from_memory(bytes).ok()?;
    if decoded.width().max(decoded.height()) > MAX_SIDE_PX {
        decoded = decoded.resize(
            MAX_SIDE_PX,
            MAX_SIDE_PX,
            image::imageops::FilterType::Triangle,
        );
    }
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut pixels = rgba.into_raw();
    for texel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(texel[3]);
        if alpha == 255 {
            continue;
        }
        for channel in &mut texel[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    Some(DecodedImage {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn png_data_url(rgba: [u8; 4]) -> String {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&rgba).unwrap();
            writer.finish().unwrap();
        }
        format!("data:image/png;base64,{}", STANDARD.encode(bytes))
    }

    fn file_with(id: &str, data_url: &str) -> SceneFile {
        let mut file = scene::sample::file(vec![]);
        file.add_missing_file(scene::file::FileData {
            id: id.to_owned(),
            mime_type: "image/png".to_owned(),
            data_url: data_url.to_owned(),
            created: 1.0,
            last_retrieved: 0.0,
        });
        file
    }

    fn wait_ready(store: &mut ImageStore, id: &str, file: &SceneFile) -> ImageState {
        for _ in 0..500 {
            match store.request(id, file) {
                ImageState::Loading => std::thread::sleep(std::time::Duration::from_millis(10)),
                state => return state,
            }
        }
        panic!("decode never finished");
    }

    #[test]
    fn decodes_in_the_background_and_premultiplies() {
        let file = file_with("a", &png_data_url([200, 100, 50, 128]));
        let mut store = ImageStore::new();
        let ImageState::Ready(image) = wait_ready(&mut store, "a", &file) else {
            panic!("expected a decoded image");
        };
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.pixels, [100, 50, 25, 128]);
        assert!(store.take_finished());
        assert!(!store.take_finished());
    }

    #[test]
    fn missing_and_corrupt_files_fail() {
        let file = file_with("bad", "data:image/png;base64,AAAA");
        let mut store = ImageStore::new();
        assert!(matches!(store.request("nope", &file), ImageState::Failed));
        assert!(matches!(
            wait_ready(&mut store, "bad", &file),
            ImageState::Failed
        ));
    }

    #[test]
    fn preload_decodes_on_the_calling_thread() {
        let mut file = file_with("a", &png_data_url([1, 2, 3, 255]));
        file.elements = vec![scene::Element::from_value(scene::sample::with(
            scene::sample::generic("image", "img", [0.0, 0.0, 10.0, 10.0]),
            json!({ "fileId": "a", "status": "saved", "scale": [1, 1], "crop": null }),
        ))];
        let store = ImageStore::new();
        store.preload(&file);
        assert!(matches!(store.request("a", &file), ImageState::Ready(_)));
    }
}
