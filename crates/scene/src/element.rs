//! Excalidraw elements (`packages/element/src/types.ts` at the pinned commit).
//!
//! Every struct keeps unknown keys in `extra` (spec §5.2). String-valued enums such as
//! `fillStyle` stay `String`: Excalidraw's code compares strings and falls through to a
//! default for unexpected values, and a Rust enum would reject such a file instead.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::json::{Slot, semantic_eq};

/// Fields every element type has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementBase {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub group_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub index: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub roundness: Slot<Roundness>,
    pub seed: f64,
    pub version: f64,
    pub version_nonce: f64,
    pub is_deleted: bool,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub updated: Slot<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Roundness {
    #[serde(rename = "type")]
    pub kind: f64,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub value: Slot<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `rectangle`, `diamond`, `ellipse`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenericElement {
    #[serde(flatten)]
    pub base: ElementBase,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `line`, `arrow`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub points: Vec<[f64; 2]>,
    /// Absent differs from `null`: shape.ts defaults a missing `endArrowhead` to "arrow".
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub start_arrowhead: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub end_arrowhead: Slot<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elbowed: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreedrawElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub points: Vec<[f64; 2]>,
    pub pressures: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulate_pressure: Option<bool>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub stroke_options: Slot<StrokeOptions>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeOptions {
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub variability: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub streamline: Slot<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub text: String,
    pub font_size: f64,
    pub font_family: f64,
    pub text_align: String,
    pub vertical_align: String,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub container_id: Slot<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Rectangle(GenericElement),
    Diamond(GenericElement),
    Ellipse(GenericElement),
    Line(LinearElement),
    Arrow(LinearElement),
    Text(TextElement),
    Freedraw(FreedrawElement),
    /// Any other type (image, frame, embeddable, stickynote, ...), and any element of a
    /// known type whose JSON the typed struct cannot reproduce exactly. Kept verbatim.
    Raw(Value),
}

/// Parses `value` as `T` only if serializing `T` gives back the same JSON. This makes
/// "read then write" lossless by construction: a schema mistake here demotes an element
/// to `Raw` (drawn as a placeholder box) instead of silently rewriting the file.
fn exact<T: DeserializeOwned + Serialize>(value: &Value) -> Option<T> {
    let parsed: T = serde_json::from_value(value.clone()).ok()?;
    let written = serde_json::to_value(&parsed).ok()?;
    semantic_eq(&written, value).then_some(parsed)
}

impl Element {
    pub fn from_value(value: Value) -> Element {
        let typed = match value.get("type").and_then(Value::as_str) {
            Some("rectangle") => exact(&value).map(Element::Rectangle),
            Some("diamond") => exact(&value).map(Element::Diamond),
            Some("ellipse") => exact(&value).map(Element::Ellipse),
            Some("line") => exact(&value).map(Element::Line),
            Some("arrow") => exact(&value).map(Element::Arrow),
            Some("text") => exact(&value).map(Element::Text),
            Some("freedraw") => exact(&value).map(Element::Freedraw),
            _ => None,
        };
        typed.unwrap_or(Element::Raw(value))
    }

    pub fn to_value(&self) -> Value {
        let written = match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => {
                serde_json::to_value(e)
            }
            Element::Line(e) | Element::Arrow(e) => serde_json::to_value(e),
            Element::Text(e) => serde_json::to_value(e),
            Element::Freedraw(e) => serde_json::to_value(e),
            Element::Raw(v) => return v.clone(),
        };
        written.expect("element structs serialize to JSON")
    }

    pub fn base(&self) -> Option<&ElementBase> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&e.base),
            Element::Line(e) | Element::Arrow(e) => Some(&e.base),
            Element::Text(e) => Some(&e.base),
            Element::Freedraw(e) => Some(&e.base),
            Element::Raw(_) => None,
        }
    }

    pub fn base_mut(&mut self) -> Option<&mut ElementBase> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&mut e.base),
            Element::Line(e) | Element::Arrow(e) => Some(&mut e.base),
            Element::Text(e) => Some(&mut e.base),
            Element::Freedraw(e) => Some(&mut e.base),
            Element::Raw(_) => None,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Element::Raw(v) => v.get("id").and_then(Value::as_str),
            _ => self.base().map(|b| b.id.as_str()),
        }
    }

    /// `element.index`; `None` for absent, `null` or non-string.
    pub fn index(&self) -> Option<&str> {
        match self {
            Element::Raw(v) => v.get("index").and_then(Value::as_str),
            _ => self
                .base()
                .and_then(|b| b.index.value())
                .map(String::as_str),
        }
    }

    pub fn set_index(&mut self, index: String) {
        match self {
            Element::Raw(Value::Object(map)) => {
                map.insert("index".into(), Value::String(index));
            }
            Element::Raw(_) => {}
            _ => self.base_mut().expect("typed element").index = Slot::Value(index),
        }
    }

    pub fn is_deleted(&self) -> bool {
        match self {
            Element::Raw(v) => v.get("isDeleted").and_then(Value::as_bool).unwrap_or(false),
            _ => self.base().is_some_and(|b| b.is_deleted),
        }
    }
}

impl Serialize for Element {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Element {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Value::deserialize(deserializer).map(Element::from_value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rectangle() -> Value {
        json!({
            "id": "r", "type": "rectangle", "x": 1, "y": 2, "width": 3, "height": 4, "angle": 0,
            "strokeColor": "#1e1e1e", "backgroundColor": "transparent", "fillStyle": "solid",
            "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100,
            "groupIds": [], "frameId": null, "index": "a0", "roundness": null, "seed": 1,
            "version": 3, "versionNonce": 4, "isDeleted": false, "boundElements": null,
            "updated": 5, "link": null, "locked": false, "customData": {"k": [1, 2]}
        })
    }

    #[test]
    fn typed_element_writes_back_identical_json() {
        let element = Element::from_value(rectangle());
        assert!(matches!(element, Element::Rectangle(_)));
        assert!(semantic_eq(&element.to_value(), &rectangle()));
    }

    #[test]
    fn absent_and_null_fields_stay_distinct() {
        let mut old = rectangle();
        old.as_object_mut().unwrap().remove("index");
        let element = Element::from_value(old.clone());
        assert!(matches!(element, Element::Rectangle(_)));
        assert_eq!(element.to_value().get("index"), None);
        assert_eq!(Element::from_value(rectangle()).index(), Some("a0"));
    }

    #[test]
    fn null_index_stays_null_and_set_index_writes_through_typed_path() {
        // `"index": null` (as opposed to absent, covered above): still typed, `index()`
        // sees no value, and writing back keeps the key present but null.
        let mut null_indexed = rectangle();
        null_indexed["index"] = Value::Null;
        let element = Element::from_value(null_indexed.clone());
        assert!(matches!(element, Element::Rectangle(_)));
        assert_eq!(element.index(), None);
        assert_eq!(element.to_value().get("index"), Some(&Value::Null));

        // `set_index` on a typed element goes through `base_mut`, not the `Raw` map path.
        let mut element = Element::from_value(rectangle());
        assert!(matches!(element, Element::Rectangle(_)));
        element.set_index("b1".into());
        assert_eq!(element.index(), Some("b1"));
        assert_eq!(element.to_value()["index"], json!("b1"));
    }

    #[test]
    fn unrepresentable_known_type_falls_back_to_raw() {
        let mut bad = rectangle();
        bad["roughness"] = json!("rough");
        let element = Element::from_value(bad.clone());
        assert_eq!(element, Element::Raw(bad));
    }

    #[test]
    fn null_in_absent_only_field_falls_back_to_raw() {
        // `elbowed` is `Option<bool>`: serde reads `null` as `None` and would then omit the
        // key on write. Only the exact round-trip check in `from_value` catches that.
        let mut arrow = rectangle();
        arrow["type"] = json!("arrow");
        arrow["points"] = json!([[0, 0], [10, 10]]);
        arrow["elbowed"] = Value::Null;
        assert!(matches!(Element::from_value(arrow), Element::Raw(_)));
    }

    #[test]
    fn unknown_type_is_raw_but_indexable() {
        let mut element =
            Element::from_value(json!({"id": "i", "type": "image", "index": null, "fileId": "f"}));
        assert_eq!(element.id(), Some("i"));
        assert_eq!(element.index(), None);
        element.set_index("a1".into());
        assert_eq!(
            element.to_value(),
            json!({"id": "i", "type": "image", "index": "a1", "fileId": "f"})
        );
    }
}
