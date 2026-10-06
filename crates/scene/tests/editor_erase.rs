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

/// Erasing a frame deletes its children outright (`eraseElements`'s flat `isDeleted` condition),
/// unlike `Command::Delete`'s `actionDeleteSelected`, which instead releases them from the frame.
#[test]
fn erasing_a_frame_deletes_its_children_and_one_undo_restores_all() {
    let mut e = editor(vec![
        json!({"id": "f", "type": "frame", "x": 0, "y": 0, "width": 200, "height": 200,
               "angle": 0, "isDeleted": false, "version": 1, "versionNonce": 1}),
        sample::with(
            sample::generic("rectangle", "c1", [10.0, 10.0, 30.0, 30.0]),
            json!({"frameId": "f"}),
        ),
        sample::with(
            sample::generic("rectangle", "c2", [60.0, 60.0, 30.0, 30.0]),
            json!({"frameId": "f", "boundElements": [{"id": "lbl", "type": "text"}]}),
        ),
        sample::with(
            sample::text("lbl", [65.0, 70.0, 20.0, 10.0], "hi", Some("c2")),
            json!({"frameId": "f"}),
        ),
    ]);
    e.set_tool(Tool::Eraser);
    // On the frame's own left edge: nowhere near any child, so only "f" is hit directly.
    e.pointer_down(at(0.0, 100.0));
    assert_eq!(e.pending_erasure().len(), 1);
    assert!(e.pending_erasure().contains("f"));
    e.pointer_up(at(0.0, 100.0));

    for id in ["f", "c1", "c2", "lbl"] {
        assert!(element(&e, id).is_deleted(), "{id} should be deleted");
    }
    assert!(e.command(Command::Undo));
    for id in ["f", "c1", "c2", "lbl"] {
        assert!(!element(&e, id).is_deleted(), "{id} should be restored");
    }
}

/// `updateElementsToBeErased`'s group expansion adds bare ids only: a group member's own bound
/// text or container is examined only for the element the stroke actually hit, not for the rest
/// of its group.
#[test]
fn a_grouped_labels_container_is_untouched_unless_the_container_itself_is_hit() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "s", [0.0, 0.0, 50.0, 50.0]),
            json!({"groupIds": ["g1"]}),
        ),
        sample::with(
            sample::generic("rectangle", "c", [200.0, 200.0, 50.0, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [210.0, 210.0, 20.0, 10.0], "hi", Some("c")),
            json!({"groupIds": ["g1"]}),
        ),
    ]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(60.0, 25.0));
    assert!(e.pending_erasure().contains("s"));
    assert!(e.pending_erasure().contains("t"));
    assert!(!e.pending_erasure().contains("c"));
    e.pointer_up(at(60.0, 25.0));

    assert!(element(&e, "s").is_deleted());
    assert!(element(&e, "t").is_deleted());
    assert!(!element(&e, "c").is_deleted());
    // The now-deleted label's dangling reference is cleaned up even though "c" itself survives.
    assert_eq!(element(&e, "c").bound_elements(), vec![]);
}

#[test]
fn a_stroke_crosses_an_ellipse_outline() {
    let mut e = editor(vec![sample::generic(
        "ellipse",
        "e",
        [0.0, 0.0, 100.0, 50.0],
    )]);
    e.set_tool(Tool::Eraser);
    e.pointer_down(at(-10.0, 25.0));
    e.pointer_move(at(110.0, 25.0));
    assert!(e.pending_erasure().contains("e"));
    e.pointer_up(at(110.0, 25.0));
    assert!(element(&e, "e").is_deleted());
}
