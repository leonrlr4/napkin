//! Translates one frame's raw egui input into the scene editor's own vocabulary: pointer
//! gestures gated on focus, panning state and the canvas rectangle, and keyboard shortcuts for
//! tools and commands.

use eframe::egui;
use scene::editor::{Command, Cursor, Modifiers, PointerEvent, Tool};

use crate::camera::Camera;

/// One editor input translated from an egui event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditorInput {
    Down(PointerEvent),
    Move(PointerEvent),
    Up(PointerEvent),
    Tool(Tool),
    Command(Command),
}

/// A primary press that started on the canvas, and the last pointer position in scene
/// coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerCapture {
    pressed: bool,
    last: Option<[f64; 2]>,
}

pub struct FrameInput<'a> {
    pub events: &'a [egui::Event],
    /// The canvas rectangle in egui points.
    pub canvas: egui::Rect,
    pub camera: Camera,
    /// `egui::InputState::modifiers` for events that carry none.
    pub modifiers: egui::Modifiers,
    /// Space held or the Hand tool active: primary presses pan instead of editing.
    pub panning: bool,
    /// An egui widget has keyboard focus (`Context::wants_keyboard_input`).
    pub keyboard_taken: bool,
    pub focused: bool,
}

/// `egui::Modifiers` to the editor's own type: `ctrl` covers both the physical Ctrl key and
/// (on macOS) Cmd.
fn scene_modifiers(modifiers: egui::Modifiers) -> Modifiers {
    Modifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        ctrl: modifiers.ctrl || modifiers.command,
    }
}

/// `pos` (egui points, canvas-relative) as a scene coordinate.
fn scene_position(input: &FrameInput, pos: egui::Pos2) -> [f64; 2] {
    input.camera.view_to_scene([
        (pos.x - input.canvas.min.x) as f64,
        (pos.y - input.canvas.min.y) as f64,
    ])
}

/// The tool or command a pressed, non-repeat key selects, or `None` for a key with no binding.
fn key_input(key: egui::Key, modifiers: egui::Modifiers) -> Option<EditorInput> {
    if modifiers.ctrl || modifiers.command {
        return match key {
            egui::Key::A => Some(EditorInput::Command(Command::SelectAll)),
            egui::Key::Z if modifiers.shift => Some(EditorInput::Command(Command::Redo)),
            egui::Key::Z => Some(EditorInput::Command(Command::Undo)),
            egui::Key::Y => Some(EditorInput::Command(Command::Redo)),
            _ => None,
        };
    }
    if modifiers.alt {
        return None;
    }
    match key {
        egui::Key::V | egui::Key::Num1 => Some(EditorInput::Tool(Tool::Selection)),
        egui::Key::R | egui::Key::Num2 => Some(EditorInput::Tool(Tool::Rectangle)),
        egui::Key::D | egui::Key::Num3 => Some(EditorInput::Tool(Tool::Diamond)),
        egui::Key::O | egui::Key::Num4 => Some(EditorInput::Tool(Tool::Ellipse)),
        egui::Key::A | egui::Key::Num5 => Some(EditorInput::Tool(Tool::Arrow)),
        egui::Key::L | egui::Key::Num6 => Some(EditorInput::Tool(Tool::Line)),
        egui::Key::P | egui::Key::X | egui::Key::Num7 => Some(EditorInput::Tool(Tool::Freedraw)),
        egui::Key::H => Some(EditorInput::Tool(Tool::Hand)),
        egui::Key::Delete | egui::Key::Backspace => Some(EditorInput::Command(Command::Delete)),
        egui::Key::Escape => Some(EditorInput::Command(Command::Escape)),
        egui::Key::Enter => Some(EditorInput::Command(Command::Finalize)),
        _ => None,
    }
}

/// This frame's editor inputs in event order.
pub fn translate(input: &FrameInput, capture: &mut PointerCapture) -> Vec<EditorInput> {
    let mut out = Vec::new();

    for event in input.events {
        match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            } => {
                let at = scene_position(input, *pos);
                let scene_event = PointerEvent {
                    at,
                    modifiers: scene_modifiers(*modifiers),
                    zoom: input.camera.zoom,
                };
                if *pressed {
                    if input.focused && !input.panning && input.canvas.contains(*pos) {
                        capture.pressed = true;
                        capture.last = Some(at);
                        out.push(EditorInput::Down(scene_event));
                    }
                } else if capture.pressed {
                    capture.pressed = false;
                    capture.last = Some(at);
                    out.push(EditorInput::Up(scene_event));
                }
            }
            egui::Event::PointerMoved(pos) => {
                if capture.pressed || input.canvas.contains(*pos) {
                    let at = scene_position(input, *pos);
                    capture.last = Some(at);
                    out.push(EditorInput::Move(PointerEvent {
                        at,
                        modifiers: scene_modifiers(input.modifiers),
                        zoom: input.camera.zoom,
                    }));
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if !input.keyboard_taken => {
                if let Some(mapped) = key_input(*key, *modifiers) {
                    out.push(mapped);
                }
            }
            _ => {}
        }
    }

    // A window that loses focus mid-drag never delivers the matching button-up event: release
    // at the last known position so the editor's gesture does not stay open forever.
    if !input.focused && capture.pressed {
        capture.pressed = false;
        out.push(EditorInput::Up(PointerEvent {
            at: capture.last.unwrap_or([0.0, 0.0]),
            modifiers: Modifiers::default(),
            zoom: input.camera.zoom,
        }));
    }

    out
}

/// Maps the editor's requested cursor to an egui icon.
pub fn cursor_icon(cursor: Cursor) -> egui::CursorIcon {
    match cursor {
        Cursor::Default => egui::CursorIcon::Default,
        Cursor::Move => egui::CursorIcon::Move,
        Cursor::Crosshair => egui::CursorIcon::Crosshair,
        Cursor::Pointer => egui::CursorIcon::PointingHand,
        Cursor::ResizeNwse => egui::CursorIcon::ResizeNwSe,
        Cursor::ResizeNesw => egui::CursorIcon::ResizeNeSw,
        Cursor::ResizeNs => egui::CursorIcon::ResizeVertical,
        Cursor::ResizeEw => egui::CursorIcon::ResizeHorizontal,
    }
}

#[cfg(test)]
mod tests {
    use scene::editor::{Command, Cursor, Modifiers, PointerEvent, Tool};

    use super::*;

    fn frame(events: &[egui::Event]) -> FrameInput<'_> {
        FrameInput {
            events,
            canvas: egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0)),
            camera: Camera {
                scroll_x: 0.0,
                scroll_y: 0.0,
                zoom: 2.0,
            },
            modifiers: egui::Modifiers::NONE,
            panning: false,
            keyboard_taken: false,
            focused: true,
        }
    }

    fn button(x: f32, y: f32, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn pointer(x: f64, y: f64, modifiers: Modifiers) -> PointerEvent {
        PointerEvent {
            at: [x, y],
            modifiers,
            zoom: 2.0,
        }
    }

    #[test]
    fn pointer_events_become_scene_coordinates() {
        let events = [
            button(30.0, 40.0, true, egui::Modifiers::SHIFT),
            egui::Event::PointerMoved(egui::pos2(50.0, 60.0)),
            button(50.0, 60.0, false, egui::Modifiers::NONE),
        ];
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            translate(&frame(&events), &mut PointerCapture::default()),
            vec![
                EditorInput::Down(pointer(10.0, 10.0, shift)),
                EditorInput::Move(pointer(20.0, 20.0, Modifiers::default())),
                EditorInput::Up(pointer(20.0, 20.0, Modifiers::default())),
            ]
        );
    }

    #[test]
    fn only_presses_that_start_on_the_canvas_reach_the_editor() {
        let mut capture = PointerCapture::default();
        let outside = [
            button(5.0, 5.0, true, egui::Modifiers::NONE),
            egui::Event::PointerMoved(egui::pos2(900.0, 900.0)),
        ];
        assert_eq!(translate(&frame(&outside), &mut capture), vec![]);
        let release = [button(900.0, 900.0, false, egui::Modifiers::NONE)];
        assert_eq!(translate(&frame(&release), &mut capture), vec![]);

        let press = [button(30.0, 40.0, true, egui::Modifiers::NONE)];
        let mut panning = frame(&press);
        panning.panning = true;
        assert_eq!(translate(&panning, &mut capture), vec![]);
        assert_eq!(translate(&frame(&release), &mut capture), vec![]);

        translate(&frame(&press), &mut capture);
        let beyond = [
            egui::Event::PointerMoved(egui::pos2(1000.0, 20.0)),
            button(1000.0, 20.0, false, egui::Modifiers::NONE),
        ];
        assert_eq!(
            translate(&frame(&beyond), &mut capture),
            vec![
                EditorInput::Move(pointer(495.0, 0.0, Modifiers::default())),
                EditorInput::Up(pointer(495.0, 0.0, Modifiers::default())),
            ]
        );
    }

    #[test]
    fn losing_focus_mid_drag_releases_at_the_last_position() {
        let mut capture = PointerCapture::default();
        translate(
            &frame(&[button(30.0, 40.0, true, egui::Modifiers::NONE)]),
            &mut capture,
        );
        let mut unfocused = frame(&[]);
        unfocused.focused = false;
        assert_eq!(
            translate(&unfocused, &mut capture),
            vec![EditorInput::Up(pointer(10.0, 10.0, Modifiers::default()))]
        );
        assert_eq!(translate(&unfocused, &mut capture), vec![]);
    }

    #[test]
    fn keys_map_to_tools_and_commands() {
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::COMMAND
        };
        let none = egui::Modifiers::NONE;
        let events = [
            key(egui::Key::R, none),
            key(egui::Key::Num5, none),
            key(egui::Key::X, none),
            key(egui::Key::H, none),
            key(egui::Key::A, ctrl),
            key(egui::Key::Z, ctrl),
            key(egui::Key::Z, ctrl_shift),
            key(egui::Key::Y, ctrl),
            key(egui::Key::Delete, none),
            key(egui::Key::Backspace, none),
            key(egui::Key::Escape, none),
            key(egui::Key::Enter, none),
            key(egui::Key::R, ctrl),
        ];
        assert_eq!(
            translate(&frame(&events), &mut PointerCapture::default()),
            vec![
                EditorInput::Tool(Tool::Rectangle),
                EditorInput::Tool(Tool::Arrow),
                EditorInput::Tool(Tool::Freedraw),
                EditorInput::Tool(Tool::Hand),
                EditorInput::Command(Command::SelectAll),
                EditorInput::Command(Command::Undo),
                EditorInput::Command(Command::Redo),
                EditorInput::Command(Command::Redo),
                EditorInput::Command(Command::Delete),
                EditorInput::Command(Command::Delete),
                EditorInput::Command(Command::Escape),
                EditorInput::Command(Command::Finalize),
            ]
        );
        let mut typing = frame(&events);
        typing.keyboard_taken = true;
        assert_eq!(translate(&typing, &mut PointerCapture::default()), vec![]);
    }

    #[test]
    fn cursors_map_to_egui_icons() {
        assert_eq!(
            cursor_icon(Cursor::ResizeNwse),
            egui::CursorIcon::ResizeNwSe
        );
        assert_eq!(
            cursor_icon(Cursor::ResizeNs),
            egui::CursorIcon::ResizeVertical
        );
        assert_eq!(cursor_icon(Cursor::Pointer), egui::CursorIcon::PointingHand);
        assert_eq!(cursor_icon(Cursor::Crosshair), egui::CursorIcon::Crosshair);
    }
}
