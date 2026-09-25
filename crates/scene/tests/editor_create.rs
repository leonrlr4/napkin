mod support;

use scene::editor::{Command, Editor, Tool};
use serde_json::{Value, json};
use support::*;

fn live(e: &Editor<TestEnv>) -> Vec<Value> {
    e.file()
        .elements
        .iter()
        .filter(|el| !el.is_deleted())
        .map(|el| el.to_value())
        .collect()
}

fn only(e: &Editor<TestEnv>) -> Value {
    let live = live(e);
    assert_eq!(live.len(), 1, "{live:?}");
    live[0].clone()
}

fn last(e: &Editor<TestEnv>) -> Value {
    e.file().elements.last().expect("an element").to_value()
}

fn xywh(v: &Value) -> [f64; 4] {
    ["x", "y", "width", "height"].map(|k| v[k].as_f64().expect(k))
}

#[test]
fn rectangle_drag_creates_a_selected_element_and_returns_to_selection() {
    let mut e = editor(vec![]);
    assert_eq!(e.style().stroke_width.value(false), 2.0);
    e.set_tool(Tool::Rectangle);
    drag(&mut e, at(10.0, 20.0), [110.0, 70.0]);
    assert_eq!(e.tool(), Tool::Selection);
    let v = only(&e);
    assert_eq!(v["type"], "rectangle");
    assert_eq!(xywh(&v), [10.0, 20.0, 100.0, 50.0]);
    assert_eq!(v["roundness"], json!({"type": 3.0}));
    assert_eq!(v["strokeWidth"], json!(2.0));
    assert_eq!(v["strokeColor"], "#1e1e1e");
    assert_eq!(v["backgroundColor"], "transparent");
    assert!(v["index"].is_string());
    assert_eq!(v["id"].as_str().map(str::len), Some(21));
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
}

#[test]
fn shift_makes_squares_alt_centers_and_up_left_drags_flip() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    drag(&mut e, shift(10.0, 10.0), [60.0, 30.0]);
    assert_eq!(xywh(&last(&e)), [10.0, 10.0, 50.0, 50.0]);

    e.set_tool(Tool::Ellipse);
    drag(&mut e, alt(100.0, 100.0), [120.0, 110.0]);
    assert_eq!(xywh(&last(&e)), [80.0, 90.0, 40.0, 20.0]);
    assert_eq!(last(&e)["roundness"], json!({"type": 2.0}));

    e.set_tool(Tool::Diamond);
    drag(&mut e, at(100.0, 100.0), [60.0, 70.0]);
    assert_eq!(xywh(&last(&e)), [60.0, 70.0, 40.0, 30.0]);
    assert_eq!(last(&e)["type"], "diamond");
}

#[test]
fn a_click_without_a_drag_creates_nothing() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    click(&mut e, at(10.0, 10.0));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));
}

#[test]
fn arrow_drag_creates_a_two_point_arrow_anchored_at_the_press() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    drag(&mut e, at(100.0, 100.0), [40.0, 160.0]);
    let v = only(&e);
    assert_eq!(v["type"], "arrow");
    assert_eq!(xywh(&v), [100.0, 100.0, 60.0, 60.0]);
    assert_eq!(v["points"], json!([[0.0, 0.0], [-60.0, 60.0]]));
    assert_eq!(v["endArrowhead"], "arrow");
    assert_eq!(v["startArrowhead"], Value::Null);
    assert_eq!(v["roundness"], json!({"type": 2.0}));
    assert_eq!(v["elbowed"], json!(false));
    assert_eq!(e.tool(), Tool::Selection);
    assert_eq!(selected(&e), [v["id"].as_str().unwrap()]);
}

#[test]
fn clicks_build_a_multi_point_line_that_enter_finishes() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    assert!(!e.is_idle());
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0));
    click(&mut e, at(100.0, 100.0));
    e.pointer_move(at(50.0, 150.0));
    assert!(e.command(Command::Finalize));
    assert!(e.is_idle());
    let v = only(&e);
    assert_eq!(
        v["points"],
        json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]])
    );
    assert_eq!(v["polygon"], json!(false));
    assert_eq!(e.tool(), Tool::Selection);
    assert!(e.command(Command::Undo));
    assert!(
        e.file().elements.is_empty(),
        "the whole line is one undo step"
    );
}

#[test]
fn finish_pending_gesture_commits_a_multi_point_line_without_finalize() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0));
    click(&mut e, at(100.0, 100.0));
    // The cursor-following point: written into `file` by `pointer_move`, but not yet
    // committed (no click confirmed it) and not yet reflected in `revision`.
    e.pointer_move(at(50.0, 150.0));
    assert!(!e.is_idle());
    let revision_before = e.revision();

    e.finish_pending_gesture();

    assert!(e.is_idle());
    let v = only(&e);
    assert_eq!(
        v["points"],
        json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]),
        "the follow point is dropped"
    );
    assert_eq!(e.revision(), revision_before + 1);
    assert!(e.command(Command::Undo));
    assert!(
        e.file().elements.is_empty(),
        "the whole line is one undo step"
    );
}

#[test]
fn clicking_back_on_the_start_closes_a_line_into_a_polygon() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0));
    click(&mut e, at(100.0, 100.0));
    e.pointer_move(at(2.0, 2.0));
    click(&mut e, at(2.0, 2.0));
    assert!(e.is_idle());
    let v = only(&e);
    assert_eq!(
        v["points"],
        json!([[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 0.0]])
    );
    assert_eq!(v["polygon"], json!(true));
}

#[test]
fn freedraw_records_relative_points_and_keeps_the_tool() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Freedraw);
    e.pointer_down(at(10.0, 10.0));
    e.pointer_move(at(15.0, 12.0));
    e.pointer_move(at(15.0, 12.0));
    e.pointer_move(at(5.0, 20.0));
    e.pointer_up(at(8.0, 25.0));
    let v = only(&e);
    assert_eq!(
        v["points"],
        json!([[0.0, 0.0], [5.0, 2.0], [-5.0, 10.0], [-2.0, 15.0]])
    );
    assert_eq!(xywh(&v), [10.0, 10.0, 10.0, 15.0]);
    assert_eq!(v["simulatePressure"], json!(true));
    assert_eq!(v["pressures"], json!([]));
    assert_eq!(v["strokeWidth"], json!(1.0));
    assert_eq!(
        v["strokeOptions"],
        json!({"variability": "constant", "streamline": 0.5})
    );
    assert_eq!(v["roundness"], Value::Null);
    assert_eq!(e.tool(), Tool::Freedraw);
    assert!(selected(&e).is_empty());

    click(&mut e, at(50.0, 50.0));
    assert_eq!(last(&e)["points"], json!([[0.0, 0.0], [0.0001, 0.0001]]));
}

#[test]
fn escape_discards_a_dragged_shape_but_finishes_a_multi_point_arrow() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    e.pointer_down(at(0.0, 0.0));
    e.pointer_move(at(50.0, 50.0));
    assert!(e.command(Command::Escape));
    e.pointer_up(at(50.0, 50.0));
    assert!(e.file().elements.is_empty());
    assert!(!e.command(Command::Undo));

    e.set_tool(Tool::Arrow);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0));
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 80.0));
    assert!(e.command(Command::Escape));
    assert_eq!(only(&e)["points"], json!([[0.0, 0.0], [100.0, 0.0]]));
}

#[test]
fn switching_tools_finishes_a_multi_point_line() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(60.0, 0.0));
    click(&mut e, at(60.0, 0.0));
    e.pointer_move(at(60.0, 60.0));
    e.set_tool(Tool::Rectangle);
    assert!(e.is_idle());
    assert_eq!(e.tool(), Tool::Rectangle);
    assert_eq!(only(&e)["points"], json!([[0.0, 0.0], [60.0, 0.0]]));
    assert!(selected(&e).is_empty());
}

#[test]
fn a_tiny_arrow_is_removed_and_creation_undoes_in_one_step() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Arrow);
    click(&mut e, at(10.0, 10.0));
    assert!(e.command(Command::Finalize));
    assert!(e.file().elements.is_empty());

    e.set_tool(Tool::Ellipse);
    drag(&mut e, at(0.0, 0.0), [30.0, 30.0]);
    let created = e.file().as_ref().clone();
    assert!(e.command(Command::Undo));
    assert!(e.file().elements.is_empty());
    assert!(e.command(Command::Redo));
    assert_eq!(*e.file().as_ref(), created);
}
