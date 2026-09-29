mod support;

use scene::editor::{Command, Tool};
use scene::sample;
use serde_json::json;
use support::*;

#[test]
fn a_stroke_across_two_shapes_erases_both_in_one_step() {
    let mut e = editor(vec![
        sample::generic("rectangle", "a", [0.0, 0.0, 50.0, 50.0]),
        sample::generic("rectangle", "b", [100.0, 0.0, 50.0, 50.0]),
        sample::generic("rectangle", "keep", [0.0, 200.0, 50.0, 50.0]),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(60.0, 25.0));
    assert!(e.pending_erasure().contains("a"));
    e.pointer_move(at(160.0, 25.0));
    assert_eq!(e.pending_erasure().len(), 2);
    e.pointer_up(at(160.0, 25.0));
    assert!(e.pending_erasure().is_empty());
    let deleted: Vec<bool> = e.file().elements.iter().map(|x| x.is_deleted()).collect();
    assert_eq!(deleted, vec![true, true, false]);
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.iter().all(|x| !x.is_deleted()));
}

#[test]
fn a_click_erases_the_element_under_it_with_its_label() {
    let mut e = editor(vec![
        sample::with(
            sample::with(
                sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
                json!({"backgroundColor": "#a5d8ff"}),
            ),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [30.0, 12.5, 40.0, 25.0], "hi", Some("r")),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(50.0, 25.0));
    e.pointer_up(at(50.0, 25.0));
    assert!(e.file().elements.iter().all(|x| x.is_deleted()));
}

#[test]
fn escape_abandons_the_stroke_and_locked_elements_survive() {
    let mut e = editor(vec![
        sample::generic("rectangle", "a", [0.0, 0.0, 50.0, 50.0]),
        sample::with(
            sample::generic("rectangle", "l", [100.0, 0.0, 50.0, 50.0]),
            json!({"locked": true}),
        ),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(160.0, 25.0));
    assert!(!e.pending_erasure().contains("l"));
    assert!(e.command(Command::Escape));
    e.pointer_up(at(160.0, 25.0));
    assert!(e.file().elements.iter().all(|x| !x.is_deleted()));
}
