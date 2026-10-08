//! The `.excalidraw` document: `{ type, version, source, elements, appState, files }`.

use std::fmt;

use serde_json::{Map, Value, json};

use crate::element::Element;
use crate::json::normalize_numbers;

/// `COLOR_PALETTE.white`, Excalidraw's default `viewBackgroundColor`.
pub const DEFAULT_VIEW_BACKGROUND_COLOR: &str = "#ffffff";

#[derive(Debug)]
pub enum LoadError {
    Json(serde_json::Error),
    /// The top level is not an object with `"type": "excalidraw"`.
    NotExcalidraw,
    ElementsNotArray,
    AppStateNotObject,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "invalid JSON: {e}"),
            LoadError::NotExcalidraw => {
                write!(f, "not an Excalidraw file (type is not \"excalidraw\")")
            }
            LoadError::ElementsNotArray => write!(f, "\"elements\" is not an array"),
            LoadError::AppStateNotObject => write!(f, "\"appState\" is not an object"),
        }
    }
}

impl std::error::Error for LoadError {}

/// The view napkin stores in `appState.napkin` (spec §5.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NapkinView {
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub zoom: f64,
}

/// One entry of the document's `files` map (`BinaryFileData`).
#[derive(Clone, Debug, PartialEq)]
pub struct FileData {
    pub id: String,
    pub mime_type: String,
    pub data_url: String,
    pub created: f64,
    pub last_retrieved: f64,
}

impl FileData {
    /// The `BinaryFileData` JSON object.
    pub fn to_value(&self) -> Value {
        json!({
            "mimeType": self.mime_type,
            "id": self.id,
            "dataURL": self.data_url,
            "created": self.created,
            "lastRetrieved": self.last_retrieved,
        })
    }

    /// Reads a `files` entry stored under `key`; `None` without string `mimeType` and
    /// `dataURL`. The entry's own `id` wins over `key` (`addMissingFiles` uses `fileData.id`).
    pub fn from_value(key: &str, entry: &Value) -> Option<FileData> {
        Some(FileData {
            id: entry
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_owned(),
            mime_type: entry.get("mimeType")?.as_str()?.to_owned(),
            data_url: entry.get("dataURL")?.as_str()?.to_owned(),
            created: entry.get("created").and_then(Value::as_f64).unwrap_or(0.0),
            last_retrieved: entry
                .get("lastRetrieved")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneFile {
    /// Every top-level key in file order. `elements` and `appState` hold `null` while the
    /// parsed copies below are authoritative; `to_json_string` puts them back in place.
    root: Map<String, Value>,
    pub elements: Vec<Element>,
    pub app_state: Map<String, Value>,
}

impl SceneFile {
    /// An empty document as napkin creates it.
    pub fn new() -> SceneFile {
        let mut root = Map::new();
        root.insert("type".into(), json!("excalidraw"));
        root.insert("version".into(), json!(2));
        root.insert("source".into(), json!("napkin"));
        root.insert("elements".into(), Value::Null);
        root.insert("appState".into(), Value::Null);
        root.insert("files".into(), json!({}));
        SceneFile {
            root,
            elements: Vec::new(),
            app_state: Map::new(),
        }
    }

    /// `serde_json` rejects a lone UTF-16 surrogate escape such as `"\ud83d"` (a `\uXXXX`
    /// escape in the `0xD800..=0xDFFF` range with no matching low/high surrogate next to
    /// it) that `JSON.parse` accepts and `JSON.stringify` can itself write back out. Such a
    /// file fails to load here with a loud `LoadError::Json`, not a silent data loss.
    pub fn from_json_str(text: &str) -> Result<SceneFile, LoadError> {
        let value: Value = serde_json::from_str(text).map_err(LoadError::Json)?;
        let Value::Object(mut root) = value else {
            return Err(LoadError::NotExcalidraw);
        };
        if root.get("type").and_then(Value::as_str) != Some("excalidraw") {
            return Err(LoadError::NotExcalidraw);
        }
        let elements = match root.get_mut("elements").map(Value::take) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items.into_iter().map(Element::from_value).collect(),
            Some(_) => return Err(LoadError::ElementsNotArray),
        };
        let app_state = match root.get_mut("appState").map(Value::take) {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map,
            Some(_) => return Err(LoadError::AppStateNotObject),
        };
        Ok(SceneFile {
            root,
            elements,
            app_state,
        })
    }

    /// Pretty-printed like Excalidraw's `JSON.stringify(data, null, 2)`.
    pub fn to_json_string(&self) -> String {
        self.to_json_string_with_view(None)
    }

    /// Like [`to_json_string`](Self::to_json_string), with `appState.napkin` set to `view` in
    /// the written text only; `self.app_state` is untouched.
    pub fn to_json_string_with_view(&self, view: Option<NapkinView>) -> String {
        let mut root = self.root.clone();
        let elements = Value::Array(self.elements.iter().map(Element::to_value).collect());
        if root.contains_key("elements") || !self.elements.is_empty() {
            root.insert("elements".into(), elements);
        }
        let mut app_state = self.app_state.clone();
        if let Some(view) = view {
            app_state.insert(
                "napkin".into(),
                json!({ "scrollX": view.scroll_x, "scrollY": view.scroll_y, "zoom": view.zoom }),
            );
        }
        if root.contains_key("appState") || !app_state.is_empty() {
            root.insert("appState".into(), Value::Object(app_state));
        }
        let mut value = Value::Object(root);
        normalize_numbers(&mut value);
        serde_json::to_string_pretty(&value).expect("JSON values serialize")
    }

    /// `files[id]`'s `dataURL` and `mimeType`; `None` when the entry or either string is
    /// missing. `created` and `lastRetrieved` read as 0 when absent.
    pub fn file_data(&self, id: &str) -> Option<FileData> {
        let entry = self.root.get("files")?.get(id)?;
        Some(FileData {
            id: id.to_owned(),
            mime_type: entry.get("mimeType")?.as_str()?.to_owned(),
            data_url: entry.get("dataURL")?.as_str()?.to_owned(),
            created: entry.get("created").and_then(Value::as_f64).unwrap_or(0.0),
            last_retrieved: entry
                .get("lastRetrieved")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        })
    }

    /// `addMissingFiles`: stores `data` under `files[data.id]` unless an entry with a
    /// `dataURL` is already there. Returns whether it stored one.
    pub fn add_missing_file(&mut self, data: FileData) -> bool {
        let has_data = self
            .root
            .get("files")
            .and_then(|files| files.get(&data.id))
            .is_some_and(|entry| entry.get("dataURL").is_some_and(Value::is_string));
        if has_data {
            return false;
        }
        let files = self
            .root
            .entry("files")
            .or_insert_with(|| Value::Object(Map::new()));
        if !files.is_object() {
            *files = Value::Object(Map::new());
        }
        files
            .as_object_mut()
            .expect("normalized to an object above")
            .insert(data.id.clone(), data.to_value());
        true
    }

    pub fn view_background_color(&self) -> &str {
        self.app_state
            .get("viewBackgroundColor")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_VIEW_BACKGROUND_COLOR)
    }

    /// `None` when absent or malformed, e.g. after excalidraw.com re-saved the file.
    pub fn napkin_view(&self) -> Option<NapkinView> {
        let view = self.app_state.get("napkin")?;
        Some(NapkinView {
            scroll_x: view.get("scrollX")?.as_f64()?,
            scroll_y: view.get("scrollY")?.as_f64()?,
            zoom: view.get("zoom")?.as_f64()?,
        })
    }

    pub fn set_napkin_view(&mut self, view: NapkinView) {
        self.app_state.insert(
            "napkin".into(),
            json!({ "scrollX": view.scroll_x, "scrollY": view.scroll_y, "zoom": view.zoom }),
        );
    }
}

impl Default for SceneFile {
    fn default() -> Self {
        SceneFile::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::semantic_eq;

    #[test]
    fn unknown_top_level_and_app_state_keys_survive() {
        let text = r##"{"type":"excalidraw","version":2,"source":"https://excalidraw.com",
            "elements":[{"type":"image","id":"i","fileId":"f1"}],
            "appState":{"gridSize":20,"viewBackgroundColor":"#fffce8"},
            "files":{"f1":{"dataURL":"data:image/png;base64,AAAA"}},"future":{"x":1}}"##;
        let file = SceneFile::from_json_str(text).unwrap();
        let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
        assert!(semantic_eq(&written, &serde_json::from_str(text).unwrap()));
        assert_eq!(file.view_background_color(), "#fffce8");
    }

    #[test]
    fn files_are_read_and_added_without_overwriting() {
        let mut file = SceneFile::new();
        let data = FileData {
            id: "f1".into(),
            mime_type: "image/png".into(),
            data_url: "data:image/png;base64,AAAA".into(),
            created: 1.0,
            last_retrieved: 1.0,
        };
        assert!(file.add_missing_file(data.clone()));
        assert!(!file.add_missing_file(FileData {
            data_url: "data:image/png;base64,BBBB".into(),
            ..data
        }));
        assert_eq!(
            file.file_data("f1").unwrap().data_url,
            "data:image/png;base64,AAAA"
        );
        let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
        assert_eq!(written["files"]["f1"]["mimeType"], json!("image/png"));
        assert_eq!(written["files"]["f1"]["id"], json!("f1"));
        assert!(semantic_eq(
            &written["files"]["f1"]["lastRetrieved"],
            &json!(1.0)
        ));
    }

    #[test]
    fn missing_elements_and_app_state_stay_missing() {
        let file = SceneFile::from_json_str(r#"{"type":"excalidraw"}"#).unwrap();
        assert_eq!(file.to_json_string(), "{\n  \"type\": \"excalidraw\"\n}");
    }

    #[test]
    fn napkin_view_round_trips() {
        let mut file = SceneFile::new();
        assert_eq!(file.napkin_view(), None);
        let view = NapkinView {
            scroll_x: -10.5,
            scroll_y: 3.0,
            zoom: 1.25,
        };
        file.set_napkin_view(view);
        let reread = SceneFile::from_json_str(&file.to_json_string()).unwrap();
        assert_eq!(reread.napkin_view(), Some(view));
    }

    #[test]
    fn null_elements_and_app_state_load_as_empty() {
        let file =
            SceneFile::from_json_str(r#"{"type":"excalidraw","elements":null,"appState":null}"#)
                .unwrap();
        assert!(file.elements.is_empty() && file.app_state.is_empty());
        let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
        assert!(
            semantic_eq(
                &written,
                &json!({"type": "excalidraw", "elements": [], "appState": {}})
            ),
            "{written}"
        );
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidraw","appState":3}"#),
            Err(LoadError::AppStateNotObject)
        ));
    }

    #[test]
    fn writing_with_a_view_leaves_the_file_untouched() {
        let file = SceneFile::new();
        let view = NapkinView {
            scroll_x: 1.0,
            scroll_y: 2.0,
            zoom: 1.5,
        };
        let text = file.to_json_string_with_view(Some(view));
        assert_eq!(
            SceneFile::from_json_str(&text).unwrap().napkin_view(),
            Some(view)
        );
        assert_eq!(file.napkin_view(), None);
        assert_eq!(file.to_json_string_with_view(None), file.to_json_string());
    }

    #[test]
    fn rejects_other_documents() {
        assert!(matches!(
            SceneFile::from_json_str("[]"),
            Err(LoadError::NotExcalidraw)
        ));
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidrawlib"}"#),
            Err(LoadError::NotExcalidraw)
        ));
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidraw","elements":{}}"#),
            Err(LoadError::ElementsNotArray)
        ));
        assert!(matches!(
            SceneFile::from_json_str("{"),
            Err(LoadError::Json(_))
        ));
    }
}
