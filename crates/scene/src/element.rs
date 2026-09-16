//! Excalidraw elements (`packages/element/src/types.ts` at the pinned commit).
//!
//! Every struct keeps unknown keys in `extra` (spec §5.2). String-valued enums such as
//! `fillStyle` stay `String`: Excalidraw's code compares strings and falls through to a
//! default for unexpected values, and a Rust enum would reject such a file instead.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

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

/// Which end of a line or arrow a binding or arrowhead applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinearEnd {
    Start,
    End,
}

/// Where an element sits: its top-left origin, size and rotation in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
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
    /// A `value` missing a field `ElementBase` requires (no `#[serde(default)]`), such as
    /// `groupIds` or `strokeStyle` in a file saved by an older Excalidraw version, fails
    /// `exact` here and falls back to `Element::Raw`: it round-trips through save/load
    /// unchanged, but napkin draws it as the dashed placeholder box (spec §1.2) instead of
    /// its rectangle/diamond/.../freedraw sketch, same as any other type this crate has no
    /// typed struct for.
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

    /// The typed element structs' `extra` map, where `frameId` lives (it is not an
    /// `ElementBase` field). `None` for `Raw`, which has no separate extra map.
    fn extra(&self) -> Option<&Map<String, Value>> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&e.extra),
            Element::Line(e) | Element::Arrow(e) => Some(&e.extra),
            Element::Text(e) => Some(&e.extra),
            Element::Freedraw(e) => Some(&e.extra),
            Element::Raw(_) => None,
        }
    }

    /// `None` for a `Raw` element whose `x`, `y`, `width` or `height` is missing or not a
    /// number; a missing or non-numeric `angle` reads as 0.
    pub fn placement(&self) -> Option<Placement> {
        match self {
            Element::Raw(v) => {
                let x = v.get("x").and_then(Value::as_f64)?;
                let y = v.get("y").and_then(Value::as_f64)?;
                let width = v.get("width").and_then(Value::as_f64)?;
                let height = v.get("height").and_then(Value::as_f64)?;
                let angle = v.get("angle").and_then(Value::as_f64).unwrap_or(0.0);
                Some(Placement {
                    x,
                    y,
                    width,
                    height,
                    angle,
                })
            }
            _ => self.base().map(|b| Placement {
                x: b.x,
                y: b.y,
                width: b.width,
                height: b.height,
                angle: b.angle,
            }),
        }
    }

    /// `frameId` when it is a string.
    pub fn frame_id(&self) -> Option<&str> {
        match self {
            Element::Raw(v) => v.get("frameId").and_then(Value::as_str),
            _ => self
                .extra()
                .and_then(|e| e.get("frameId"))
                .and_then(Value::as_str),
        }
    }

    /// `opacity` (0 to 100); a `Raw` element without a numeric one reads as 100.
    pub fn opacity(&self) -> f64 {
        match self {
            Element::Raw(v) => v.get("opacity").and_then(Value::as_f64).unwrap_or(100.0),
            _ => self.base().map(|b| b.opacity).unwrap_or(100.0),
        }
    }

    /// The `type` string.
    pub fn kind(&self) -> &str {
        match self {
            Element::Raw(v) => v.get("type").and_then(Value::as_str).unwrap_or(""),
            _ => self.base().map(|b| b.kind.as_str()).unwrap_or(""),
        }
    }

    /// `version`; a `Raw` element without a numeric one reads as 0.
    pub fn version(&self) -> f64 {
        match self {
            Element::Raw(v) => v.get("version").and_then(Value::as_f64).unwrap_or(0.0),
            _ => self.base().map(|b| b.version).unwrap_or(0.0),
        }
    }

    /// `versionNonce`; a `Raw` element without a numeric one reads as 0.
    pub fn version_nonce(&self) -> f64 {
        match self {
            Element::Raw(v) => v.get("versionNonce").and_then(Value::as_f64).unwrap_or(0.0),
            _ => self.base().map(|b| b.version_nonce).unwrap_or(0.0),
        }
    }

    /// `locked === true`.
    pub fn is_locked(&self) -> bool {
        self.field("locked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// `groupIds`, innermost first; missing or non-string entries are skipped. A typed
    /// element's `ElementBase::group_ids` is already an all-string array (a non-string entry
    /// would have failed the exact round-trip check in `from_value` and landed as `Raw`
    /// instead), so only the `Raw` path needs to filter.
    pub fn group_ids(&self) -> Vec<&str> {
        match self {
            Element::Raw(v) => v
                .get("groupIds")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default(),
            _ => self
                .base()
                .map(|b| b.group_ids.iter().map(String::as_str).collect())
                .unwrap_or_default(),
        }
    }

    /// A text element's `containerId` when it is a string; `None` for every other type.
    pub fn container_id(&self) -> Option<&str> {
        match self {
            Element::Text(t) => t.container_id.value().map(String::as_str),
            Element::Raw(v) if v.get("type").and_then(Value::as_str) == Some("text") => {
                v.get("containerId").and_then(Value::as_str)
            }
            _ => None,
        }
    }

    /// `boundElements` as `(id, type)` pairs; `null`, missing or malformed entries are
    /// skipped.
    pub fn bound_elements(&self) -> Vec<(&str, &str)> {
        match self.field("boundElements") {
            Some(Value::Array(entries)) => entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id")?.as_str()?;
                    let kind = entry.get("type")?.as_str()?;
                    Some((id, kind))
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `startBinding.elementId` / `endBinding.elementId` of a line or arrow.
    pub fn binding_target(&self, end: LinearEnd) -> Option<&str> {
        self.field(binding_key(end))?
            .get("elementId")
            .and_then(Value::as_str)
    }

    /// Sets `x` and `y` (a `Raw` element only when both are already numbers).
    pub fn set_position(&mut self, x: f64, y: f64) {
        match self {
            Element::Raw(Value::Object(map)) => {
                let has_xy = map.get("x").is_some_and(Value::is_number)
                    && map.get("y").is_some_and(Value::is_number);
                if has_xy {
                    map.insert("x".into(), json!(x));
                    map.insert("y".into(), json!(y));
                }
            }
            Element::Raw(_) => {}
            _ => {
                let base = self.base_mut().expect("typed element");
                base.x = x;
                base.y = y;
            }
        }
    }

    pub fn set_deleted(&mut self, deleted: bool) {
        match self {
            Element::Raw(Value::Object(map)) => {
                map.insert("isDeleted".into(), json!(deleted));
            }
            Element::Raw(_) => {}
            _ => self.base_mut().expect("typed element").is_deleted = deleted,
        }
    }

    /// Removes every `boundElements` entry whose `id` is `id`; the array stays, possibly
    /// empty. A no-op when there is no array to remove from.
    pub fn remove_bound_element(&mut self, id: &str) {
        if let Some(Value::Array(entries)) = self.field_mut("boundElements") {
            entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(id));
        }
    }

    /// Sets `startBinding` or `endBinding` to `null`, even when the key is currently absent
    /// (`mutateElement`'s result). Only typed line/arrow elements carry these keys in
    /// `extra`; a no-op on every other element, `Raw` included (spec §5.2's restricted
    /// `Raw` mutation surface has no binding fields).
    pub fn clear_binding(&mut self, end: LinearEnd) {
        if let Some(extra) = self.extra_mut() {
            extra.insert(binding_key(end).into(), Value::Null);
        }
    }

    /// Sets a text element's `containerId` to `null`. A no-op for every other element.
    pub fn clear_container_id(&mut self) {
        if let Element::Text(t) = self {
            t.container_id = Slot::Null;
        }
    }

    /// Sets `frameId` to `null`, even when the key is currently absent.
    pub fn clear_frame_id(&mut self) {
        match self {
            Element::Raw(Value::Object(map)) => {
                map.insert("frameId".into(), Value::Null);
            }
            Element::Raw(_) => {}
            _ => {
                if let Some(extra) = self.extra_mut() {
                    extra.insert("frameId".into(), Value::Null);
                }
            }
        }
    }

    /// A key that lives in the typed element structs' `extra` map (`Raw`'s own JSON object
    /// for `Raw`), such as `locked`, `boundElements`, `startBinding` or `endBinding`.
    fn field(&self, key: &str) -> Option<&Value> {
        match self {
            Element::Raw(v) => v.get(key),
            _ => self.extra().and_then(|e| e.get(key)),
        }
    }

    /// The typed element structs' `extra` map, mutably. `None` for `Raw`, which has no
    /// separate extra map.
    fn extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&mut e.extra),
            Element::Line(e) | Element::Arrow(e) => Some(&mut e.extra),
            Element::Text(e) => Some(&mut e.extra),
            Element::Freedraw(e) => Some(&mut e.extra),
            Element::Raw(_) => None,
        }
    }

    /// Mutable counterpart of [`Element::field`].
    fn field_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Element::Raw(v) => v.get_mut(key),
            _ => self.extra_mut().and_then(|e| e.get_mut(key)),
        }
    }
}

/// The `extra`/JSON key `startBinding`/`endBinding` mutation and lookup goes through.
fn binding_key(end: LinearEnd) -> &'static str {
    match end {
        LinearEnd::Start => "startBinding",
        LinearEnd::End => "endBinding",
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

    #[test]
    fn typed_elements_expose_placement_and_attributes() {
        let mut value = rectangle();
        value["angle"] = json!(0.5);
        value["opacity"] = json!(60);
        value["frameId"] = json!("frame-1");
        let element = Element::from_value(value);
        assert!(matches!(element, Element::Rectangle(_)));
        assert_eq!(
            element.placement(),
            Some(Placement {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                angle: 0.5
            })
        );
        assert_eq!(element.frame_id(), Some("frame-1"));
        assert_eq!(element.opacity(), 60.0);
        assert_eq!(element.kind(), "rectangle");
        assert_eq!(element.version(), 3.0);
    }

    #[test]
    fn raw_elements_expose_placement_and_attributes() {
        let image = Element::from_value(json!({
            "id": "i", "type": "image", "x": 5, "y": 6, "width": 7, "height": 8,
            "opacity": 40, "frameId": "f", "version": 9
        }));
        assert!(matches!(image, Element::Raw(_)));
        assert_eq!(
            image.placement(),
            Some(Placement {
                x: 5.0,
                y: 6.0,
                width: 7.0,
                height: 8.0,
                angle: 0.0
            })
        );
        assert_eq!(image.frame_id(), Some("f"));
        assert_eq!(image.opacity(), 40.0);
        assert_eq!(image.kind(), "image");
        assert_eq!(image.version(), 9.0);

        let bare = Element::from_value(json!({ "id": "b", "type": "magic", "x": "no" }));
        assert_eq!(bare.placement(), None);
        assert_eq!(bare.frame_id(), None);
        assert_eq!(bare.opacity(), 100.0);
        assert_eq!(bare.version(), 0.0);
    }

    #[test]
    fn reads_editing_attributes_from_typed_and_raw_elements() {
        let mut value = rectangle();
        value["locked"] = json!(true);
        value["groupIds"] = json!(["inner", "outer"]);
        value["boundElements"] =
            json!([{"id": "t1", "type": "text"}, {"id": "a1", "type": "arrow"}]);
        let element = Element::from_value(value);
        assert!(matches!(element, Element::Rectangle(_)));
        assert!(element.is_locked());
        assert_eq!(element.group_ids(), vec!["inner", "outer"]);
        assert_eq!(
            element.bound_elements(),
            vec![("t1", "text"), ("a1", "arrow")]
        );
        assert_eq!(element.version_nonce(), 4.0);
        assert_eq!(element.container_id(), None);

        let image = Element::from_value(json!({
            "id": "i", "type": "image", "x": 1, "y": 2, "groupIds": ["g"], "locked": false,
            "boundElements": [{"id": "a", "type": "arrow"}], "versionNonce": 7
        }));
        assert!(!image.is_locked());
        assert_eq!(image.group_ids(), vec!["g"]);
        assert_eq!(image.bound_elements(), vec![("a", "arrow")]);
        assert_eq!(image.version_nonce(), 7.0);
    }

    #[test]
    fn reads_bindings_and_containers() {
        let arrow = Element::from_value(crate::sample::with(
            crate::sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]),
            json!({"startBinding": {"elementId": "r", "fixedPoint": [0.5, 0.5], "mode": "orbit"}}),
        ));
        assert!(matches!(arrow, Element::Arrow(_)));
        assert_eq!(arrow.binding_target(LinearEnd::Start), Some("r"));
        assert_eq!(arrow.binding_target(LinearEnd::End), None);
        let text = Element::from_value(crate::sample::text(
            "t",
            [0.0, 0.0, 10.0, 10.0],
            "hi",
            Some("r"),
        ));
        assert_eq!(text.container_id(), Some("r"));
    }

    #[test]
    fn setters_write_through_typed_and_raw_paths() {
        let mut rect = Element::from_value(crate::sample::with(
            rectangle(),
            json!({"boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}], "frameId": "f"}),
        ));
        rect.set_position(7.0, 8.0);
        rect.remove_bound_element("a");
        rect.clear_frame_id();
        rect.set_deleted(true);
        let value = rect.to_value();
        assert_eq!(
            (value["x"].clone(), value["y"].clone()),
            (json!(7.0), json!(8.0))
        );
        assert_eq!(value["boundElements"], json!([{"id": "t", "type": "text"}]));
        assert_eq!(value["frameId"], Value::Null);
        assert_eq!(value["isDeleted"], json!(true));

        let mut arrow = Element::from_value(crate::sample::with(
            crate::sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 0.0]]),
            json!({"endBinding": {"elementId": "r", "fixedPoint": [0.5, 0.5], "mode": "orbit"}}),
        ));
        arrow.clear_binding(LinearEnd::End);
        assert_eq!(arrow.to_value()["endBinding"], Value::Null);
        assert_eq!(arrow.binding_target(LinearEnd::End), None);

        let mut text = Element::from_value(crate::sample::text(
            "t",
            [0.0, 0.0, 10.0, 10.0],
            "hi",
            Some("r"),
        ));
        text.clear_container_id();
        assert_eq!(text.to_value()["containerId"], Value::Null);

        let mut image = Element::from_value(json!({"id": "i", "type": "image", "x": 1, "y": 2,
            "frameId": "f", "boundElements": [{"id": "a", "type": "arrow"}]}));
        image.set_position(3.0, 4.0);
        image.remove_bound_element("a");
        image.clear_frame_id();
        assert_eq!(
            image.to_value(),
            json!({"id": "i", "type": "image", "x": 3.0, "y": 4.0, "frameId": null, "boundElements": []})
        );
        let mut bare = Element::from_value(json!({"id": "b", "type": "magic"}));
        bare.set_position(1.0, 1.0);
        assert_eq!(bare.to_value(), json!({"id": "b", "type": "magic"}));
    }
}
