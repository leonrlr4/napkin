mod support;

use scene::editor::{Command, Property, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;
use support::*;

#[test]
fn text_tool_creates_a_measured_text_element_in_one_step() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(100.0, 100.0), &mut CharWidthMeasure);
    e.pointer_up(at(100.0, 100.0), &mut CharWidthMeasure);
    let editing = e.text_editing().expect("editing").clone();
    assert_eq!(editing.element_id, None);
    assert_eq!(editing.font_size, 20.0);
    assert!(!e.is_idle());
    assert!(e.commit_text("hi\n中文", &mut CharWidthMeasure));
    assert!(e.text_editing().is_none());
    assert_eq!(e.tool(), Tool::Selection);
    let v = e.file().elements[0].to_value();
    assert_eq!(v["text"], json!("hi\n中文"));
    assert_eq!(v["height"], json!(50.0));
    assert!(e.selection().contains(v["id"].as_str().unwrap()));
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
}

#[test]
fn committing_empty_new_text_creates_nothing() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(0.0, 0.0), &mut CharWidthMeasure);
    e.pointer_up(at(0.0, 0.0), &mut CharWidthMeasure);
    assert!(!e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));
}

#[test]
fn double_click_edits_existing_text_and_empty_deletes_it() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 36.0, 25.0], "old", None)]);
    assert!(e.double_click(at(10.0, 10.0), &mut CharWidthMeasure));
    assert_eq!(e.text_editing().unwrap().element_id.as_deref(), Some("t"));
    assert_eq!(e.text_editing().unwrap().text, "old");
    assert!(e.commit_text("longer", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[0].to_value()["width"], json!(6.0 * 12.0));
    assert!(e.double_click(at(10.0, 10.0), &mut CharWidthMeasure));
    assert!(e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements[0].is_deleted());
}

#[test]
fn a_panel_change_during_an_edit_is_applied_and_remeasured_on_commit() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 36.0, 25.0], "old", None)]);
    assert!(e.double_click(at(10.0, 10.0), &mut CharWidthMeasure));

    e.set_property(Property::FontSize(40.0), &mut CharWidthMeasure);
    e.set_property(
        Property::StrokeColor("#ff0000".into()),
        &mut CharWidthMeasure,
    );
    e.set_property(Property::Opacity(50.0), &mut CharWidthMeasure);
    // The overlay already shows the change (it reads `TextEditing`, which `set_property` did
    // update); what this test guards is that the element itself picks it up too, on commit.
    let editing = e.text_editing().expect("still editing");
    assert_eq!(editing.font_size, 40.0);
    assert_eq!(editing.stroke_color, "#ff0000");
    assert_eq!(editing.opacity, 50.0);

    assert!(e.commit_text("old", &mut CharWidthMeasure));
    let v = e.file().elements[0].to_value();
    assert_eq!(v["fontSize"], json!(40.0));
    assert_eq!(v["strokeColor"], json!("#ff0000"));
    assert_eq!(v["opacity"], json!(50.0));
    assert_eq!(
        v["width"],
        json!(3.0 * 40.0 * 0.6),
        "re-measured at the new font size"
    );
}

#[test]
fn double_click_on_a_container_adds_a_centered_label() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 200.0, 100.0],
    )]);
    assert!(e.double_click(at(100.0, 50.0), &mut CharWidthMeasure));
    assert_eq!(e.text_editing().unwrap().container_id.as_deref(), Some("r"));
    assert!(e.commit_text("box", &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["containerId"], json!("r"));
    assert_eq!(label["x"], json!(5.0 + (190.0 / 2.0 - 36.0 / 2.0)));
    assert!(
        e.file().elements[0]
            .bound_elements()
            .contains(&(label["id"].as_str().unwrap(), "text"))
    );
}

#[test]
fn double_click_on_empty_canvas_starts_new_text_there() {
    let mut e = editor(vec![]);
    assert!(e.double_click(at(40.0, 40.0), &mut CharWidthMeasure));
    assert_eq!(e.text_editing().unwrap().element_id, None);
    assert_eq!(e.text_editing().unwrap().container_id, None);
}

#[test]
fn switching_to_another_tool_while_editing_does_not_lose_or_erase_the_text() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 36.0, 25.0], "old", None)]);
    assert!(e.double_click(at(10.0, 10.0), &mut CharWidthMeasure));

    // The eraser tool would otherwise delete "t" out from under this edit.
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(10.0, 10.0), &mut CharWidthMeasure);
    e.pointer_up(at(10.0, 10.0), &mut CharWidthMeasure);
    assert!(!e.file().elements[0].is_deleted());

    // A creation tool would otherwise start drawing a new shape underneath the editor.
    e.set_tool(Tool::Rectangle);
    e.pointer_down(at(50.0, 50.0), &mut CharWidthMeasure);
    e.pointer_up(at(80.0, 80.0), &mut CharWidthMeasure);
    assert_eq!(e.file().elements.len(), 1);

    assert!(e.text_editing().is_some());
    assert!(e.commit_text("still here", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[0].to_value()["text"], json!("still here"));
}

#[test]
fn text_tool_click_far_from_a_containers_center_creates_free_text_not_a_label() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 200.0, 100.0],
    )]);
    e.set_tool(Tool::Text);
    // (20, 20) is inside the rectangle's bounding box but about 85 units from its center
    // (100, 50), well past the 30-unit center-snap threshold.
    e.pointer_down(at(20.0, 20.0), &mut CharWidthMeasure);
    e.pointer_up(at(20.0, 20.0), &mut CharWidthMeasure);
    assert_eq!(e.text_editing().unwrap().container_id, None);
    assert!(e.commit_text("free", &mut CharWidthMeasure));
    assert_eq!(e.file().elements.len(), 2);
    assert!(e.file().elements[0].bound_elements().is_empty());
}

#[test]
fn text_tool_click_near_a_containers_center_binds_its_label() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 200.0, 100.0],
    )]);
    e.set_tool(Tool::Text);
    // (110, 60) is about 14 units from the center (100, 50), within the threshold.
    e.pointer_down(at(110.0, 60.0), &mut CharWidthMeasure);
    e.pointer_up(at(110.0, 60.0), &mut CharWidthMeasure);
    assert_eq!(e.text_editing().unwrap().container_id.as_deref(), Some("r"));
}

#[test]
fn double_click_near_a_transparent_containers_edge_creates_free_text_inheriting_its_group() {
    let mut e = editor(vec![sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 200.0, 100.0]),
        json!({"groupIds": ["g1"]}),
    )]);
    // The rectangle's background is transparent (the sample default) and (20, 20) neither
    // hits its outline nor falls within the center-snap threshold, so this must not force a
    // bind the way clicking its actual outline or an opaque fill would.
    assert!(e.double_click(at(20.0, 20.0), &mut CharWidthMeasure));
    let editing = e.text_editing().unwrap().clone();
    assert_eq!(editing.container_id, None);
    assert_eq!(editing.element_id, None);
    assert_eq!(editing.group_ids, vec!["g1".to_owned()]);
    assert!(e.commit_text("free", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[1].to_value()["groupIds"], json!(["g1"]));
}

#[test]
fn new_ui_label_takes_the_current_item_style_not_the_containers_stroke() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 200.0, 100.0],
    )]);
    let container_version_before = e.file().elements[0].version();
    // The container's own stroke color is the sample default, "#1e1e1e"; give the item style
    // a different color and opacity so the label's source is distinguishable.
    e.set_property(
        Property::StrokeColor("#ff0000".to_owned()),
        &mut CharWidthMeasure,
    );
    e.set_property(Property::Opacity(50.0), &mut CharWidthMeasure);
    assert!(e.double_click(at(100.0, 50.0), &mut CharWidthMeasure));
    assert!(e.commit_text("box", &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["strokeColor"], json!("#ff0000"));
    assert_eq!(label["opacity"], json!(50.0));
    // The container gained a `boundElements` entry, a real mutation that must bump its
    // version like any other (`bind_label` itself does not, see its own doc comment).
    assert!(e.file().elements[0].version() > container_version_before);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements[0].bound_elements().is_empty());
}

#[test]
fn font_size_set_while_editing_applies_to_the_committed_text() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(0.0, 0.0), &mut CharWidthMeasure);
    e.pointer_up(at(0.0, 0.0), &mut CharWidthMeasure);
    e.set_property(Property::FontSize(36.0), &mut CharWidthMeasure);
    assert_eq!(e.text_editing().unwrap().font_size, 36.0);
    assert!(e.commit_text("hi", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[0].to_value()["fontSize"], json!(36.0));
}

#[test]
fn a_new_label_wraps_inside_its_container_and_grows_it() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 100.0, 50.0],
    )]);
    assert!(e.double_click(at(50.0, 25.0), &mut CharWidthMeasure));
    assert_eq!(e.text_editing().unwrap().wrap_width, Some(90.0));
    assert!(e.commit_text("hello world", &mut CharWidthMeasure));
    let label = e.file().elements[1].to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(e.file().elements[0].placement().unwrap().height, 60.0);
    assert!(e.command(Command::Undo));
    assert_eq!(e.file().elements[0].placement().unwrap().height, 50.0);
}

#[test]
fn editing_a_wrapped_label_starts_from_its_original_text() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 100.0, 50.0],
    )]);
    e.double_click(at(50.0, 25.0), &mut CharWidthMeasure);
    e.commit_text("hello world", &mut CharWidthMeasure);
    assert!(e.double_click(at(50.0, 30.0), &mut CharWidthMeasure));
    assert_eq!(e.text_editing().unwrap().text, "hello world");
    assert_eq!(e.text_editing().unwrap().wrap_width, Some(90.0));
}

#[test]
fn a_tiny_container_grows_to_the_minimum_before_editing() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    let before = e.file().clone();
    assert!(e.double_click(at(5.0, 5.0), &mut CharWidthMeasure));
    // approx_min_container_size with CharWidthMeasure at fontSize 20: [22, 35].
    let p = e.file().elements[0].placement().unwrap();
    assert_eq!([p.width, p.height], [22.0, 35.0]);
    assert!(!e.commit_text("", &mut CharWidthMeasure));
    assert_eq!(e.file().elements, before.elements);
    assert!(!e.command(Command::Undo));
}

#[test]
fn growing_a_tiny_container_and_labelling_it_is_one_undo_step() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.double_click(at(5.0, 5.0), &mut CharWidthMeasure);
    assert!(e.commit_text("a", &mut CharWidthMeasure));
    assert!(e.command(Command::Undo));
    let p = e.file().elements[0].placement().unwrap();
    assert_eq!([p.width, p.height], [10.0, 10.0]);
    assert!(
        e.file()
            .elements
            .iter()
            .all(|x| x.is_deleted() || x.id() == Some("r"))
    );
}
