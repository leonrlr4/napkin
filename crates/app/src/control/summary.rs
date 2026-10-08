//! The `napkin scene` / `napkin selection` listing format: a `file` header line, then one
//! line per non-deleted element. A text element bound to a live container folds into that
//! container's `label=`; every other element gets its own line with the fields that apply
//! to it, in a fixed order.

use std::fmt::Write as _;

use scene::element::LinearEnd;
use scene::{Element, SceneFile};
use serde_json::Value;

/// A text element's `originalText` (what was typed, before automatic wrapping), or its `text`
/// when it has none.
fn original_or_text(value: &Value) -> Option<String> {
    value
        .get("originalText")
        .and_then(Value::as_str)
        .or_else(|| value.get("text").and_then(Value::as_str))
        .map(str::to_owned)
}

/// Two decimal places, trailing zeros and a trailing `.` trimmed, `-0` written as `0`.
pub fn format_number(value: f64) -> String {
    let mut text = format!("{value:.2}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_string();
    }
    text
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("a &str always serializes")
}

fn live_element_by_id<'a>(file: &'a SceneFile, id: &str) -> Option<&'a Element> {
    file.elements
        .iter()
        .find(|element| !element.is_deleted() && element.id() == Some(id))
}

/// `x y w h`, or `? ? ? ?` for a `Raw` element with no numeric placement.
fn base_fields(element: &Element) -> String {
    match element.placement() {
        Some(p) => format!(
            "{} {} {} {}",
            format_number(p.x),
            format_number(p.y),
            format_number(p.width),
            format_number(p.height)
        ),
        None => "? ? ? ?".to_string(),
    }
}

fn push_field(line: &mut String, name: &str, value: impl std::fmt::Display) {
    write!(line, " {name}={value}").expect("String writes are infallible");
}

/// Renders each point that is a two-element array as `[x,y]`; a malformed point (not an
/// array, or fewer than two elements) is dropped rather than panicking, since this reads a
/// `Raw` element's JSON, which napkin does not validate.
fn format_points(points: &[Value]) -> String {
    let parts: Vec<String> = points
        .iter()
        .filter_map(|point| {
            let coords = point.as_array()?;
            if coords.len() < 2 {
                return None;
            }
            Some(format!(
                "[{},{}]",
                format_number(coords[0].as_f64().unwrap_or(0.0)),
                format_number(coords[1].as_f64().unwrap_or(0.0))
            ))
        })
        .collect();
    format!("[{}]", parts.join(","))
}

/// One line for `element`: `<id> <type> <x> <y> <w> <h>`, then whichever of `label=`,
/// `text=`, `stroke=`, `bg=`, `start=`, `end=`, `points=`, `groups=`, `angle=` and `locked`
/// apply, in that order.
fn element_line(file: &SceneFile, element: &Element) -> String {
    let value = element.to_value();
    let mut line = format!(
        "{} {} {}",
        element.id().unwrap_or("?"),
        element.kind(),
        base_fields(element)
    );

    if let Some((text_id, _)) = element
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")
        && let Some(label) = live_element_by_id(file, text_id)
            .and_then(|text_element| original_or_text(&text_element.to_value()))
    {
        push_field(&mut line, "label", json_string(&label));
    }

    if element.kind() == "text"
        && let Some(text) = original_or_text(&value)
    {
        push_field(&mut line, "text", json_string(&text));
    }

    if let Some(stroke) = value.get("strokeColor").and_then(Value::as_str)
        && stroke != "#1e1e1e"
    {
        push_field(&mut line, "stroke", stroke);
    }

    if let Some(bg) = value.get("backgroundColor").and_then(Value::as_str)
        && bg != "transparent"
    {
        push_field(&mut line, "bg", bg);
    }

    if let Some(start) = element.binding_target(LinearEnd::Start) {
        push_field(&mut line, "start", start);
    }

    if let Some(end) = element.binding_target(LinearEnd::End) {
        push_field(&mut line, "end", end);
    }

    if matches!(element.kind(), "line" | "arrow")
        && let Some(points) = value.get("points").and_then(Value::as_array)
    {
        push_field(&mut line, "points", format_points(points));
    }

    let groups = element.group_ids();
    if !groups.is_empty() {
        push_field(&mut line, "groups", groups.join(","));
    }

    if let Some(placement) = element.placement()
        && placement.angle != 0.0
    {
        push_field(&mut line, "angle", format_number(placement.angle));
    }

    if element.is_locked() {
        line.push_str(" locked");
    }

    line
}

/// One compact line per non-deleted, non-folded element at `positions`.
pub fn element_lines(file: &SceneFile, positions: &[usize]) -> Vec<String> {
    positions
        .iter()
        .filter_map(|&position| {
            let element = &file.elements[position];
            if element.is_deleted() {
                return None;
            }
            if element.kind() == "text"
                && element
                    .container_id()
                    .is_some_and(|container_id| live_element_by_id(file, container_id).is_some())
            {
                return None;
            }
            Some(element_line(file, element))
        })
        .collect()
}

/// One compact-JSON line per non-deleted element at `positions`, no folding: the `--full`
/// listing.
pub fn full_lines(file: &SceneFile, positions: &[usize]) -> Vec<String> {
    positions
        .iter()
        .filter(|&&position| !file.elements[position].is_deleted())
        .map(|&position| {
            serde_json::to_string(&file.elements[position].to_value())
                .expect("element JSON serializes")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use scene::sample;
    use serde_json::json;

    use super::*;

    #[test]
    fn numbers_are_short() {
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(1.0 / 3.0), "0.33");
        assert_eq!(format_number(-0.001), "0");
    }

    #[test]
    fn one_line_per_element_with_labels_folded_in() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 160.0, 70.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}],
                       "backgroundColor": "#a5d8ff", "groupIds": ["g1"]}),
            ),
            sample::text("t", [50.0, 22.5, 60.0, 25.0], "Parser", Some("r")),
            sample::with(
                sample::linear("arrow", "a", [165.0, 35.0], &[[0.0, 0.0], [100.0, 0.0]]),
                json!({"startBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [1.03, 0.5001]},
                       "strokeColor": "#e03131"}),
            ),
            sample::text("free", [0.0, 100.0, 80.0, 25.0], "note\nline", None),
            sample::with(
                sample::generic("ellipse", "gone", [0.0, 0.0, 1.0, 1.0]),
                json!({"isDeleted": true}),
            ),
        ]);
        let positions: Vec<usize> = (0..file.elements.len()).collect();
        assert_eq!(
            element_lines(&file, &positions),
            vec![
                r##"r rectangle 0 0 160 70 label="Parser" bg=#a5d8ff groups=g1"##,
                r##"a arrow 165 35 100 0 stroke=#e03131 start=r points=[[0,0],[100,0]]"##,
                r##"free text 0 100 80 25 text="note\nline""##,
            ]
        );
    }

    #[test]
    fn a_wrapped_label_shows_its_original_text() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 60.0]),
                json!({"boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::with(
                sample::text("t", [20.0, 5.0, 60.0, 50.0], "hello\nworld", Some("r")),
                json!({"originalText": "hello world"}),
            ),
        ]);
        assert_eq!(
            element_lines(&file, &[0, 1]),
            vec![r##"r rectangle 0 0 100 60 label="hello world""##]
        );
    }

    #[test]
    fn format_points_skips_malformed_points_instead_of_panicking() {
        assert_eq!(
            format_points(&[json!([1, 2]), json!([3, 4])]),
            "[[1,2],[3,4]]"
        );
        // A point that is not a two-element array: not an array at all, and one with a
        // single element. Both are dropped instead of panicking or indexing out of bounds.
        assert_eq!(format_points(&[json!(1), json!([1, 2])]), "[[1,2]]");
        assert_eq!(format_points(&[json!([1])]), "[]");
    }

    #[test]
    fn a_raw_element_with_no_numeric_placement_shows_question_marks() {
        let file = sample::file(vec![json!({"id": "w", "type": "weird", "x": "a"})]);
        assert_eq!(element_lines(&file, &[0]), vec!["w weird ? ? ? ?"]);
    }

    #[test]
    fn angle_and_locked_and_end_binding_each_show_up() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 50.0, 50.0]),
                json!({"angle": 1.5, "locked": true}),
            ),
            sample::with(
                sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 50.0]]),
                json!({"endBinding": {"elementId": "r", "mode": "orbit", "fixedPoint": [0.5, 0.5]}}),
            ),
        ]);
        assert_eq!(
            element_lines(&file, &[0, 1]),
            vec![
                "r rectangle 0 0 50 50 angle=1.5 locked",
                "a arrow 0 0 50 50 end=r points=[[0,0],[50,50]]",
            ]
        );
    }

    #[test]
    fn text_bound_to_a_dead_container_gets_its_own_line() {
        let file = sample::file(vec![
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 50.0, 50.0]),
                json!({"isDeleted": true, "boundElements": [{"id": "t", "type": "text"}]}),
            ),
            sample::text("t", [0.0, 0.0, 40.0, 25.0], "orphan", Some("r")),
        ]);
        assert_eq!(
            element_lines(&file, &[0, 1]),
            vec![r##"t text 0 0 40 25 text="orphan""##]
        );
    }
}
