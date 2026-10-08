mod support;

use scene::editor::{Command, Property, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;

use support::{at, editor};

#[test]
fn a_batch_is_one_undo_step_and_keeps_the_selection() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 50.0, 50.0],
    )]);
    editor.command(Command::SelectAll);
    let before = editor.file().as_ref().clone();
    let revision = editor.revision();
    let report = editor
        .apply_batch(
            &json!({"ops": [
                {"op": "add", "type": "rectangle", "id": "a", "x": 100, "y": 0, "width": 50, "height": 50},
                {"op": "add", "type": "arrow", "x": 55, "y": 25, "points": [[0, 0], [40, 0]],
                 "start": {"id": "r"}, "end": {"id": "a"}},
            ]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    assert_eq!(report.added.len(), 2);
    assert_eq!(editor.revision(), revision + 1);
    assert!(editor.selection().contains("r"));
    assert!(editor.command(Command::Undo));
    assert_eq!(editor.file().as_ref(), &before);
    assert!(editor.command(Command::Redo));
    assert_eq!(editor.file().elements.len(), 3);
}

#[test]
fn a_failed_batch_changes_nothing_and_records_nothing() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 50.0, 50.0],
    )]);
    let before = editor.file().clone();
    let errors = editor
        .apply_batch(
            &json!({"ops": [{"op": "delete", "ids": ["missing"]}]}),
            &mut CharWidthMeasure,
        )
        .unwrap_err();
    assert_eq!(errors[0].op, Some(0));
    assert_eq!(editor.file(), &before);
    assert!(!editor.command(Command::Undo));
}

#[test]
fn deleting_a_selected_element_drops_it_from_the_selection() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 50.0, 50.0],
    )]);
    editor.command(Command::SelectAll);
    editor
        .apply_batch(
            &json!({"ops": [{"op": "delete", "ids": ["r"]}]}),
            &mut CharWidthMeasure,
        )
        .unwrap();
    assert!(editor.selection().is_empty());
}

#[test]
fn a_batch_uses_the_default_style_not_the_panels_current_picks() {
    let mut editor = editor(vec![]);
    editor.set_property(
        Property::StrokeColor("#ff0000".into()),
        &mut CharWidthMeasure,
    );
    editor.set_property(Property::Roughness(0.0), &mut CharWidthMeasure);

    let report = editor
        .apply_batch(
            &json!({"ops": [
                {"op": "add", "type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 50, "height": 50},
            ]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    assert_eq!(report.added.len(), 1);
    let value = editor.file().elements[0].to_value();
    assert_eq!(
        value["strokeColor"],
        json!("#1e1e1e"),
        "must use ItemStyle::default, not the panel's #ff0000"
    );
    assert_eq!(
        value["roughness"],
        json!(1.0),
        "must use ItemStyle::default, not the panel's 0"
    );
}

#[test]
fn refuses_while_a_gesture_is_in_progress() {
    let mut editor = editor(vec![]);
    editor.set_tool(Tool::Rectangle);
    editor.pointer_down(at(0.0, 0.0), &mut CharWidthMeasure);
    let errors = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "text", "x": 0, "y": 0, "text": "hi"}]}),
            &mut CharWidthMeasure,
        )
        .unwrap_err();
    assert!(errors[0].message.contains("gesture"), "{errors:?}");
}

#[test]
fn a_long_label_wraps_and_its_box_grows() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 100, "height": 50, "label": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = &report.created["r"];
    let container = editor
        .file()
        .elements
        .iter()
        .find(|e| e.id() == Some(id))
        .unwrap();
    assert_eq!(container.placement().unwrap().height, 60.0);
    let label = editor.file().elements[1].to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(label["originalText"], json!("hello world"));
}

#[test]
fn narrowing_a_box_by_update_rewraps_its_label() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 300, "height": 50, "label": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = report.created["r"].clone();
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": id, "set": {"width": 100}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    assert_eq!(
        editor.file().elements[1].to_value()["text"],
        json!("hello\nworld")
    );
    assert_eq!(editor.file().elements[0].placement().unwrap().height, 60.0);
}

#[test]
fn moving_a_labeled_box_only_repositions_its_label() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 300, "height": 50, "label": {"text": "hello"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = report.created["r"].clone();
    let before = editor.file().elements[1].to_value();
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": id, "set": {"x": 40, "y": 10}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    let after = editor.file().elements[1].to_value();
    assert_eq!(after["text"], before["text"]);
    assert_eq!(after["width"], before["width"]);
    assert_eq!(after["height"], before["height"]);
    assert_eq!(
        after["x"].as_f64().unwrap(),
        before["x"].as_f64().unwrap() + 40.0
    );
    assert_eq!(
        after["y"].as_f64().unwrap(),
        before["y"].as_f64().unwrap() + 10.0
    );
    assert_eq!(editor.file().elements[0].placement().unwrap().height, 50.0);
}

#[test]
fn setting_a_containers_text_to_something_longer_grows_it() {
    let mut editor = editor(vec![]);
    let report = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "rectangle", "id": "r", "x": 0, "y": 0,
                "width": 100, "height": 50, "label": {"text": "hi"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid batch");
    let id = report.created["r"].clone();
    assert_eq!(editor.file().elements[0].placement().unwrap().height, 50.0);
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": id, "set": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    let label = editor.file().elements[1].to_value();
    assert_eq!(label["text"], json!("hello\nworld"));
    assert_eq!(editor.file().elements[0].placement().unwrap().height, 60.0);
}

#[test]
fn setting_text_on_a_fixed_width_text_keeps_its_width_and_rewraps() {
    let mut editor = editor(vec![sample::with(
        sample::text("t", [0.0, 0.0, 100.0, 25.0], "hi", None),
        json!({"autoResize": false}),
    )]);
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": "t", "set": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    let v = editor.file().elements[0].to_value();
    assert_eq!(v["width"], json!(100.0));
    assert_eq!(v["text"], json!("hello\nworld"));
    assert_eq!(v["originalText"], json!("hello world"));
    assert_eq!(v["height"], json!(50.0));
    assert_eq!(v["version"], json!(2.0));
}

#[test]
fn setting_text_on_a_rotated_text_keeps_its_left_edge_in_the_rotated_frame() {
    let mut editor = editor(vec![sample::with(
        sample::text("t", [0.0, 0.0, 24.0, 25.0], "hi", None),
        json!({"angle": std::f64::consts::FRAC_PI_2}),
    )]);
    editor
        .apply_batch(
            &json!({"ops": [{"op": "update", "id": "t", "set": {"text": "hello world"}}]}),
            &mut CharWidthMeasure,
        )
        .expect("valid update");
    let v = editor.file().elements[0].to_value();
    assert_eq!(v["width"], json!(132.0));
    assert!((v["x"].as_f64().unwrap() + 54.0).abs() < 1e-9, "{v}");
    assert!((v["y"].as_f64().unwrap() - 54.0).abs() < 1e-9, "{v}");
}
