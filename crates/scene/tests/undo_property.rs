mod support;

use scene::editor::{Command, Editor, Tool};
use scene::sample;
use serde_json::json;
use support::*;

/// Numerical Recipes LCG picking operations and coordinates.
struct Ops(u32);

impl Ops {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    fn coord(&mut self) -> f64 {
        f64::from(self.below(60)) * 10.0
    }
}

fn starting_scene() -> Vec<serde_json::Value> {
    vec![
        sample::with(
            sample::generic("rectangle", "r", [0.0, 0.0, 100.0, 80.0]),
            json!({"backgroundColor": "#ffc9c9", "boundElements": [{"id": "t", "type": "text"}, {"id": "a", "type": "arrow"}]}),
        ),
        sample::with(
            sample::text("t", [20.0, 30.0, 60.0, 20.0], "hi", Some("r")),
            json!({"textAlign": "center", "verticalAlign": "middle"}),
        ),
        sample::with(
            sample::linear("arrow", "a", [150.0, 40.0], &[[0.0, 0.0], [120.0, 60.0]]),
            json!({"startBinding": {"elementId": "r", "fixedPoint": [1.0, 0.5], "mode": "orbit"}}),
        ),
        sample::with(
            sample::generic("ellipse", "e", [300.0, 200.0, 80.0, 80.0]),
            json!({"groupIds": ["g"]}),
        ),
        sample::with(
            sample::generic("diamond", "d", [400.0, 200.0, 80.0, 80.0]),
            json!({"groupIds": ["g"]}),
        ),
        sample::freedraw("f", [50.0, 300.0], &[[0.0, 0.0], [10.0, 5.0], [20.0, -3.0]]),
        sample::linear(
            "line",
            "l",
            [200.0, 400.0],
            &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]],
        ),
        json!({"id": "img", "type": "image", "x": 500, "y": 50, "width": 60, "height": 40, "angle": 0,
               "isDeleted": false, "version": 1, "versionNonce": 1, "fileId": "file-1"}),
    ]
}

fn center(b: [f64; 4]) -> (f64, f64) {
    ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
}

fn random_step(e: &mut Editor<TestEnv>, ops: &mut Ops) {
    let (x, y) = (ops.coord(), ops.coord());
    let to = [ops.coord(), ops.coord()];
    match ops.below(14) {
        0 => {
            e.set_tool(Tool::Rectangle);
            drag(e, at(x, y), to);
        }
        1 => {
            e.set_tool(Tool::Ellipse);
            drag(e, alt(x, y), to);
        }
        2 => {
            e.set_tool(Tool::Arrow);
            drag(e, at(x, y), to);
        }
        3 => {
            e.set_tool(Tool::Line);
            click(e, at(x, y));
            e.pointer_move(at(to[0], to[1]));
            click(e, at(to[0], to[1]));
            e.command(Command::Finalize);
        }
        4 => {
            e.set_tool(Tool::Freedraw);
            drag(e, at(x, y), to);
            e.set_tool(Tool::Selection);
        }
        5 => {
            e.set_tool(Tool::Selection);
            click(e, at(x, y));
        }
        6 => {
            e.set_tool(Tool::Selection);
            drag(e, at(x, y), to);
        }
        7 => {
            e.set_tool(Tool::Selection);
            drag(e, shift(x, y), to);
        }
        8 => {
            e.command(Command::SelectAll);
        }
        9 => {
            e.command(Command::Delete);
        }
        10 => {
            e.command(Command::Undo);
        }
        11 => {
            e.command(Command::Redo);
        }
        12 => {
            if let Some(handle) = e.overlay(1.0).handles.first().copied() {
                let (hx, hy) = center(handle);
                drag(e, at(hx, hy), to);
            }
        }
        _ => {
            if let Some(point) = e.overlay(1.0).points.last().copied() {
                drag(e, at(point[0], point[1]), to);
            }
        }
    }
}

#[test]
fn undoing_everything_restores_the_initial_scene() {
    for seed in 1..=150u32 {
        let mut e = Editor::new(
            sample::file(starting_scene()),
            TestEnv::seeded(u64::from(seed)),
        );
        let initial = e.file().as_ref().clone();
        let mut ops = Ops(seed);
        for _ in 0..60 {
            random_step(&mut e, &mut ops);
        }
        e.command(Command::Finalize);
        e.set_tool(Tool::Selection);
        let finished = e.file().as_ref().clone();
        let mut undone = 0;
        while e.command(Command::Undo) {
            undone += 1;
        }
        assert_eq!(*e.file().as_ref(), initial, "seed {seed}: undo everything");
        for _ in 0..undone {
            assert!(e.command(Command::Redo), "seed {seed}");
        }
        assert_eq!(*e.file().as_ref(), finished, "seed {seed}: redo everything");
    }
}
