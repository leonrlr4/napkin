mod support;

use scene::editor::{Command, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::json;
use support::*;

#[test]
fn text_tool_creates_a_measured_text_element_in_one_step() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.pointer_down(at(100.0, 100.0));
    e.pointer_up(at(100.0, 100.0));
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
    e.pointer_down(at(0.0, 0.0));
    e.pointer_up(at(0.0, 0.0));
    assert!(!e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));
}

#[test]
fn double_click_edits_existing_text_and_empty_deletes_it() {
    let mut e = editor(vec![sample::text("t", [0.0, 0.0, 36.0, 25.0], "old", None)]);
    assert!(e.double_click(at(10.0, 10.0)));
    assert_eq!(e.text_editing().unwrap().element_id.as_deref(), Some("t"));
    assert_eq!(e.text_editing().unwrap().text, "old");
    assert!(e.commit_text("longer", &mut CharWidthMeasure));
    assert_eq!(e.file().elements[0].to_value()["width"], json!(6.0 * 12.0));
    assert!(e.double_click(at(10.0, 10.0)));
    assert!(e.commit_text("", &mut CharWidthMeasure));
    assert!(e.file().elements[0].is_deleted());
}

#[test]
fn double_click_on_a_container_adds_a_centered_label() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 200.0, 100.0],
    )]);
    assert!(e.double_click(at(100.0, 50.0)));
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
    assert!(e.double_click(at(40.0, 40.0)));
    assert_eq!(e.text_editing().unwrap().element_id, None);
    assert_eq!(e.text_editing().unwrap().container_id, None);
}
