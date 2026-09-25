//! Shared helpers for the editor interaction tests.

#![allow(dead_code)]

use scene::editor::{Editor, Modifiers, PointerEvent};
use scene::env::Env;
use scene::{Element, sample};
use serde_json::Value;

/// xorshift64 random bytes and a clock that advances 1 ms per read.
pub struct TestEnv {
    state: u64,
    now: f64,
}

impl TestEnv {
    pub fn seeded(seed: u64) -> TestEnv {
        TestEnv {
            state: seed.max(1),
            now: 1_700_000_000_000.0,
        }
    }
}

impl Env for TestEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            *byte = (self.state >> 32) as u8;
        }
    }

    fn now_ms(&mut self) -> f64 {
        self.now += 1.0;
        self.now
    }
}

pub fn editor(elements: Vec<Value>) -> Editor<TestEnv> {
    Editor::new(sample::file(elements), TestEnv::seeded(1))
}

pub fn at(x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        at: [x, y],
        modifiers: Modifiers::default(),
        zoom: 1.0,
    }
}

pub fn shift(x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        ..at(x, y)
    }
}

pub fn alt(x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        modifiers: Modifiers {
            alt: true,
            ..Modifiers::default()
        },
        ..at(x, y)
    }
}

pub fn click(editor: &mut Editor<TestEnv>, event: PointerEvent) {
    editor.pointer_down(event);
    editor.pointer_up(event);
}

/// Presses at `from`, moves to `to` in four equal steps and releases there, keeping `from`'s
/// modifiers and zoom.
pub fn drag(editor: &mut Editor<TestEnv>, from: PointerEvent, to: [f64; 2]) {
    editor.pointer_down(from);
    for step in 1..=4 {
        let t = f64::from(step) / 4.0;
        let at = [
            from.at[0] + (to[0] - from.at[0]) * t,
            from.at[1] + (to[1] - from.at[1]) * t,
        ];
        editor.pointer_move(PointerEvent { at, ..from });
    }
    editor.pointer_up(PointerEvent { at: to, ..from });
}

pub fn element<'a>(editor: &'a Editor<TestEnv>, id: &str) -> &'a Element {
    editor
        .file()
        .elements
        .iter()
        .find(|e| e.id() == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}

/// `[x, y, width, height]`.
pub fn rect_of(editor: &Editor<TestEnv>, id: &str) -> [f64; 4] {
    let p = element(editor, id).placement().expect("placement");
    [p.x, p.y, p.width, p.height]
}

pub fn selected(editor: &Editor<TestEnv>) -> Vec<String> {
    editor.selection().iter().map(str::to_string).collect()
}
