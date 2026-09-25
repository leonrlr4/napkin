//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title, the camera,
//! the scene editor and its overlay, the load-error banner and the GPU canvas itself.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use scene::editor::{Editor, Modifiers, PointerEvent, Tool};
use scene::env::SystemEnv;
use scene::file::NapkinView;

use crate::autosave::{Autosave, DocumentState, Trigger};
use crate::bench;
use crate::camera::{Camera, SceneRect, normalized_zoom};
use crate::edit_input::{self, EditorInput, PointerCapture};
use crate::input::{self, CanvasInput};
use crate::overlay::{self, OverlayColors};
use crate::pinch::{PinchListener, PinchTracker};
use crate::render::callback::{self, CanvasCallback};
use crate::render::color::render_color;
use crate::render::gpu::{CanvasFrame, CanvasRenderer};
use crate::stats::FrameStats;
use crate::storage::{self, Content};
use crate::theme::{self, Theme};
use crate::writer::{SaveJob, SaveWorker};

/// How long a notice stays on screen (spec §8).
const NOTICE_DURATION: Duration = Duration::from_secs(10);

/// `self.editor` is always `Some` past the `load_error` early return in `ui()`: `load_error`
/// is set exactly when the app was constructed without one (spec §8), and nothing ever clears
/// `editor` afterwards.
const EDITOR_INVARIANT: &str = "editor exists once load_error is None";

/// `--bench`'s progress: waiting for the first real frame (positive view size, camera
/// initialized), running the camera pan/zoom script, running the drag phase that follows it,
/// or finished (both summary lines printed and the window asked to close).
#[derive(Clone, Copy)]
enum BenchState {
    NotStarted,
    /// Driven by [`bench::camera_at`] from `start` (the wall-clock origin) and `base_camera`
    /// (the camera in effect when the script began).
    Camera {
        start: Instant,
        base_camera: Camera,
    },
    /// Driven by [`bench::drag_pointer_at`] from `start` (the wall-clock origin) and `target`
    /// (the dragged element's grab point in scene coordinates, fixed for the whole phase).
    /// `base_clones` is `Editor::scene_clones()` when the phase began, so the summary line
    /// reports only the clones the drag itself caused.
    Drag {
        start: Instant,
        target: [f64; 2],
        base_clones: u64,
    },
    Done,
}

pub struct NapkinApp {
    name: String,
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
    /// Set from `--bench`: drives the camera with [`bench::camera_at`], then the selection
    /// tool through a scripted drag with [`bench::drag_pointer_at`], then prints a summary for
    /// each phase and closes the window (spec §9.3, decision 11).
    bench: bool,
    /// `--bench`'s state machine.
    bench_state: BenchState,
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
    /// Where the open canvas is written; `None` means changes are never saved (spec §5.4,
    /// `--bench` and a missing `$HOME` with no file argument).
    path: Option<PathBuf>,
    /// Set when reloading the file after an external change finds it unparseable: the canvas
    /// keeps showing the last good scene, but stops accepting input and stops saving
    /// (spec §8). `None` on a load error at startup, which has no editor at all and uses
    /// `load_error` instead.
    unreadable: Option<String>,
    /// A message to show for [`NOTICE_DURATION`] after it was set (spec §8), such as which
    /// requested file could not be opened.
    notice: Option<(String, Instant)>,
    autosave: Autosave,
    /// `None` alongside `path`: nothing to write to.
    writer: Option<SaveWorker>,
    /// The mtime of `path` as last read or written, to detect an external change (spec §5.6).
    known_mtime: Option<SystemTime>,
    /// Bumped every time a reload replaces the scene outright, so the renderer drops its
    /// per-element caches instead of matching old and new elements by id.
    generation: u64,
}

impl NapkinApp {
    /// `content` is what [`storage::open_at_startup`] (or, for `--bench` and a missing
    /// `$HOME`, [`storage::load`]) found at `path`; `path` is `None` when nothing should ever
    /// be written. `notice` is shown for [`NOTICE_DURATION`], such as which requested file
    /// fell back to another one.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        name: String,
        path: Option<PathBuf>,
        content: Content,
        notice: Option<String>,
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
        let (editor, load_error, known_mtime) = match content {
            Content::Editable { file, mtime } => (Some(Editor::new(file, SystemEnv)), None, mtime),
            Content::Unreadable(message) => (None, Some(message), None),
        };
        let revision = editor.as_ref().map_or(0, |editor| editor.revision());
        let view = editor
            .as_ref()
            .and_then(|editor| stored_view(editor.file()))
            .unwrap_or(NapkinView {
                scroll_x: 0.0,
                scroll_y: 0.0,
                zoom: 1.0,
            });
        // A load error at startup has no editor and nothing is ever submitted to save, so no
        // worker thread is spawned for it even when `path` names the unreadable file.
        let writer = (path.is_some() && editor.is_some()).then(|| {
            let ctx = cc.egui_ctx.clone();
            SaveWorker::spawn(move || ctx.request_repaint())
        });
        NapkinApp {
            name,
            load_error,
            editor,
            capture: PointerCapture::default(),
            theme: load_theme(),
            camera: None,
            bench,
            bench_state: BenchState::NotStarted,
            stats: FrameStats::new(),
            show_stats: false,
            // Assume the window starts focused: `theme` was just loaded above, so the first
            // `ui()` call's `focused && !self.focused` check must not read it a second time
            // merely because `self.focused` starts at the type's default. If the window is
            // not actually focused yet, the next real focus transition still reloads it.
            focused: true,
            pinch,
            pinch_tracker: PinchTracker::default(),
            path,
            unreadable: None,
            notice: notice.map(|message| (message, Instant::now())),
            autosave: Autosave::new(revision, view),
            writer,
            known_mtime,
            generation: 0,
        }
    }

    /// Applies a background save's outcome: remembers the new mtime on success, then either
    /// way tells `autosave` the save finished, so it can track what's still unwritten and
    /// back off after a failure.
    fn record_save_result(&mut self, now: Instant, result: crate::writer::SaveResult) {
        match result {
            Ok(mtime) => {
                self.known_mtime = Some(mtime);
                self.autosave.finished(now, Ok(()));
            }
            Err(message) => self.autosave.finished(now, Err(message)),
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

/// `file`'s stored view, when its `scrollX`/`scrollY`/`zoom` are all finite and `zoom` is
/// positive (a hand-edited file could have anything in there). `None` for a missing or
/// malformed `appState.napkin`.
fn stored_view(file: &scene::SceneFile) -> Option<NapkinView> {
    let view = file.napkin_view()?;
    (view.scroll_x.is_finite()
        && view.scroll_y.is_finite()
        && view.zoom.is_finite()
        && view.zoom > 0.0)
        .then_some(view)
}

/// `appState.napkin` if present and valid, otherwise zoom 1 centered on the union of every
/// non-deleted element's (unrotated) placement rectangle, or the origin for an empty
/// document (decision 5).
///
/// `None` when `view_size` has zero (or negative) width or height: the canvas hasn't been
/// laid out with a real size yet, so there is nothing sensible to center on, and the caller
/// should keep waiting rather than latch onto a degenerate camera.
///
/// A stored `appState.napkin` ([`stored_view`]) has its `zoom` passed through
/// [`normalized_zoom`] so an out-of-range value (a hand-edited file, or a future Excalidraw
/// with a wider zoom range) still clamps to `MIN_ZOOM..=MAX_ZOOM` instead of producing a
/// degenerate transform. A missing or invalid stored view falls back to the bounds-centered
/// camera below.
fn initial_camera(file: &scene::SceneFile, view_size: [f64; 2]) -> Option<Camera> {
    if !(view_size[0] > 0.0 && view_size[1] > 0.0) {
        return None;
    }
    if let Some(view) = stored_view(file) {
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

/// `camera` as the view napkin stores in `appState.napkin`.
fn camera_view(camera: Camera) -> NapkinView {
    NapkinView {
        scroll_x: camera.scroll_x,
        scroll_y: camera.scroll_y,
        zoom: camera.zoom,
    }
}

/// Spec §5.6: reload when the file on disk changed since napkin last read or wrote it, no
/// save is running and no unsaved element change would be lost.
pub fn should_reload(
    known_mtime: Option<SystemTime>,
    disk_mtime: Option<SystemTime>,
    saving: bool,
    unsaved: bool,
) -> bool {
    if saving || unsaved {
        return false;
    }
    match disk_mtime {
        Some(disk_mtime) => known_mtime != Some(disk_mtime),
        None => false,
    }
}

impl eframe::App for NapkinApp {
    /// Stops the pinch dispatch thread before eframe disconnects the Wayland display it
    /// borrows from (see [`PinchListener`]'s doc comment), finishes any queued save and hands
    /// its result to `autosave`, then, if that still leaves an element or view change
    /// unwritten, saves once more directly on this thread (spec §5.5). A panic skips
    /// `on_exit` entirely, so nothing is saved after one (spec §8).
    fn on_exit(&mut self) {
        if let Some(pinch) = &mut self.pinch {
            pinch.stop();
        }
        let Some(writer) = self.writer.take() else {
            return;
        };
        let now = Instant::now();
        for result in writer.shutdown() {
            self.record_save_result(now, result);
        }
        if self.unreadable.is_some() {
            return;
        }
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let Some(camera) = self.camera else {
            return;
        };
        let path = self.path.clone().expect("a writer implies a save path");
        let view = camera_view(camera);
        let state = DocumentState {
            revision: editor.revision(),
            view,
            idle: editor.is_idle(),
        };
        if self.autosave.should_save(now, state, Trigger::Exit) {
            let job = SaveJob {
                path,
                file: editor.file().clone(),
                view,
            };
            if let Err(error) = crate::writer::save(&job) {
                eprintln!("napkin: {error}");
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        self.stats.frame_started(frame_start);

        let save_result = self.writer.as_ref().and_then(|writer| writer.try_result());
        if let Some(result) = save_result {
            self.record_save_result(frame_start, result);
        }

        let focused = ui.input(|i| i.focused);
        let gained_focus = focused && !self.focused;
        let lost_focus = !focused && self.focused;
        if gained_focus {
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
                    // `--bench` has no editor to drive a script with; without this, the
                    // window would sit open indefinitely instead of finishing the run.
                    if self.bench && !matches!(self.bench_state, BenchState::Done) {
                        println!("bench: no scene");
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        self.bench_state = BenchState::Done;
                    }
                    return;
                }

                let (_, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let view_size = [response.rect.width() as f64, response.rect.height() as f64];
                if self.camera.is_none() {
                    self.camera = initial_camera(
                        self.editor.as_ref().expect(EDITOR_INVARIANT).file(),
                        view_size,
                    );
                }
                // Still `None` on a zero-size first frame (window not yet laid out): wait
                // for a later frame with a real size instead of latching onto a degenerate
                // camera.
                let Some(camera) = self.camera.as_mut() else {
                    return;
                };

                if self.bench {
                    if matches!(self.bench_state, BenchState::NotStarted) {
                        // The script starts now, on the first frame with a real camera: reset
                        // the statistics so the summary only covers the script itself, not
                        // whatever startup frames came before it.
                        self.bench_state = BenchState::Camera {
                            start: frame_start,
                            base_camera: *camera,
                        };
                        self.stats = FrameStats::new();
                        self.stats.frame_started(frame_start);
                    }
                    match self.bench_state {
                        BenchState::Camera { start, base_camera } => {
                            let elapsed = start.elapsed().as_secs_f64();
                            match bench::camera_at(elapsed, base_camera, view_size) {
                                Some(next) => {
                                    *camera = next;
                                    ui.ctx().request_repaint();
                                }
                                None => {
                                    println!(
                                        "bench: frames {}, interval p99 {:.2} ms, cpu p99 {:.2} ms",
                                        self.stats.frames(),
                                        self.stats.interval_p99().unwrap_or(0.0),
                                        self.stats.cpu_p99().unwrap_or(0.0),
                                    );
                                    let editor = self.editor.as_mut().expect(EDITOR_INVARIANT);
                                    self.bench_state = match bench::drag_target(editor.file()) {
                                        Some((index, target)) => {
                                            let placement =
                                                editor.file().elements[index].placement().expect(
                                                    "drag_target only returns elements with a \
                                                     placement",
                                                );
                                            *camera = Camera::centered_on(
                                                SceneRect {
                                                    min: [placement.x, placement.y],
                                                    max: [
                                                        placement.x + placement.width,
                                                        placement.y + placement.height,
                                                    ],
                                                },
                                                view_size,
                                            );
                                            editor.set_tool(Tool::Selection);
                                            let base_clones = editor.scene_clones();
                                            editor.pointer_down(PointerEvent {
                                                at: target,
                                                modifiers: Modifiers::default(),
                                                zoom: camera.zoom,
                                            });
                                            // The drag phase gets its own statistics window,
                                            // same as the camera script's above.
                                            self.stats = FrameStats::new();
                                            self.stats.frame_started(frame_start);
                                            ui.ctx().request_repaint();
                                            BenchState::Drag {
                                                start: frame_start,
                                                target,
                                                base_clones,
                                            }
                                        }
                                        None => {
                                            println!("bench drag: no shape to drag");
                                            ui.ctx()
                                                .send_viewport_cmd(egui::ViewportCommand::Close);
                                            BenchState::Done
                                        }
                                    };
                                }
                            }
                        }
                        BenchState::Drag {
                            start,
                            target,
                            base_clones,
                        } => {
                            let elapsed = start.elapsed().as_secs_f64();
                            let editor = self.editor.as_mut().expect(EDITOR_INVARIANT);
                            match bench::drag_pointer_at(elapsed, target) {
                                Some(pos) => {
                                    editor.pointer_move(PointerEvent {
                                        at: pos,
                                        modifiers: Modifiers::default(),
                                        zoom: camera.zoom,
                                    });
                                    ui.ctx().request_repaint();
                                }
                                None => {
                                    // A full revolution ends back where it started.
                                    editor.pointer_up(PointerEvent {
                                        at: target,
                                        modifiers: Modifiers::default(),
                                        zoom: camera.zoom,
                                    });
                                    println!(
                                        "bench drag: frames {}, interval p99 {:.2} ms, cpu p99 \
                                         {:.2} ms, scene clones {}",
                                        self.stats.frames(),
                                        self.stats.interval_p99().unwrap_or(0.0),
                                        self.stats.cpu_p99().unwrap_or(0.0),
                                        editor.scene_clones() - base_clones,
                                    );
                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                    self.bench_state = BenchState::Done;
                                }
                            }
                        }
                        BenchState::NotStarted | BenchState::Done => {}
                    }
                    if matches!(self.bench_state, BenchState::Done) {
                        return;
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
                        // Regaining focus: reread the file if it changed while napkin was away
                        // and nothing here would be lost (spec §5.6). The camera does not
                        // move; a failed reparse stops input and saving until a later reload
                        // (on a later focus gain) succeeds.
                        if gained_focus && let Some(path) = self.path.clone() {
                            let disk_mtime = storage::modified(&path).unwrap_or_else(|error| {
                                eprintln!("napkin: {}: {error}", path.display());
                                None
                            });
                            let unsaved = self.autosave.has_unsaved_changes(editor.revision());
                            if should_reload(
                                self.known_mtime,
                                disk_mtime,
                                self.autosave.in_flight(),
                                unsaved,
                            ) {
                                match storage::load(&path) {
                                    storage::Loaded::Parsed { file, mtime } => {
                                        editor.replace_file(file);
                                        self.generation += 1;
                                        self.autosave
                                            .reset(editor.revision(), camera_view(*camera));
                                        self.known_mtime = Some(mtime);
                                        self.unreadable = None;
                                    }
                                    storage::Loaded::Invalid(message) => {
                                        self.unreadable = Some(message);
                                    }
                                    storage::Loaded::Missing => {}
                                }
                            }
                        }

                        if self.unreadable.is_none() {
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
                        }
                        ui.ctx().set_cursor_icon(if panning {
                            egui::CursorIcon::Grab
                        } else {
                            edit_input::cursor_icon(editor.cursor())
                        });

                        if self.unreadable.is_none()
                            && let (Some(path), Some(writer)) =
                                (self.path.clone(), self.writer.as_ref())
                        {
                            let view = camera_view(*camera);
                            let state = DocumentState {
                                revision: editor.revision(),
                                view,
                                idle: editor.is_idle(),
                            };
                            if self
                                .autosave
                                .should_save(frame_start, state, Trigger::Frame)
                            {
                                self.autosave.started(state);
                                writer.submit(SaveJob {
                                    path: path.clone(),
                                    file: editor.file().clone(),
                                    view,
                                });
                            }
                            if lost_focus
                                && self
                                    .autosave
                                    .should_save(frame_start, state, Trigger::FocusLost)
                            {
                                self.autosave.started(state);
                                writer.submit(SaveJob {
                                    path,
                                    file: editor.file().clone(),
                                    view,
                                });
                            }
                        }
                    }
                    if let Some(wake_after) = self.autosave.wake_after(frame_start) {
                        ui.ctx().request_repaint_after(wake_after);
                    }
                }

                let file = self.editor.as_ref().expect(EDITOR_INVARIANT).file().clone();

                let background = render_color(file.view_background_color(), self.theme.dark);
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
                let frame = CanvasFrame {
                    file,
                    camera: *camera,
                    size_px,
                    pixels_per_point,
                    dark: self.theme.dark,
                    generation: self.generation,
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

                    if let Some(message) = &self.unreadable {
                        egui::Area::new(egui::Id::new("napkin-unreadable"))
                            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 28.0))
                            .show(ui.ctx(), |ui| {
                                ui.colored_label(egui::Color32::RED, message);
                            });
                    }
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
                ui.label(&self.name);
            });

        if let Some(error) = self.autosave.error() {
            let message = format!("Save failed: {error}");
            egui::Area::new(egui::Id::new("napkin-save-error"))
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 28.0))
                .show(ui.ctx(), |ui| {
                    ui.colored_label(egui::Color32::RED, message);
                });
        } else if let Some((message, shown_at)) = self.notice.clone() {
            match NOTICE_DURATION.checked_sub(shown_at.elapsed()) {
                Some(remaining) => {
                    egui::Area::new(egui::Id::new("napkin-notice"))
                        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 28.0))
                        .show(ui.ctx(), |ui| {
                            ui.label(message);
                        });
                    ui.ctx().request_repaint_after(remaining);
                }
                None => self.notice = None,
            }
        }

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
    use std::time::{Duration, SystemTime};

    use scene::file::NapkinView;

    use super::*;
    use crate::camera::MAX_ZOOM;

    #[test]
    fn reload_only_when_the_disk_changed_and_nothing_would_be_lost() {
        let t = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        assert!(should_reload(Some(t(10)), Some(t(11)), false, false));
        assert!(
            should_reload(Some(t(10)), Some(t(9)), false, false),
            "an older file restored from a backup"
        );
        assert!(
            should_reload(None, Some(t(5)), false, false),
            "the file appeared"
        );
        assert!(!should_reload(Some(t(10)), Some(t(10)), false, false));
        assert!(
            !should_reload(Some(t(10)), None, false, false),
            "deleted: keep editing, the next save recreates it"
        );
        assert!(!should_reload(Some(t(10)), Some(t(11)), true, false));
        assert!(!should_reload(Some(t(10)), Some(t(11)), false, true));
    }

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
