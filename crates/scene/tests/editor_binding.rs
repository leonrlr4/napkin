mod support;

use scene::Element;
use scene::binding::linear_end_point;
use scene::editor::{Command, Editor, Modifiers, PointerEvent, Tool};
use scene::element::LinearEnd;
use scene::sample::{self, CharWidthMeasure};
use serde_json::{Value, json};
use support::*;

fn filled_rectangle(id: &str, rect: [f64; 4]) -> Value {
    sample::with(
        sample::generic("rectangle", id, rect),
        json!({"backgroundColor": "#ffc9c9"}),
    )
}

fn last_element(editor: &Editor<TestEnv>) -> &Element {
    editor.file().elements.last().expect("an element")
}

fn event(at: [f64; 2], modifiers: Modifiers) -> PointerEvent {
    PointerEvent {
        at,
        modifiers,
        zoom: 1.0,
    }
}

fn hover(editor: &mut Editor<TestEnv>, to: [f64; 2]) {
    editor.pointer_move(event(to, Modifiers::default()), &mut CharWidthMeasure);
}

fn drag_with(editor: &mut Editor<TestEnv>, from: [f64; 2], to: [f64; 2], modifiers: Modifiers) {
    drag(editor, event(from, modifiers), to);
}

/// An arrow drawn onto `r`'s left edge by the editor itself, so the binding is whatever the
/// editor produces, then back to the selection tool.
fn bound_pair() -> Editor<TestEnv> {
    let mut editor = editor(vec![filled_rectangle("r", [200.0, 0.0, 100.0, 100.0])]);
    editor.set_tool(Tool::Arrow);
    drag_with(
        &mut editor,
        [0.0, 50.0],
        [195.0, 50.0],
        Modifiers::default(),
    );
    assert_eq!(
        last_element(&editor).binding_target(LinearEnd::End),
        Some("r")
    );
    editor.command(Command::Escape);
    editor.set_tool(Tool::Selection);
    editor
}

fn arrow_id(editor: &Editor<TestEnv>) -> String {
    last_element(editor).id().expect("id").to_owned()
}

#[test]
fn drawing_an_arrow_onto_a_rectangle_binds_it_and_one_undo_removes_everything() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    drag_with(
        &mut editor,
        [0.0, 50.0],
        [195.0, 50.0],
        Modifiers::default(),
    );
    let arrow = last_element(&editor);
    assert_eq!(arrow.binding_target(LinearEnd::End), Some("r"));
    assert_eq!(arrow.binding_target(LinearEnd::Start), None);
    assert_eq!(
        element(&editor, "r").bound_elements(),
        vec![(arrow.id().unwrap(), "arrow")]
    );
    assert!(editor.command(Command::Undo));
    assert_eq!(editor.file().elements.len(), 1);
    assert!(element(&editor, "r").bound_elements().is_empty());
}

#[test]
fn starting_an_arrow_on_a_shape_binds_its_start() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [0.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    drag_with(
        &mut editor,
        [105.0, 50.0],
        [300.0, 50.0],
        Modifiers::default(),
    );
    let arrow = last_element(&editor);
    assert_eq!(arrow.binding_target(LinearEnd::Start), Some("r"));
    assert_eq!(arrow.binding_target(LinearEnd::End), None);
}

#[test]
fn dragging_a_bound_rectangle_moves_the_arrow_end_and_undo_restores_it() {
    let mut editor = bound_pair();
    let arrow = arrow_id(&editor);
    let before = element(&editor, &arrow).clone();
    let end_before = linear_end_point(&before, LinearEnd::End).unwrap();
    editor.set_tool(Tool::Selection);
    drag_with(
        &mut editor,
        [250.0, 50.0],
        [250.0, 150.0],
        Modifiers::default(),
    );
    let end = linear_end_point(element(&editor, &arrow), LinearEnd::End).unwrap();
    assert!(end[1] > end_before[1] + 50.0, "{end:?} from {end_before:?}");
    assert!(editor.command(Command::Undo));
    assert_eq!(element(&editor, &arrow), &before);
}

#[test]
fn dragging_a_bound_arrow_end_away_unbinds_it_in_one_undo_step() {
    let mut editor = bound_pair();
    let arrow = arrow_id(&editor);
    let before_arrow = element(&editor, &arrow).clone();
    let before_rect = element(&editor, "r").clone();
    // Select the arrow, then pull its end handle off the shape.
    click(&mut editor, at(100.0, 50.0));
    assert_eq!(selected(&editor), [arrow.as_str()]);
    let end = linear_end_point(element(&editor, &arrow), LinearEnd::End).unwrap();
    drag_with(&mut editor, end, [150.0, 300.0], Modifiers::default());
    assert_eq!(
        element(&editor, &arrow).binding_target(LinearEnd::End),
        None
    );
    assert!(element(&editor, "r").bound_elements().is_empty());
    assert!(editor.command(Command::Undo));
    assert_eq!(element(&editor, &arrow), &before_arrow);
    assert_eq!(element(&editor, "r"), &before_rect);
}

#[test]
fn hovering_with_the_arrow_tool_shows_the_binding_highlight() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    hover(&mut editor, [195.0, 50.0]);
    let highlight = editor.overlay(1.0).binding_highlight.expect("highlight");
    assert!(highlight.outline.len() >= 4);
    assert_eq!(highlight.outline.first(), highlight.outline.last());
    assert_eq!(highlight.stroke_width, 2.0);
    assert_eq!(highlight.midpoint_radius, 4.0);
    assert_eq!(highlight.midpoints, vec![([200.0, 50.0], true)]);
    hover(&mut editor, [0.0, 0.0]);
    assert!(editor.overlay(1.0).binding_highlight.is_none());
}

#[test]
fn the_highlight_is_gone_after_switching_tools_and_while_ctrl_is_held() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    editor.pointer_move(
        event(
            [195.0, 50.0],
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
        &mut CharWidthMeasure,
    );
    assert!(editor.overlay(1.0).binding_highlight.is_none());
    hover(&mut editor, [195.0, 50.0]);
    assert!(editor.overlay(1.0).binding_highlight.is_some());
    editor.set_tool(Tool::Rectangle);
    assert!(editor.overlay(1.0).binding_highlight.is_none());
}

#[test]
fn a_line_never_binds_or_shows_a_hint() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Line);
    hover(&mut editor, [195.0, 50.0]);
    assert!(editor.overlay(1.0).binding_highlight.is_none());
    drag_with(
        &mut editor,
        [0.0, 50.0],
        [195.0, 50.0],
        Modifiers::default(),
    );
    assert!(element(&editor, "r").bound_elements().is_empty());
}

#[test]
fn ctrl_held_draws_an_unbound_arrow() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    drag_with(
        &mut editor,
        [0.0, 50.0],
        [195.0, 50.0],
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(last_element(&editor).binding_target(LinearEnd::End), None);
    assert!(element(&editor, "r").bound_elements().is_empty());
}

#[test]
fn escaping_a_drag_that_bound_leaves_the_shape_untouched() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    let before = editor.file().as_ref().clone();
    editor.set_tool(Tool::Arrow);
    editor.pointer_down(at(0.0, 50.0), &mut CharWidthMeasure);
    editor.pointer_move(at(195.0, 50.0), &mut CharWidthMeasure);
    assert!(editor.command(Command::Escape));
    assert_eq!(editor.file().as_ref(), &before);
    assert!(!editor.command(Command::Undo));
}

#[test]
fn clicking_an_arrow_onto_a_shape_ends_it_there_bound() {
    let mut editor = editor(vec![sample::generic(
        "rectangle",
        "r",
        [200.0, 0.0, 100.0, 100.0],
    )]);
    editor.set_tool(Tool::Arrow);
    // A click without a drag starts a multi-point arrow.
    click(&mut editor, at(0.0, 50.0));
    hover(&mut editor, [100.0, 80.0]);
    click(&mut editor, at(100.0, 80.0));
    hover(&mut editor, [195.0, 50.0]);
    click(&mut editor, at(195.0, 50.0));
    assert!(editor.is_idle());
    let arrow = last_element(&editor);
    assert_eq!(arrow.binding_target(LinearEnd::End), Some("r"));
    assert_eq!(editor.tool(), Tool::Selection);
    assert!(editor.command(Command::Undo));
    assert_eq!(editor.file().elements.len(), 1);
    assert!(element(&editor, "r").bound_elements().is_empty());
}

#[test]
fn a_label_that_grows_its_container_drags_the_bound_arrow_along() {
    // r: 100x50 at (0, 0); arrow a starts below r and ends bound to r's bottom.
    let mut editor = editor(vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 50.0]),
            json!({"boundElements": [{"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::linear("arrow", "a", [0.0, 120.0], &[[0.0, 0.0], [50.01, -64.0]]),
            json!({"endBinding": {"elementId": "r", "mode": "orbit",
                                  "fixedPoint": [0.5001, 1.12]}}),
        ),
    ]);
    let before = editor.file().as_ref().clone();
    let bottom = |editor: &Editor<TestEnv>| {
        let p = element(editor, "r").placement().unwrap();
        p.y + p.height
    };
    assert_eq!(bottom(&editor), 50.0);
    assert!(editor.double_click(at(50.0, 25.0), &mut CharWidthMeasure));
    assert!(editor.commit_text("aaaa\nbbbb\ncccc\ndddd", &mut CharWidthMeasure));
    assert!(bottom(&editor) > 50.0, "{}", bottom(&editor));
    let end = linear_end_point(element(&editor, "a"), LinearEnd::End).unwrap();
    assert!(end[1] > bottom(&editor), "{end:?} vs {}", bottom(&editor));
    assert!(editor.command(Command::Undo));
    assert_eq!(editor.file().as_ref(), &before);
}

#[test]
fn changing_a_bound_shapes_stroke_width_keeps_the_arrow_on_its_outline() {
    use scene::editor::{Property, StrokeWidth};
    let mut editor = bound_pair();
    let arrow = arrow_id(&editor);
    let end_before = linear_end_point(element(&editor, &arrow), LinearEnd::End).unwrap();
    click(&mut editor, at(250.0, 50.0));
    assert_eq!(selected(&editor), ["r"]);
    editor.set_property(
        Property::StrokeWidth(StrokeWidth::ExtraBold),
        &mut CharWidthMeasure,
    );
    let end = linear_end_point(element(&editor, &arrow), LinearEnd::End).unwrap();
    assert_ne!(end, end_before, "the binding gap grows with the stroke");
    assert!(editor.command(Command::Undo));
    assert_eq!(
        linear_end_point(element(&editor, &arrow), LinearEnd::End).unwrap(),
        end_before
    );
}
