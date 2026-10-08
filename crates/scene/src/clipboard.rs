//! Excalidraw's clipboard format: `serializeAsClipboardJSON`, `parseClipboard` and
//! `parseClipboardEventTextData` (`packages/excalidraw/clipboard.ts`), at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. napkin's clipboard is plain text:
//! `Ctrl+C` writes [`serialize`]'s JSON string through `egui::Context::copy_text`, `Ctrl+V`
//! reads it back from an `egui::Event::Paste`, the same `text/plain` payload excalidraw.com
//! itself reads and writes.
//!
//! `serializeAsClipboardJSON`'s frame handling (stripping `frameId` from an element copied
//! without its containing frame) is not ported: napkin has no typed frame element, so it never
//! takes that branch. `files` carries the data of every copied image.

use serde_json::{Map, Value, json};

use crate::element::Element;
use crate::file::{FileData, SceneFile};
use crate::selection::Selection;

/// `serializeAsClipboardJSON` for the selected, non-deleted elements, plus a selected
/// container's bound text (`getSelectedElements` with `includeBoundTextElement: true`) and the
/// `files` entry of every selected image that has one.
/// `None` when nothing is selected: an empty `elements: []` payload would still parse back on
/// paste as (useless) Excalidraw data instead of falling through to plain text.
pub fn serialize(file: &SceneFile, selection: &Selection) -> Option<String> {
    let selected: Vec<&Element> = file
        .elements
        .iter()
        .filter(|element| {
            !element.is_deleted()
                && (element.id().is_some_and(|id| selection.contains(id))
                    || element
                        .container_id()
                        .is_some_and(|container_id| selection.contains(container_id)))
        })
        .collect();
    if selected.is_empty() {
        return None;
    }
    let mut files = Map::new();
    for element in &selected {
        if let Element::Image(image) = element
            && let Some(file_id) = image.file_id.value()
            && let Some(data) = file.file_data(file_id)
        {
            files.insert(file_id.clone(), data.to_value());
        }
    }
    let elements: Vec<Value> = selected.into_iter().map(Element::to_value).collect();
    Some(json!({"type": "excalidraw/clipboard", "elements": elements, "files": files}).to_string())
}

/// What [`parse`] found in a pasted string.
pub enum Pasted {
    /// An `excalidraw/clipboard` (or `excalidraw-api/clipboard`, or a whole `.excalidraw`
    /// file's) payload's elements and its `files` entries.
    Elements {
        elements: Vec<Element>,
        files: Vec<FileData>,
    },
    /// Anything else, trimmed.
    Text(String),
}

/// `parseClipboard`'s JSON branch (`clipboardContainsElements`), falling back to
/// `parseClipboardEventTextData`'s trimmed plain text. `None` for text that trims to empty.
pub fn parse(text: &str) -> Option<Pasted> {
    if let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text)
        && matches!(
            root.get("type").and_then(Value::as_str),
            Some("excalidraw" | "excalidraw/clipboard" | "excalidraw-api/clipboard")
        )
        && let Some(Value::Array(elements)) = root.get("elements")
    {
        let files = match root.get("files") {
            Some(Value::Object(files)) => files
                .iter()
                .filter_map(|(key, entry)| FileData::from_value(key, entry))
                .collect(),
            _ => Vec::new(),
        };
        return Some(Pasted::Elements {
            elements: elements.iter().cloned().map(Element::from_value).collect(),
            files,
        });
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Pasted::Text(trimmed.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sample;

    #[test]
    fn serialize_includes_a_selected_containers_bound_text_but_skips_deleted_elements() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::text("t", [0.0, 0.0, 10.0, 10.0], "hi", Some("r")),
            sample::with(
                sample::generic("rectangle", "gone", [0.0, 0.0, 10.0, 10.0]),
                json!({"isDeleted": true}),
            ),
        ]);
        let selected = Selection::from_ids(["r", "gone"]);
        let text = serialize(&file, &selected).expect("selection is non-empty");
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["type"], json!("excalidraw/clipboard"));
        let ids: Vec<&str> = value["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["r", "t"]);

        assert!(serialize(&file, &Selection::new()).is_none());
    }

    #[test]
    fn parse_reads_clipboard_json_and_falls_back_to_trimmed_text() {
        let json_text = serialize(
            &sample::file(vec![sample::generic(
                "rectangle",
                "r",
                [0.0, 0.0, 10.0, 10.0],
            )]),
            &Selection::from_ids(["r"]),
        )
        .unwrap();
        assert!(
            matches!(parse(&json_text), Some(Pasted::Elements { elements, .. }) if elements.len() == 1)
        );
        assert!(matches!(parse("  hello  "), Some(Pasted::Text(text)) if text == "hello"));
        assert!(parse("   ").is_none());
        assert!(parse("").is_none());
    }
}
