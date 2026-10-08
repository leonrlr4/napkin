mod support;

use scene::clipboard::{self, Pasted};
use scene::editor::{Command, Tool};
use scene::sample::{self, CharWidthMeasure};
use serde_json::{Value, json};
use support::*;

#[test]
fn copy_then_paste_round_trips_through_excalidraw_clipboard_json() {
    let mut e = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::text("t", [30.0, 12.5, 40.0, 25.0], "hi", Some("r")),
    ]);
    e.command(Command::SelectAll);
    let text = e.copy_selection().expect("something selected");
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], json!("excalidraw/clipboard"));
    assert_eq!(value["elements"].as_array().unwrap().len(), 2);

    assert!(e.paste(&text, [500.0, 500.0], &mut CharWidthMeasure));
    let live: Vec<Value> = e
        .file()
        .elements
        .iter()
        .filter(|x| !x.is_deleted())
        .map(|x| x.to_value())
        .collect();
    assert_eq!(live.len(), 4);
    let pasted_rect = &live[2];
    assert_eq!(
        (pasted_rect["x"].clone(), pasted_rect["y"].clone()),
        (json!(450.0), json!(475.0))
    );
    assert_ne!(pasted_rect["id"], json!("r"));
    assert_eq!(live[3]["containerId"], pasted_rect["id"]);
    assert_eq!(e.selection().len(), 2);
    assert!(e.command(Command::Undo));
    assert_eq!(
        e.file().elements.iter().filter(|x| !x.is_deleted()).count(),
        2
    );
}

#[test]
fn plain_text_pastes_as_a_text_element() {
    let mut e = editor(vec![]);
    assert!(matches!(clipboard::parse("hello"), Some(Pasted::Text(_))));
    assert!(e.paste("hello", [10.0, 20.0], &mut CharWidthMeasure));
    let v = e.file().elements[0].to_value();
    assert_eq!(v["type"], json!("text"));
    assert_eq!(v["text"], json!("hello"));
    assert!(clipboard::parse("").is_none());
}

#[test]
fn paste_is_refused_while_drawing_a_multi_point_line() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    click(&mut e, at(0.0, 0.0));
    e.pointer_move(at(100.0, 0.0), &mut CharWidthMeasure);
    click(&mut e, at(100.0, 0.0));
    e.pointer_move(at(100.0, 100.0), &mut CharWidthMeasure);

    assert!(!e.paste("hello", [500.0, 500.0], &mut CharWidthMeasure));
    assert!(!e.is_idle(), "the line gesture must still be in progress");

    click(&mut e, at(100.0, 100.0));
    assert!(e.command(Command::Finalize));
    let live: Vec<Value> = e
        .file()
        .elements
        .iter()
        .filter(|x| !x.is_deleted())
        .map(|x| x.to_value())
        .collect();
    assert_eq!(live.len(), 1, "the refused paste must not add anything");
    assert_eq!(live[0]["type"], json!("line"));
}

#[test]
fn paste_is_refused_mid_selection_drag() {
    let mut e = editor(vec![sample::with(
        sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 100.0]),
        json!({"backgroundColor": "#ffc9c9"}),
    )]);
    e.pointer_down(at(50.0, 50.0), &mut CharWidthMeasure);
    e.pointer_move(at(80.0, 50.0), &mut CharWidthMeasure);

    assert!(!e.paste("hello", [500.0, 500.0], &mut CharWidthMeasure));

    e.pointer_move(at(120.0, 50.0), &mut CharWidthMeasure);
    e.pointer_up(at(120.0, 50.0), &mut CharWidthMeasure);
    let live: Vec<Value> = e
        .file()
        .elements
        .iter()
        .filter(|x| !x.is_deleted())
        .map(|x| x.to_value())
        .collect();
    assert_eq!(live.len(), 1, "the refused paste must not add anything");
    assert_eq!(
        (live[0]["x"].clone(), live[0]["y"].clone()),
        (json!(70.0), json!(0.0)),
        "the drag must still complete normally"
    );
    assert_eq!(selected(&e), ["r"]);
}

#[test]
fn duplicate_offsets_by_half_a_grid_and_selects_the_copies() {
    let mut e = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 10.0, 10.0],
    )]);
    e.command(Command::SelectAll);
    assert!(e.command(Command::Duplicate));
    let copy = e.file().elements[1].to_value();
    assert_eq!(
        (copy["x"].clone(), copy["y"].clone()),
        (json!(10.0), json!(10.0))
    );
    assert!(e.selection().contains(copy["id"].as_str().unwrap()));
    assert!(!e.selection().contains("r"));
}

#[test]
fn a_pasted_label_is_rewrapped_with_the_local_measurer() {
    let mut source = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        ),
        sample::with(
            sample::text("t", [5.0, 12.5, 132.0, 25.0], "hello world", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
    ]);
    source.command(Command::SelectAll);
    let text = source.copy_selection().unwrap();

    let mut e = editor(vec![]);
    assert!(e.paste(&text, [200.0, 200.0], &mut CharWidthMeasure));
    let live: Vec<Value> = e.file().elements.iter().map(|x| x.to_value()).collect();
    assert_eq!(live[1]["text"], json!("hello\nworld"));
    assert_eq!(live[0]["height"], json!(60.0));
}
