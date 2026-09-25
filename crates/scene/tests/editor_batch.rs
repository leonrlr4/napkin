mod support;

use scene::editor::{Command, Tool};
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
fn refuses_while_a_gesture_is_in_progress() {
    let mut editor = editor(vec![]);
    editor.set_tool(Tool::Rectangle);
    editor.pointer_down(at(0.0, 0.0));
    let errors = editor
        .apply_batch(
            &json!({"ops": [{"op": "add", "type": "text", "x": 0, "y": 0, "text": "hi"}]}),
            &mut CharWidthMeasure,
        )
        .unwrap_err();
    assert!(errors[0].message.contains("gesture"), "{errors:?}");
}
