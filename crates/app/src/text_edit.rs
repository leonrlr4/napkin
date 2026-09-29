//! The overlay that edits a text element in place: a frameless `egui::TextEdit` positioned and
//! sized from `Editor::text_editing()`'s [`TextEditing`], committing the same way
//! `wysiwyg/textWysiwyg.tsx`'s `handleSubmit` does (Escape, Ctrl/Cmd+Enter, or losing focus;
//! plain Enter inserts a newline instead, same as that file's own default `textarea` behaviour).
//!
//! Escape needs no explicit key check here: `egui::TextEdit`'s focus-lock filter does not claim
//! it, so egui itself surrenders the widget's focus the moment it is pressed, and [`show`]
//! already treats any loss of focus as a commit.

use eframe::egui;
use scene::editor::TextEditing;

use crate::camera::Camera;
use crate::fonts;
use crate::render::color::render_color;

/// What the overlay decided this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum TextEditOutcome {
    Editing,
    /// Editing ended; this is the text to pass to `Editor::commit_text`.
    Commit(String),
}

/// The overlay's on-screen anchor and font size for `editing`, under `camera` and with the
/// canvas's own top-left at `canvas_origin`: `editing.origin` projected into screen points, and
/// `editing.font_size` scaled by the same zoom. [`show`] anchors the `TextEdit` on this point
/// directly, top-left for left-aligned text and centered for a container label (`editing.origin`
/// is that label's container center rather than a top-left, since editing a brand new label has
/// no element yet to have one), so the box grows to the right or symmetrically about the center
/// as typing changes its width, rather than needing to know that width up front.
pub fn layout(
    editing: &TextEditing,
    camera: Camera,
    canvas_origin: egui::Pos2,
) -> (egui::Pos2, f32) {
    let [x, y] = camera.scene_to_view(editing.origin);
    let pos = egui::pos2(canvas_origin.x + x as f32, canvas_origin.y + y as f32);
    (pos, (editing.font_size * camera.zoom) as f32)
}

/// Whether a freshly pressed `key` should end editing while keeping the buffer's current text:
/// `textWysiwyg`'s `onKeyDown`, its `Enter` + `CTRL_OR_CMD` branch (its `Escape` branch has no
/// equivalent here; see this module's own doc comment).
fn is_forced_submit(key: egui::Key, modifiers: egui::Modifiers) -> bool {
    key == egui::Key::Enter && (modifiers.ctrl || modifiers.command)
}

/// `editing.stroke_color`, dark-mode filtered when `dark` (`applyDarkModeFilter`).
fn stroke_color(editing: &TextEditing, dark: bool) -> egui::Color32 {
    let rgba = render_color(&editing.stroke_color, dark);
    egui::Color32::from_rgba_unmultiplied(
        (rgba[0] * 255.0).round() as u8,
        (rgba[1] * 255.0).round() as u8,
        (rgba[2] * 255.0).round() as u8,
        (rgba[3] * 255.0).round() as u8,
    )
}

/// Shows the overlay for one frame: a frameless, multiline `TextEdit` over `editing`'s box, its
/// width growing with `buffer`'s content, in `fonts::egui_family(editing.font_family)` at
/// [`layout`]'s screen font size and in `editing.stroke_color` (dark-mode filtered when `dark`).
/// Takes keyboard focus on `first_frame`. A rotated container's label is still shown unrotated
/// here (egui cannot rotate a `TextEdit`); the committed element is drawn at its own angle once
/// the caller applies the [`TextEditOutcome::Commit`] this returns.
///
/// Returns [`TextEditOutcome::Commit`] with `buffer`'s current text once Ctrl/Cmd+Enter is
/// pressed or the widget loses focus (which Escape, and a click outside it, both already cause
/// on their own, per this module's doc comment); the caller still has to call
/// `Editor::commit_text` and clear its own copy of `buffer` itself, since committing needs a
/// `TextMeasure` this function has no access to.
pub fn show(
    ui: &mut egui::Ui,
    editing: &TextEditing,
    buffer: &mut String,
    camera: Camera,
    canvas_origin: egui::Pos2,
    dark: bool,
    first_frame: bool,
) -> TextEditOutcome {
    let (anchor, font_size) = layout(editing, camera, canvas_origin);
    let font_id = egui::FontId::new(font_size, fonts::egui_family(editing.font_family));
    let color = stroke_color(editing, dark);
    let pivot = if editing.text_align == "center" {
        egui::Align2::CENTER_CENTER
    } else {
        egui::Align2::LEFT_TOP
    };

    let forced_submit = ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if is_forced_submit(*key, *modifiers)
            )
        })
    });

    let mut outcome = TextEditOutcome::Editing;
    egui::Area::new(egui::Id::new("napkin-text-edit"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .pivot(pivot)
        .show(ui.ctx(), |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut *buffer)
                    .frame(egui::Frame::NONE)
                    .font(font_id)
                    .text_color(color)
                    .desired_width(0.0),
            );
            if first_frame {
                response.request_focus();
            }
            if forced_submit || response.lost_focus() {
                outcome = TextEditOutcome::Commit(buffer.clone());
            }
        });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_editing(text_align: &str) -> TextEditing {
        TextEditing {
            element_id: None,
            container_id: None,
            text: String::new(),
            origin: [10.0, 20.0],
            width: 0.0,
            font_family: 5.0,
            font_size: 20.0,
            line_height: 1.25,
            text_align: text_align.to_owned(),
            stroke_color: "#1e1e1e".to_owned(),
            opacity: 100.0,
            angle: 0.0,
            group_ids: Vec::new(),
        }
    }

    #[test]
    fn doubling_zoom_doubles_position_and_font_size() {
        let editing = sample_editing("left");
        let canvas_origin = egui::pos2(5.0, 8.0);
        let camera = |zoom| Camera {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom,
        };
        let (pos1, size1) = layout(&editing, camera(1.0), canvas_origin);
        let (pos2, size2) = layout(&editing, camera(2.0), canvas_origin);
        assert_eq!(pos2.x - canvas_origin.x, (pos1.x - canvas_origin.x) * 2.0);
        assert_eq!(pos2.y - canvas_origin.y, (pos1.y - canvas_origin.y) * 2.0);
        assert_eq!(size2, size1 * 2.0);
    }

    #[test]
    fn only_ctrl_or_cmd_enter_forces_a_submit() {
        assert!(is_forced_submit(egui::Key::Enter, egui::Modifiers::COMMAND));
        assert!(is_forced_submit(
            egui::Key::Enter,
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::NONE
            }
        ));
        assert!(!is_forced_submit(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(!is_forced_submit(
            egui::Key::Escape,
            egui::Modifiers::COMMAND
        ));
    }
}
