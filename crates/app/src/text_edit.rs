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

/// The overlay's `TextEdit`'s own id: explicit (not egui's auto-assigned one) so [`show`] can
/// name it again to clear its persisted state when a new edit starts.
fn text_edit_id() -> egui::Id {
    egui::Id::new("napkin-text-edit-buffer")
}

/// What the overlay decided this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum TextEditOutcome {
    Editing,
    /// Editing ended; this is the text to pass to `Editor::commit_text`.
    Commit(String),
}

/// The overlay's on-screen anchor and font size for `editing`, under `camera` and with the
/// canvas's own top-left at `canvas_origin`: `editing.origin` projected into screen points, and
/// `editing.font_size` scaled by the same zoom. [`pivot`] decides what that projected point
/// anchors; the box then grows to the right or symmetrically about the anchor as typing changes
/// its width, rather than needing to know that width up front.
pub fn layout(
    editing: &TextEditing,
    camera: Camera,
    canvas_origin: egui::Pos2,
) -> (egui::Pos2, f32) {
    let [x, y] = camera.scene_to_view(editing.origin);
    let pos = egui::pos2(canvas_origin.x + x as f32, canvas_origin.y + y as f32);
    (pos, (editing.font_size * camera.zoom) as f32)
}

/// Where [`show`] anchors the box [`layout`] positions: the container's own center for a label
/// that does not exist as an element yet (`editing.element_id` is `None` but `container_id` is
/// `Some`), since there has been no `bind_label`/`bound_text_position` call yet to have computed
/// a real top-left for it; a plain top-left for everything else. That covers an existing text or
/// label (`editing.origin` is already that element's own `x`/`y`, itself centered within its
/// container already if it is a bound label re-opened for editing) and a brand new free text
/// (`editing.origin` is likewise its intended top-left). `editing.text_align` plays no part in
/// this choice: `textWysiwyg.tsx`'s own CSS `textAlign` only aligns text within an
/// already-positioned, fixed-left box; it never moves the box itself, so a right- or
/// center-aligned standalone text is anchored top-left exactly like a left-aligned one.
fn pivot(editing: &TextEditing) -> egui::Align2 {
    if editing.element_id.is_none() && editing.container_id.is_some() {
        egui::Align2::CENTER_CENTER
    } else {
        egui::Align2::LEFT_TOP
    }
}

/// Whether a freshly pressed `key` should end editing while keeping the buffer's current text:
/// `textWysiwyg`'s `onKeyDown`, its `Enter` + `CTRL_OR_CMD` branch (its `Escape` branch has no
/// equivalent here; see this module's own doc comment).
fn is_forced_submit(key: egui::Key, modifiers: egui::Modifiers) -> bool {
    key == egui::Key::Enter && (modifiers.ctrl || modifiers.command)
}

/// `editing.stroke_color`, dark-mode filtered when `dark` (`applyDarkModeFilter`), its alpha
/// additionally scaled by `editing.opacity / 100` (`textWysiwyg.tsx`'s `editable.style.opacity =
/// updatedTextElement.opacity / 100`).
fn stroke_color(editing: &TextEditing, dark: bool) -> egui::Color32 {
    let rgba = render_color(&editing.stroke_color, dark);
    let alpha = (rgba[3] * (editing.opacity / 100.0) as f32).clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        (rgba[0] * 255.0).round() as u8,
        (rgba[1] * 255.0).round() as u8,
        (rgba[2] * 255.0).round() as u8,
        (alpha * 255.0).round() as u8,
    )
}

/// Shows the overlay for one frame: a frameless, multiline `TextEdit` over `editing`'s box, its
/// width growing with `buffer`'s content, in `fonts::egui_family(editing.font_family)` at
/// [`layout`]'s screen font size and in [`stroke_color`]. Takes keyboard focus on `first_frame`.
/// A rotated container's label is still shown unrotated
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
    let pivot_align = pivot(editing);

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
        .pivot(pivot_align)
        .show(ui.ctx(), |ui| {
            if first_frame {
                // The `TextEdit` below keeps the same id across every edit, so without this its
                // persisted `TextEditState` (cursor position, and its own Ctrl+Z/Ctrl+Y undo
                // history) would still hold whatever a previous, unrelated edit left behind: a
                // fresh edit's very first Ctrl+Z could restore that earlier edit's text here.
                egui::text_edit::TextEditState::default().store(ui.ctx(), text_edit_id());
            }
            let response = ui.add(
                egui::TextEdit::multiline(&mut *buffer)
                    .id(text_edit_id())
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

    /// [`layout`] itself is a plain projection of `editing.origin`, unaffected by alignment or
    /// by whether the element exists yet: [`pivot`] is what actually reacts to those, and is
    /// tested separately below.
    #[test]
    fn layout_just_projects_origin_for_every_kind_of_edit() {
        let canvas_origin = egui::pos2(3.0, 4.0);
        let camera = Camera {
            scroll_x: 1.0,
            scroll_y: -2.0,
            zoom: 1.5,
        };
        for (text_align, element_id, container_id) in [
            ("center", Some("t"), Some("c")), // an existing, already-centered label
            ("center", None, Some("c")),      // a brand new label, not created yet
            ("left", None, None),             // left-aligned standalone text
            ("right", Some("t"), None),       // right-aligned standalone text
        ] {
            let mut editing = sample_editing(text_align);
            editing.element_id = element_id.map(str::to_owned);
            editing.container_id = container_id.map(str::to_owned);

            let (pos, size) = layout(&editing, camera, canvas_origin);

            let [x, y] = camera.scene_to_view(editing.origin);
            assert_eq!(
                pos,
                egui::pos2(canvas_origin.x + x as f32, canvas_origin.y + y as f32)
            );
            assert_eq!(size, (editing.font_size * camera.zoom) as f32);
        }
    }

    #[test]
    fn pivot_is_top_left_except_for_a_label_with_no_element_yet() {
        // An existing, already-centered label: `origin` is that label's own real top-left
        // (computed once, when it was first bound), not the container's center.
        let mut existing_label = sample_editing("center");
        existing_label.element_id = Some("t".to_owned());
        existing_label.container_id = Some("c".to_owned());
        assert_eq!(pivot(&existing_label), egui::Align2::LEFT_TOP);

        // A brand new label: nothing has computed a real top-left for it yet, so `origin` is
        // just the container's center.
        let mut new_label = sample_editing("center");
        new_label.container_id = Some("c".to_owned());
        assert_eq!(pivot(&new_label), egui::Align2::CENTER_CENTER);

        // Standalone text is always anchored top-left, whatever its alignment: `textAlign` only
        // moves the text within the box, never the box itself.
        let left_text = sample_editing("left");
        assert_eq!(pivot(&left_text), egui::Align2::LEFT_TOP);

        let mut right_text = sample_editing("right");
        right_text.element_id = Some("t".to_owned());
        assert_eq!(pivot(&right_text), egui::Align2::LEFT_TOP);
    }

    #[test]
    fn stroke_color_alpha_scales_with_opacity() {
        let mut editing = sample_editing("left");
        editing.stroke_color = "#000000".to_owned();
        editing.opacity = 50.0;
        assert_eq!(stroke_color(&editing, false).a(), 128);

        editing.opacity = 100.0;
        assert_eq!(stroke_color(&editing, false).a(), 255);
    }

    /// Runs one headless frame of [`show`] over `editing`/`buffer`, with `events` fed through
    /// `egui::RawInput` and a canvas-sized widget allocated first (as `NapkinApp::ui`'s central
    /// panel is), matching how the overlay actually sits in the frame: a click can land on that
    /// canvas widget instead of the overlay's own (possibly zero-sized) `TextEdit`.
    fn run_frame(
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        editing: &TextEditing,
        buffer: &mut String,
        first_frame: bool,
    ) -> TextEditOutcome {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let camera = Camera {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 1.0,
        };
        let mut outcome = TextEditOutcome::Editing;
        let mut output = ctx.run_ui(raw, |ui| {
            let (_rect, _response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            outcome = show(
                ui,
                editing,
                buffer,
                camera,
                egui::Pos2::ZERO,
                false,
                first_frame,
            );
        });
        output.textures_delta.clear();
        outcome
    }

    fn press(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    /// With egui configured the way `napkin_app::configure_input` sets it up
    /// (`SurrenderFocusOn::Presses`), the click that creates a text edit must not commit an
    /// empty string on its own release: [`pointer_down`](scene::editor::Editor::pointer_down)
    /// starts the edit and the overlay `request_focus`es in the very same frame, so the
    /// matching pointer-up lands with the pointer still down over nothing but canvas (the
    /// widget just appeared, at zero width) and would read as "clicked outside" under the
    /// default `SurrenderFocusOn::Clicks`. A later press elsewhere still ends the edit.
    #[test]
    fn keeps_focus_through_its_own_creating_clicks_release_but_not_a_later_press() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| {
            options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
        });
        fonts::install(&ctx, &glyphon::fontdb::Database::new());
        // Font definitions only take effect at the start of the next pass.
        let mut warmup = ctx.run_ui(egui::RawInput::default(), |_| {});
        warmup.textures_delta.clear();
        let editing = sample_editing("left");
        let click = egui::pos2(editing.origin[0] as f32, editing.origin[1] as f32);
        let mut buffer = String::new();

        assert_eq!(
            run_frame(&ctx, 0.0, press(click, true), &editing, &mut buffer, true),
            TextEditOutcome::Editing,
            "the creating press should not itself commit"
        );
        assert_eq!(
            run_frame(
                &ctx,
                0.01,
                press(click, false),
                &editing,
                &mut buffer,
                false
            ),
            TextEditOutcome::Editing,
            "the creating click's own release must not surrender focus"
        );

        let elsewhere = egui::pos2(500.0, 500.0);
        assert!(
            matches!(
                run_frame(
                    &ctx,
                    0.02,
                    press(elsewhere, true),
                    &editing,
                    &mut buffer,
                    false
                ),
                TextEditOutcome::Commit(_)
            ),
            "a later press elsewhere should still end the edit"
        );
    }

    /// With the widget's `TextEditState` reset every time a new edit starts (`show`'s own
    /// `first_frame` branch), pressing Ctrl+Z right after typing into a brand new edit can only
    /// ever undo within that edit's own (empty) history, never reach back into an earlier,
    /// already-committed edit that reused the same widget id.
    #[test]
    fn ctrl_z_in_a_new_edit_never_restores_a_previous_edits_text() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, &glyphon::fontdb::Database::new());
        let mut warmup = ctx.run_ui(egui::RawInput::default(), |_| {});
        warmup.textures_delta.clear();

        let text_event = |text: &str| vec![egui::Event::Text(text.to_owned())];
        let ctrl_z = vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }];

        // Edit A: focus, type "A", then let the widget's own undo point for "A" stabilize
        // (`Undoer`'s `stable_time` is 1 second).
        let editing_a = sample_editing("left");
        let mut buffer_a = String::new();
        run_frame(&ctx, 0.0, vec![], &editing_a, &mut buffer_a, true);
        run_frame(&ctx, 0.1, text_event("A"), &editing_a, &mut buffer_a, false);
        run_frame(&ctx, 2.0, vec![], &editing_a, &mut buffer_a, false);
        assert_eq!(buffer_a, "A");

        // Edit A commits; the app starts a fresh buffer for a new edit B, reusing the same
        // overlay (and so the same widget id) `show` always draws at.
        let editing_b = sample_editing("left");
        let mut buffer_b = String::new();
        run_frame(&ctx, 2.1, vec![], &editing_b, &mut buffer_b, true);
        run_frame(&ctx, 2.2, text_event("B"), &editing_b, &mut buffer_b, false);
        // Ctrl+Z fires well before B's own "B" state has had a second to stabilize into its own
        // undo point.
        run_frame(&ctx, 2.3, ctrl_z, &editing_b, &mut buffer_b, false);

        assert_ne!(buffer_b, "A", "edit B's undo pulled in edit A's text");
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
