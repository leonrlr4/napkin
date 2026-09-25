//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title, the camera,
//! the scene editor and its overlay, the load-error banner and the GPU canvas itself.

use std::time::Instant;

use eframe::egui;
use scene::editor::{Editor, Tool};
use scene::env::SystemEnv;

use crate::bench;
use crate::camera::{Camera, SceneRect, normalized_zoom};
use crate::document::Document;
use crate::edit_input::{self, EditorInput, PointerCapture};
use crate::input::{self, CanvasInput};
use crate::overlay::{self, OverlayColors};
use crate::pinch::{PinchListener, PinchTracker};
use crate::render::callback::{self, CanvasCallback};
use crate::render::color::render_color;
use crate::render::gpu::{CanvasFrame, CanvasRenderer};
use crate::stats::FrameStats;
use crate::theme::{self, Theme};

/// The camera and wall-clock origin `--bench`'s script runs from, captured once the first
/// real frame (positive view size, camera initialized) arrives: `bench::camera_at` needs a
/// concrete starting camera and wall-clock origin to compute each later frame's camera from.
struct BenchRun {
    start: Instant,
    base_camera: Camera,
}

pub struct NapkinApp {
    document: Document,
    load_error: Option<String>,
    /// `Some` whenever `load_error` is `None`: the scene editor driving pointer and keyboard
    /// input, undo/redo and the selection overlay. `None` on a load error, same as M3's
    /// read-only banner.
    editor: Option<Editor<SystemEnv>>,
    /// The active primary-button press, carried across frames so a drag that leaves the canvas
    /// (or the window loses focus mid-drag) still reaches the editor as one gesture.
    capture: PointerCapture,
    theme: Theme,
    /// `None` until the first frame, when the canvas's `view_size` becomes known and the
    /// camera is set from `appState.napkin` or the document's bounds (decision 5).
    camera: Option<Camera>,
    /// Set from `--bench`: drives the camera with [`bench::camera_at`] once the first real
    /// frame arrives, then prints a summary and closes the window (spec §9.3, decision 11).
    bench: bool,
    /// `--bench`'s script state, `None` until the first real frame starts it.
    bench_run: Option<BenchRun>,
    /// Set once the closing `bench:` summary line has been printed, so a frame or two of
    /// shutdown latency after `ViewportCommand::Close` cannot print it twice.
    bench_done: bool,
    /// Rolling frame-time statistics for the hidden panel and `--bench`'s summary line.
    stats: FrameStats,
    /// Toggled by F12; the panel is hidden by default (spec §9.3).
    show_stats: bool,
    /// Last frame's `ui.input(|i| i.focused)`, to detect the false-to-true edge that
    /// triggers a theme re-read (spec §7.6).
    focused: bool,
    /// `None` when the compositor isn't Wayland or lacks `zwp_pointer_gestures_v1`; the
    /// reason is logged once in [`NapkinApp::new`] and the canvas falls back to Ctrl+wheel
    /// zoom.
    pinch: Option<PinchListener>,
    /// Turns this listener's begin-relative `scale` into per-event zoom factors.
    pinch_tracker: PinchTracker,
}

impl NapkinApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        document: Document,
        load_error: Option<String>,
        bench: bool,
    ) -> NapkinApp {
        if let Some(render_state) = &cc.wgpu_render_state {
            callback::install(render_state);
        }
        let pinch = PinchListener::start(cc)
            .inspect_err(|reason| {
                eprintln!("napkin: touchpad pinch zoom unavailable: {reason}");
            })
            .ok();
        let editor = load_error
            .is_none()
            .then(|| Editor::new((*document.file).clone(), SystemEnv));
        NapkinApp {
            document,
            load_error,
            editor,
            capture: PointerCapture::default(),
            theme: load_theme(),
            camera: None,
            bench,
            bench_run: None,
            bench_done: false,
            stats: FrameStats::new(),
            show_stats: false,
            // Assume the window starts focused: `theme` was just loaded above, so the first
            // `ui()` call's `focused && !self.focused` check must not read it a second time
            // merely because `self.focused` starts at the type's default. If the window is
            // not actually focused yet, the next real focus transition still reloads it.
            focused: true,
            pinch,
            pinch_tracker: PinchTracker::default(),
        }
    }
}

/// The tool's lowercase name, as shown above the canvas.
fn tool_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Selection => "selection",
        Tool::Hand => "hand",
        Tool::Rectangle => "rectangle",
        Tool::Diamond => "diamond",
        Tool::Ellipse => "ellipse",
        Tool::Arrow => "arrow",
        Tool::Line => "line",
        Tool::Freedraw => "freedraw",
    }
}

/// `appState.napkin` if present and valid, otherwise zoom 1 centered on the union of every
/// non-deleted element's (unrotated) placement rectangle, or the origin for an empty
/// document (decision 5).
///
/// `None` when `view_size` has zero (or negative) width or height: the canvas hasn't been
/// laid out with a real size yet, so there is nothing sensible to center on, and the caller
/// should keep waiting rather than latch onto a degenerate camera.
///
/// A stored `appState.napkin` is used only when its `scrollX`/`scrollY`/`zoom` are all
/// finite and `zoom` is positive; `zoom` is then passed through [`normalized_zoom`] so an
/// out-of-range value (a hand-edited file, or a future Excalidraw with a wider zoom range)
/// still clamps to `MIN_ZOOM..=MAX_ZOOM` instead of producing a degenerate transform. A
/// non-finite or non-positive stored zoom falls back to the bounds-centered camera below,
/// same as a missing `appState.napkin`.
fn initial_camera(file: &scene::SceneFile, view_size: [f64; 2]) -> Option<Camera> {
    if !(view_size[0] > 0.0 && view_size[1] > 0.0) {
        return None;
    }
    if let Some(view) = file.napkin_view()
        && view.scroll_x.is_finite()
        && view.scroll_y.is_finite()
        && view.zoom.is_finite()
        && view.zoom > 0.0
    {
        return Some(Camera {
            scroll_x: view.scroll_x,
            scroll_y: view.scroll_y,
            zoom: normalized_zoom(view.zoom),
        });
    }
    let bounds = file
        .elements
        .iter()
        .filter(|element| !element.is_deleted())
        .filter_map(|element| element.placement())
        .map(|p| SceneRect {
            min: [p.x, p.y],
            max: [p.x + p.width, p.y + p.height],
        })
        .reduce(|a, b| a.union(&b))
        .unwrap_or(SceneRect {
            min: [0.0, 0.0],
            max: [0.0, 0.0],
        });
    Some(Camera::centered_on(bounds, view_size))
}

/// Reads the omarchy theme file, falling back to [`Theme::builtin`] and logging the reason
/// when it is missing or invalid (spec §7.6, spec §8).
fn load_theme() -> Theme {
    let Some(path) = theme::omarchy_theme_path() else {
        return Theme::builtin();
    };
    match Theme::load(&path) {
        Ok(theme) => theme,
        Err(error) => {
            eprintln!("napkin: {error}");
            Theme::builtin()
        }
    }
}

impl eframe::App for NapkinApp {
    /// Stops the pinch dispatch thread before eframe disconnects the Wayland display it
    /// borrows from (see [`PinchListener`]'s doc comment).
    fn on_exit(&mut self) {
        if let Some(pinch) = &mut self.pinch {
            pinch.stop();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        self.stats.frame_started(frame_start);

        let focused = ui.input(|i| i.focused);
        if focused && !self.focused {
            self.theme = load_theme();
        }
        self.focused = focused;
        ui.ctx().set_visuals(self.theme.visuals());

        if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.show_stats = !self.show_stats;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.theme.background))
            .show(ui, |ui| {
                if let Some(error) = &self.load_error {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(egui::Color32::RED, error);
                    });
                    return;
                }

                let (_, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let view_size = [response.rect.width() as f64, response.rect.height() as f64];
                if self.camera.is_none() {
                    self.camera = initial_camera(&self.document.file, view_size);
                }
                // Still `None` on a zero-size first frame (window not yet laid out): wait
                // for a later frame with a real size instead of latching onto a degenerate
                // camera.
                let Some(camera) = self.camera.as_mut() else {
                    return;
                };

                if self.bench {
                    if self.bench_run.is_none() {
                        // The script starts now, on the first frame with a real camera: reset
                        // the statistics so the summary only covers the script itself, not
                        // whatever startup frames came before it.
                        self.bench_run = Some(BenchRun {
                            start: frame_start,
                            base_camera: *camera,
                        });
                        self.stats = FrameStats::new();
                        self.stats.frame_started(frame_start);
                    }
                    let run = self.bench_run.as_ref().expect("set above");
                    let elapsed = run.start.elapsed().as_secs_f64();
                    match bench::camera_at(elapsed, run.base_camera, view_size) {
                        Some(next) => {
                            *camera = next;
                            ui.ctx().request_repaint();
                        }
                        None => {
                            if !self.bench_done {
                                println!(
                                    "bench: frames {}, interval p99 {:.2} ms, cpu p99 {:.2} ms",
                                    self.stats.frames(),
                                    self.stats.interval_p99().unwrap_or(0.0),
                                    self.stats.cpu_p99().unwrap_or(0.0),
                                );
                                self.bench_done = true;
                            }
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                            return;
                        }
                    }
                } else {
                    let space_down = ui.input(|i| i.key_down(egui::Key::Space));
                    let hand_tool = self.editor.as_ref().is_some_and(|e| e.tool() == Tool::Hand);
                    let mut canvas_input = CanvasInput::from_egui(ui, &response, hand_tool);
                    if let Some(pinch) = &self.pinch {
                        for event in pinch.events() {
                            if let Some(factor) = self.pinch_tracker.factor(event) {
                                canvas_input.pinch =
                                    Some(canvas_input.pinch.map_or(factor, |f| f * factor));
                            }
                        }
                    }
                    // The input that changed the camera already triggered this repaint, so
                    // no `request_repaint` call is needed here.
                    input::apply(camera, &canvas_input);

                    let panning = space_down || hand_tool;
                    if let Some(editor) = self.editor.as_mut() {
                        let events = ui.input(|i| i.events.clone());
                        let frame_input = edit_input::FrameInput {
                            events: &events,
                            canvas: response.rect,
                            camera: *camera,
                            modifiers: ui.input(|i| i.modifiers),
                            panning,
                            keyboard_taken: ui.ctx().egui_wants_keyboard_input(),
                            focused: self.focused,
                        };
                        for action in edit_input::translate(&frame_input, &mut self.capture) {
                            match action {
                                EditorInput::Down(event) => editor.pointer_down(event),
                                EditorInput::Move(event) => editor.pointer_move(event),
                                EditorInput::Up(event) => editor.pointer_up(event),
                                EditorInput::Tool(tool) => editor.set_tool(tool),
                                EditorInput::Command(command) => {
                                    editor.command(command);
                                }
                            }
                        }
                        ui.ctx().set_cursor_icon(if panning {
                            egui::CursorIcon::Grab
                        } else {
                            edit_input::cursor_icon(editor.cursor())
                        });
                    }
                }

                let background =
                    render_color(self.document.file.view_background_color(), self.theme.dark);
                ui.painter().rect_filled(
                    response.rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(
                        (background[0] * 255.0).round() as u8,
                        (background[1] * 255.0).round() as u8,
                        (background[2] * 255.0).round() as u8,
                        (background[3] * 255.0).round() as u8,
                    ),
                );

                let pixels_per_point = ui.ctx().pixels_per_point();
                let size_px = [
                    (response.rect.width() * pixels_per_point).round() as u32,
                    (response.rect.height() * pixels_per_point).round() as u32,
                ];
                let file = self.editor.as_ref().map_or_else(
                    || self.document.file.clone(),
                    |editor| editor.file().clone(),
                );
                let frame = CanvasFrame {
                    file,
                    camera: *camera,
                    size_px,
                    pixels_per_point,
                    dark: self.theme.dark,
                    // The editor never replaces its own scene wholesale (only a reload would),
                    // so every frame is still the one it started with.
                    generation: 0,
                };
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    response.rect,
                    CanvasCallback { frame },
                ));

                if let Some(editor) = self.editor.as_mut() {
                    let overlay = editor.overlay(camera.zoom);
                    let colors = OverlayColors::from_theme(&self.theme);
                    ui.painter().extend(overlay::shapes(
                        &overlay,
                        camera,
                        response.rect.min,
                        colors,
                    ));

                    egui::Area::new(egui::Id::new("napkin-current-tool"))
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
                        .show(ui.ctx(), |ui| {
                            ui.small(tool_label(editor.tool()));
                        });
                }
            });

        // The renderer's stats are from the *previous* frame's `prepare` call: this frame's
        // `prepare` (via the paint callback queued above) has not run yet, so `prepare_time` is
        // folded into this frame's CPU sample rather than the frame it actually measures.
        let render_stats = frame
            .wgpu_render_state()
            .and_then(|render_state| {
                let renderer = render_state.renderer.read();
                renderer
                    .callback_resources
                    .get::<CanvasRenderer>()
                    .map(CanvasRenderer::stats)
            })
            .unwrap_or_default();
        self.stats
            .cpu_finished(frame_start.elapsed() + render_stats.prepare_time);

        egui::Area::new(egui::Id::new("napkin-document-name"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.label(&self.document.name);
            });

        if self.show_stats {
            egui::Area::new(egui::Id::new("napkin-stats-panel"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -8.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(format!("frames {}", self.stats.frames()));
                        ui.label(format_ms("interval p99", self.stats.interval_p99()));
                        ui.label(format_ms("cpu p99", self.stats.cpu_p99()));
                        ui.label(format!("drawn elements {}", render_stats.drawn_elements));
                        ui.label(format!("cached meshes {}", render_stats.cached_meshes));
                        ui.label(format!(
                            "buffer vertices {}/{}",
                            render_stats.buffer_vertices_used,
                            render_stats.buffer_vertices_capacity
                        ));
                        if let Some(editor) = &self.editor {
                            ui.label(format!("scene clones {}", editor.scene_clones()));
                        }
                    });
                });
        }
    }
}

/// `"{label} {value:.2} ms"`, or `"{label} -"` when there is no sample yet.
fn format_ms(label: &str, value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{label} {value:.2} ms"),
        None => format!("{label} -"),
    }
}

#[cfg(test)]
mod tests {
    use scene::file::NapkinView;

    use super::*;
    use crate::camera::MAX_ZOOM;

    #[test]
    fn zero_size_view_has_no_initial_camera() {
        let file = scene::SceneFile::new();
        assert_eq!(initial_camera(&file, [0.0, 600.0]), None);
        assert_eq!(initial_camera(&file, [800.0, 0.0]), None);
    }

    #[test]
    fn centers_on_non_deleted_element_bounds() {
        let text = r#"{"type":"excalidraw","elements":[
            {"type":"rectangle","id":"r","x":0,"y":0,"width":200,"height":100},
            {"type":"rectangle","id":"d","x":1000,"y":1000,"width":10,"height":10,"isDeleted":true}
        ]}"#;
        let file = scene::SceneFile::from_json_str(text).expect("parses");
        let camera = initial_camera(&file, [800.0, 600.0]).expect("positive view size");
        assert_eq!(camera.zoom, 1.0);
        // Bounds are [0, 0]..[200, 100] (the deleted element is excluded), center [100, 50].
        assert_eq!(camera.scene_to_view([100.0, 50.0]), [400.0, 300.0]);
    }

    #[test]
    fn uses_a_valid_stored_napkin_view() {
        let mut file = scene::SceneFile::new();
        file.set_napkin_view(NapkinView {
            scroll_x: 5.0,
            scroll_y: -3.0,
            zoom: 2.0,
        });
        let camera = initial_camera(&file, [800.0, 600.0]).expect("positive view size");
        assert_eq!(
            camera,
            Camera {
                scroll_x: 5.0,
                scroll_y: -3.0,
                zoom: 2.0
            }
        );
    }

    #[test]
    fn normalizes_an_out_of_range_stored_zoom() {
        let mut file = scene::SceneFile::new();
        file.set_napkin_view(NapkinView {
            scroll_x: 1.0,
            scroll_y: 2.0,
            zoom: 1000.0,
        });
        let camera = initial_camera(&file, [800.0, 600.0]).expect("positive view size");
        assert_eq!(camera.zoom, MAX_ZOOM);
        assert_eq!((camera.scroll_x, camera.scroll_y), (1.0, 2.0));
    }

    #[test]
    fn falls_back_to_bounds_when_stored_zoom_is_not_positive() {
        let mut file = scene::SceneFile::new();
        file.set_napkin_view(NapkinView {
            scroll_x: 1.0,
            scroll_y: 2.0,
            zoom: 0.0,
        });
        let camera = initial_camera(&file, [800.0, 600.0]).expect("positive view size");
        // No elements either: falls back to the origin-centered default, ignoring the
        // invalid stored scroll/zoom entirely.
        assert_eq!(
            camera,
            Camera {
                scroll_x: 400.0,
                scroll_y: 300.0,
                zoom: 1.0
            }
        );
    }
}
