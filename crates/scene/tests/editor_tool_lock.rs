mod support;

use scene::editor::{Command, Tool};
use support::*;

#[test]
fn a_locked_tool_stays_active_and_selects_nothing_it_draws() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Rectangle);
    e.toggle_tool_lock();
    assert!(e.tool_locked());
    drag(&mut e, at(0.0, 0.0), [50.0, 50.0]);
    drag(&mut e, at(100.0, 0.0), [150.0, 50.0]);
    assert_eq!(e.tool(), Tool::Rectangle);
    assert_eq!(e.file().elements.len(), 2);
    assert!(selected(&e).is_empty());

    e.set_tool(Tool::Arrow);
    drag(&mut e, at(0.0, 100.0), [100.0, 100.0]);
    assert_eq!(e.tool(), Tool::Arrow);
    assert_eq!(e.file().elements.len(), 3);
    assert!(selected(&e).is_empty());
}

#[test]
fn a_locked_text_tool_stays_active_after_committing() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Text);
    e.set_tool_locked(true);
    click(&mut e, at(10.0, 10.0));
    assert!(e.commit_text("hi", &mut CharWidthMeasure));
    assert_eq!(e.tool(), Tool::Text);
    assert!(selected(&e).is_empty());
}

#[test]
fn unlocking_returns_to_the_selection_tool() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Line);
    e.toggle_tool_lock();
    assert_eq!(e.tool(), Tool::Line);
    e.toggle_tool_lock();
    assert!(!e.tool_locked());
    assert_eq!(e.tool(), Tool::Selection);
}

#[test]
fn escape_with_nothing_selected_leaves_a_creation_tool() {
    let mut e = editor(vec![]);
    e.set_tool(Tool::Ellipse);
    e.set_tool_locked(true);
    assert!(e.command(Command::Escape));
    assert_eq!(e.tool(), Tool::Selection);
    assert!(e.tool_locked());
    assert!(!e.command(Command::Escape));
}
