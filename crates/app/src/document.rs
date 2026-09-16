//! The scene currently open in the viewer.

use std::sync::Arc;

pub struct Document {
    pub name: String,
    pub file: Arc<scene::SceneFile>,
}

impl Document {
    /// An empty scene named "untitled".
    pub fn empty() -> Document {
        Document {
            name: "untitled".to_string(),
            file: Arc::new(scene::SceneFile::new()),
        }
    }

    /// Reads and parses `path`; the name is the file stem. The error names the path.
    pub fn load(path: &std::path::Path) -> Result<Document, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let file = scene::SceneFile::from_json_str(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Document {
            name,
            file: Arc::new(file),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn corpus(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../scene/tests/corpus")
            .join(name)
    }

    #[test]
    fn loads_a_corpus_file_named_after_its_stem() {
        let document = Document::load(&corpus("shapes.excalidraw")).expect("loads");
        assert_eq!(document.name, "shapes");
        assert!(!document.file.elements.is_empty());
    }

    #[test]
    fn errors_name_the_path() {
        let missing = corpus("missing.excalidraw");
        let error = Document::load(&missing).err().expect("missing file fails");
        assert!(error.contains("missing.excalidraw"), "{error}");

        let invalid =
            std::env::temp_dir().join(format!("napkin-invalid-{}.excalidraw", std::process::id()));
        std::fs::write(&invalid, "{ not json").expect("write temp file");
        let error = Document::load(&invalid).err().expect("invalid JSON fails");
        std::fs::remove_file(&invalid).ok();
        assert!(error.contains("invalid JSON"), "{error}");
    }
}
