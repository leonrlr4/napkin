mod support;

use scene::editor::{Command, Cursor, Tool};
use scene::sample;
use serde_json::{Value, json};
use support::*;

fn solid(id: &str, r: [f64; 4]) -> Value {
    sample::with(
        sample::generic("rectangle", id, r),
        json!({"backgroundColor": "#ffc9c9"}),
    )
}

#[test]
fn click_selects_the_hit_element_and_empty_space_clears() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 100.0, 100.0]),
        solid("b", [200.0, 0.0, 100.0, 100.0]),
    ]);
    click(&mut e, at(50.0, 50.0));
    assert_eq!(selected(&e), ["a"]);
    click(&mut e, at(250.0, 50.0));
    assert_eq!(selected(&e), ["b"]);
    click(&mut e, at(500.0, 500.0));
    assert!(selected(&e).is_empty());
    assert_eq!(e.revision(), 0, "selection changes are not edits");
}

#[test]
fn shift_click_adds_and_removes_on_release() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 100.0, 100.0]),
        solid("b", [200.0, 0.0, 100.0, 100.0]),
    ]);
    click(&mut e, at(50.0, 50.0));
    click(&mut e, shift(250.0, 50.0));
    assert_eq!(selected(&e), ["a", "b"]);
    e.pointer_down(shift(50.0, 50.0));
    assert_eq!(selected(&e), ["a", "b"], "removal waits for pointer up");
    e.pointer_up(shift(50.0, 50.0));
    assert_eq!(selected(&e), ["b"]);
}

#[test]
fn clicking_one_of_several_selected_elements_narrows_the_selection() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 100.0, 100.0]),
        solid("b", [200.0, 0.0, 100.0, 100.0]),
    ]);
    assert!(e.command(Command::SelectAll));
    click(&mut e, at(50.0, 50.0));
    assert_eq!(selected(&e), ["a"]);
}

#[test]
fn clicking_a_group_member_selects_the_group() {
    let grouped = |id, r| sample::with(solid(id, r), json!({"groupIds": ["g"]}));
    let mut e = editor(vec![
        grouped("a", [0.0, 0.0, 50.0, 50.0]),
        grouped("b", [100.0, 0.0, 50.0, 50.0]),
    ]);
    click(&mut e, at(25.0, 25.0));
    assert_eq!(selected(&e), ["a", "b"]);
}

#[test]
fn box_selection_updates_while_dragging() {
    let mut e = editor(vec![
        solid("a", [10.0, 10.0, 50.0, 50.0]),
        solid("b", [100.0, 100.0, 50.0, 50.0]),
    ]);
    e.pointer_down(at(0.0, 0.0));
    e.pointer_move(at(70.0, 70.0));
    assert_eq!(selected(&e), ["a"]);
    assert_eq!(e.overlay(1.0).box_selection, Some([0.0, 0.0, 70.0, 70.0]));
    e.pointer_move(at(200.0, 200.0));
    assert_eq!(selected(&e), ["a", "b"]);
    e.pointer_up(at(200.0, 200.0));
    assert_eq!(selected(&e), ["a", "b"]);
    assert_eq!(e.overlay(1.0).box_selection, None);
    assert_eq!(e.revision(), 0);
}

#[test]
fn dragging_moves_bound_text_and_undoes_in_one_step() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some("r")),
    ]);
    drag(&mut e, at(50.0, 50.0), [80.0, 70.0]);
    assert_eq!(selected(&e), ["r"]);
    assert_eq!(rect_of(&e, "r")[..2], [30.0, 20.0]);
    assert_eq!(rect_of(&e, "t")[..2], [60.0, 60.0]);
    let moved = e.file().as_ref().clone();

    assert!(e.command(Command::Undo));
    assert_eq!(rect_of(&e, "r")[..2], [0.0, 0.0]);
    assert_eq!(rect_of(&e, "t")[..2], [30.0, 40.0]);
    assert!(selected(&e).is_empty());
    assert!(e.command(Command::Redo));
    assert_eq!(*e.file().as_ref(), moved);
    assert_eq!(selected(&e), ["r"]);
    assert!(!e.command(Command::Redo));
}

#[test]
fn shift_drag_locks_to_the_longer_axis() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 100.0])]);
    drag(&mut e, shift(50.0, 50.0), [90.0, 60.0]);
    assert_eq!(rect_of(&e, "a")[..2], [40.0, 0.0]);
}

#[test]
fn dragging_inside_the_box_of_a_multi_selection_moves_everything() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 50.0, 50.0]),
        solid("b", [100.0, 100.0, 50.0, 50.0]),
    ]);
    e.command(Command::SelectAll);
    drag(&mut e, at(75.0, 75.0), [85.0, 85.0]);
    assert_eq!(rect_of(&e, "a")[..2], [10.0, 10.0]);
    assert_eq!(rect_of(&e, "b")[..2], [110.0, 110.0]);
}

#[test]
fn corner_handle_resizes_and_shows_a_resize_cursor() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 100.0, 50.0])]);
    click(&mut e, at(50.0, 25.0));
    e.pointer_move(at(104.0, 54.0));
    assert_eq!(e.cursor(), Cursor::ResizeNwse);
    // Grabbed 4 units past the corner: the offset is kept, so the corner ends at (150, 100).
    drag(&mut e, at(104.0, 54.0), [154.0, 104.0]);
    assert_eq!(rect_of(&e, "a"), [0.0, 0.0, 150.0, 100.0]);
    let overlay = e.overlay(1.0);
    assert_eq!(overlay.handles.len(), 4);
    assert_eq!(
        overlay.outlines,
        vec![[[-4.0, -4.0], [154.0, -4.0], [154.0, 104.0], [-4.0, 104.0]]]
    );
    assert_eq!(overlay.selection_box, None);
}

#[test]
fn a_selected_two_point_arrow_shows_its_points_and_drags_them() {
    let arrow = sample::with(
        sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [100.0, 0.0]]),
        json!({"roughness": 0, "roundness": null}),
    );
    let mut e = editor(vec![arrow]);
    click(&mut e, at(50.0, 0.0));
    let overlay = e.overlay(1.0);
    assert!(overlay.handles.is_empty() && overlay.outlines.is_empty());
    assert_eq!(overlay.points, vec![[0.0, 0.0], [100.0, 0.0]]);
    drag(&mut e, at(103.0, 2.0), [153.0, 52.0]);
    assert_eq!(
        element(&e, "a").to_value()["points"],
        json!([[0.0, 0.0], [150.0, 50.0]])
    );
    assert_eq!(selected(&e), ["a"]);
}

#[test]
fn select_all_then_delete_is_one_undo_step() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 10.0, 10.0]),
        sample::with(
            sample::generic("rectangle", "c", [50.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [80.0, 40.0, 40.0, 20.0], "hi", Some("c")),
    ]);
    let before = e.file().as_ref().clone();
    assert!(!e.command(Command::Delete), "nothing selected");
    assert!(e.command(Command::SelectAll));
    assert_eq!(selected(&e), ["a", "c"]);
    assert!(e.command(Command::Delete));
    assert!(e.file().elements.iter().all(|el| el.is_deleted()));
    assert!(selected(&e).is_empty());
    assert!(e.command(Command::Undo));
    assert_eq!(*e.file().as_ref(), before);
    assert_eq!(selected(&e), ["a", "c"]);
}

#[test]
fn escape_clears_the_selection_then_has_nothing_to_cancel() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    click(&mut e, at(5.0, 5.0));
    assert!(e.command(Command::Escape));
    assert!(selected(&e).is_empty());
    assert!(!e.command(Command::Escape));
}

#[test]
fn undo_waits_for_the_gesture_and_revision_counts_edits() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    drag(&mut e, at(5.0, 5.0), [15.0, 5.0]);
    let after_first = e.revision();
    assert!(after_first > 0);
    e.pointer_down(at(15.0, 5.0));
    e.pointer_move(at(25.0, 5.0));
    assert!(!e.is_idle());
    assert!(!e.command(Command::Undo));
    e.pointer_up(at(25.0, 5.0));
    assert!(e.is_idle());
    assert!(e.command(Command::Undo));
    assert_eq!(rect_of(&e, "a")[..2], [10.0, 0.0]);
    assert!(e.revision() > after_first);
}

#[test]
fn a_drag_copies_the_scene_once() {
    let mut e = editor(vec![solid("a", [0.0, 0.0, 10.0, 10.0])]);
    drag(&mut e, at(5.0, 5.0), [45.0, 5.0]);
    assert_eq!(
        e.scene_clones(),
        1,
        "only the first move after the undo snapshot copies"
    );
}

#[test]
fn hand_tool_ignores_the_pointer_and_multi_selection_has_a_dashed_box() {
    let mut e = editor(vec![
        solid("a", [0.0, 0.0, 100.0, 50.0]),
        solid("b", [200.0, 0.0, 50.0, 50.0]),
    ]);
    e.set_tool(Tool::Hand);
    drag(&mut e, at(50.0, 25.0), [80.0, 25.0]);
    assert_eq!(rect_of(&e, "a")[..2], [0.0, 0.0]);
    e.set_tool(Tool::Selection);
    e.command(Command::SelectAll);
    let overlay = e.overlay(1.0);
    assert_eq!(overlay.outlines.len(), 2);
    assert_eq!(overlay.selection_box, Some([-4.0, -4.0, 254.0, 54.0]));
    assert_eq!(overlay.handles.len(), 4);
}

#[test]
fn rotated_elements_move_but_have_no_handles() {
    let rotated = sample::with(solid("r", [0.0, 0.0, 100.0, 20.0]), json!({"angle": 0.5}));
    let mut e = editor(vec![rotated]);
    click(&mut e, at(50.0, 10.0));
    assert_eq!(selected(&e), ["r"]);
    assert!(e.overlay(1.0).handles.is_empty());
    drag(&mut e, at(50.0, 10.0), [60.0, 10.0]);
    assert_eq!(rect_of(&e, "r")[..2], [10.0, 0.0]);
}

#[test]
fn elbow_arrows_move_as_a_whole_without_point_or_resize_handles() {
    let elbow = sample::with(
        sample::linear(
            "arrow",
            "e",
            [0.0, 0.0],
            &[[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]],
        ),
        json!({"elbowed": true, "roughness": 0, "roundness": null}),
    );
    let mut e = editor(vec![elbow]);
    click(&mut e, at(50.0, 0.0));
    assert_eq!(selected(&e), ["e"]);
    let overlay = e.overlay(1.0);
    assert!(overlay.points.is_empty() && overlay.handles.is_empty());
    drag(&mut e, at(100.0, 50.0), [110.0, 60.0]);
    assert_eq!(rect_of(&e, "e")[..2], [10.0, 10.0]);
    assert_eq!(
        element(&e, "e").to_value()["points"],
        json!([[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]])
    );
}
