//! Complete element JSON for tests and fixtures: enough keys that
//! [`crate::Element::from_value`] loads a typed element, not [`crate::Element::Raw`].

use serde_json::{Value, json};

/// The bounding box extents (max - min per axis) of `points`, `(0, 0)` for no points.
fn points_extent(points: &[[f64; 2]]) -> (f64, f64) {
    let Some(first) = points.first() else {
        return (0.0, 0.0);
    };
    let mut min = *first;
    let mut max = *first;
    for p in points {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    (max[0] - min[0], max[1] - min[1])
}

/// `rectangle`/`diamond`/`ellipse`: the field set of `crates/scene/src/element.rs`'s test
/// `rectangle()` helper, with `id`, `type`, `x`, `y`, `width`, `height` as parameters.
pub fn generic(kind: &str, id: &str, rect: [f64; 4]) -> Value {
    let [x, y, width, height] = rect;
    json!({
        "id": id, "type": kind, "x": x, "y": y, "width": width, "height": height, "angle": 0,
        "strokeColor": "#1e1e1e", "backgroundColor": "transparent", "fillStyle": "solid",
        "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100,
        "groupIds": [], "frameId": null, "index": "a0", "roundness": null, "seed": 1,
        "version": 3, "versionNonce": 4, "isDeleted": false, "boundElements": null,
        "updated": 5, "link": null, "locked": false, "customData": {"k": [1, 2]}
    })
}

/// `line`/`arrow`: the field set of the same-type element in `shapes.excalidraw` (line) or
/// `bindings.excalidraw` (arrow). `width`/`height` are `points`'s bounding box.
pub fn linear(kind: &str, id: &str, origin: [f64; 2], points: &[[f64; 2]]) -> Value {
    let [x, y] = origin;
    let (width, height) = points_extent(points);
    let mut value = json!({
        "id": id, "type": kind, "x": x, "y": y, "width": width, "height": height, "angle": 0,
        "strokeColor": "#1e1e1e", "backgroundColor": "transparent", "fillStyle": "solid",
        "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100,
        "groupIds": [], "frameId": null, "index": "a0", "roundness": {"type": 2}, "seed": 1,
        "version": 1, "versionNonce": 1, "isDeleted": false, "boundElements": null,
        "updated": 1, "created": 1, "link": null, "locked": false,
        "points": points, "startBinding": null, "endBinding": null,
        "startArrowhead": null, "endArrowhead": null
    });
    // `polygon` is line-only and `elbowed` is arrow-only (`shapes.excalidraw`'s `line` has no
    // `elbowed` key, `bindings.excalidraw`'s `arrow` has no `polygon` key).
    if kind == "arrow" {
        value["elbowed"] = json!(false);
    } else {
        value["polygon"] = json!(false);
    }
    value
}

/// `freedraw`: the field set of `shapes.excalidraw`'s `freedraw` element. `width`/`height`
/// are `points`'s bounding box.
pub fn freedraw(id: &str, origin: [f64; 2], points: &[[f64; 2]]) -> Value {
    let [x, y] = origin;
    let (width, height) = points_extent(points);
    json!({
        "id": id, "type": "freedraw", "x": x, "y": y, "width": width, "height": height,
        "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
        "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
        "opacity": 100, "groupIds": [], "frameId": null, "index": "a0", "roundness": null,
        "seed": 1, "version": 1, "versionNonce": 1, "isDeleted": false, "boundElements": null,
        "updated": 1, "created": 1, "link": null, "locked": false,
        "points": points, "pressures": [], "simulatePressure": true,
        "strokeOptions": {"variability": "constant", "streamline": 0.5}
    })
}

/// `text`: the field set of `shapes.excalidraw`'s `text` element.
pub fn text(id: &str, rect: [f64; 4], text: &str, container: Option<&str>) -> Value {
    let [x, y, width, height] = rect;
    json!({
        "id": id, "type": "text", "x": x, "y": y, "width": width, "height": height,
        "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent",
        "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1,
        "opacity": 100, "groupIds": [], "frameId": null, "index": "a0", "roundness": null,
        "seed": 1, "version": 1, "versionNonce": 1, "isDeleted": false, "boundElements": null,
        "updated": 1, "created": 1, "link": null, "locked": false,
        "text": text, "fontSize": 20, "baseFontSize": 20, "fontFamily": 5,
        "textAlign": "left", "verticalAlign": "top", "containerId": container,
        "originalText": text, "autoResize": true, "lineHeight": 1.25, "labelPosition": null
    })
}

/// Shallow merge: every key of `overrides` replaces the same key of `value`.
pub fn with(value: Value, overrides: Value) -> Value {
    let mut value = value;
    if let (Some(target), Value::Object(overrides)) = (value.as_object_mut(), overrides) {
        for (key, v) in overrides {
            target.insert(key, v);
        }
    }
    value
}

/// A scene file holding `elements`, parsed the same way [`crate::SceneFile::from_json_str`]
/// would.
pub fn file(elements: Vec<Value>) -> crate::SceneFile {
    let mut file = crate::SceneFile::new();
    file.elements = elements
        .into_iter()
        .map(crate::Element::from_value)
        .collect();
    file
}

#[cfg(test)]
mod tests {
    use crate::Element;

    use super::*;

    #[test]
    fn samples_load_as_typed_elements() {
        let values = [
            generic("rectangle", "r", [0.0, 0.0, 10.0, 10.0]),
            generic("diamond", "d", [0.0, 0.0, 10.0, 10.0]),
            generic("ellipse", "e", [0.0, 0.0, 10.0, 10.0]),
            linear("line", "l", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0]]),
            linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [10.0, 5.0]]),
            freedraw("f", [0.0, 0.0], &[[0.0, 0.0], [3.0, 4.0], [6.0, 1.0]]),
            text("t", [0.0, 0.0, 40.0, 25.0], "hi", None),
        ];
        for value in values {
            let element = Element::from_value(value.clone());
            assert!(!matches!(element, Element::Raw(_)), "{value}");
        }
        let file = file(vec![generic("rectangle", "r", [0.0, 0.0, 1.0, 1.0])]);
        assert_eq!(file.elements.len(), 1);
    }
}
