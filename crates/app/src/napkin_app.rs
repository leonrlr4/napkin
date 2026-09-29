//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title, the camera,
//! the scene editor and its overlay, the load-error banner and the GPU canvas itself.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use scene::editor::{Editor, Modifiers, PointerEvent, Tool};
use scene::env::SystemEnv;
use scene::file::NapkinView;
use scene::text::TextMeasure;

use crate::agent;
use crate::autosave::{Autosave, DocumentState, Trigger};
use crate::bench;
use crate::camera::{Camera, SceneRect, normalized_zoom};
use crate::control::Response;
use crate::control::handler::{self, Session, handle};
use crate::control::render::Rasterize;
use crate::control::server::{BindError, Incoming, Server, socket_path};
use crate::edit_input::{self, EditorInput, PointerCapture};
use crate::fonts;
use crate::input::{self, CanvasInput};
use crate::overlay::{self, OverlayColors};
use crate::pinch::{PinchListener, PinchTracker};
use crate::properties_panel::{self, PanelColors};
use crate::render::callback::{self, CanvasCallback};
use crate::render::color::render_color;
use crate::render::gpu::{CanvasFrame, CanvasRenderer};
use crate::render::offscreen::GpuRasterizer;
use crate::render::text::FontMeasure;
use crate::stats::FrameStats;
use crate::storage::{self, Content};
use crate::text_edit::{self, TextEditOutcome};
use crate::theme::{self, Theme};
use crate::toolbar::{self, ToolbarColors};
use crate::writer::{SaveJob, SaveWorker};

/// How often a mutating request at the front of the queue re-checks `Editor::is_idle` while a
/// user gesture is in progress: frequent enough that the request runs soon after the gesture
/// ends, without polling on every single frame.
const PENDING_REQUEST_RETRY: Duration = Duration::from_millis(50);

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
    /// The open canvas's path, split for the corner label and the window title.
    display: storage::DisplayPath,
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
    /// Set on a focus gain, cleared once the reload check in [`NapkinApp::ui`] actually runs.
    /// A save in flight at the moment focus returns defers the check rather than skipping it:
    /// without this flag, that single frame's `gained_focus` edge would be the only chance to
    /// notice an external change, and the next autosave would silently overwrite it.
    reload_check_pending: bool,
    /// `None` in `--bench`, without `$XDG_RUNTIME_DIR`, or when another napkin owns the socket.
    control: Option<Server>,
    /// Requests in arrival order; a mutating one at the front waits for `Editor::is_idle`.
    pending: VecDeque<Incoming>,
    /// Built at startup rather than lazily: `fonts::install` needs its font database to look up
    /// the system CJK font, and building a second `FontSystem` just for that would scan system
    /// fonts twice.
    measure: Option<FontMeasure>,
    /// The renderer `render` requests draw with (its own format, `offscreen::FORMAT`).
    offscreen: Option<CanvasRenderer>,
    /// The canvas size in points, from the latest laid-out frame.
    canvas_size: [f64; 2],
    /// The text-edit overlay's live buffer, `Some` for exactly as long as
    /// `Editor::text_editing()` is: created (from its `text`) the frame editing starts, mutated
    /// in place by the `TextEdit` widget every frame after, and taken and committed whenever
    /// editing ends, however it ends (the overlay's own Ctrl+Enter/Escape/blur, a canvas click
    /// or tool change, losing window focus, or exiting).
    text_edit_buffer: Option<String>,
}

/// A [`Rasterize`] for a `render` request that arrives before the first frame has a GPU render
/// state (`frame.wgpu_render_state()` is `None` until eframe's wgpu backend is ready).
struct NoGpu;

/// Stands in for `Session::measure` on a request that never reads it, so building the real
/// `FontMeasure` (which sets up its own `cosmic_text::FontSystem`) can wait for a request
/// that actually needs to lay out text.
struct NoMeasure;

impl TextMeasure for NoMeasure {
    fn line_width(&mut self, _line: &str, _font_family: f64, _font_size: f64) -> f64 {
        unreachable!("only `apply` measures text, and it always gets the real FontMeasure")
    }
}

impl Rasterize for NoGpu {
    fn rasterize(
        &mut self,
        _scene: std::sync::Arc<scene::SceneFile>,
        _camera: Camera,
        _size_px: [u32; 2],
        _dark: bool,
    ) -> Result<Vec<u8>, String> {
        Err("no GPU".to_string())
    }
}

/// Binds the control socket, unless `bench` is set (spec §9.3: `--bench` never opens it). `None`
/// without `$XDG_RUNTIME_DIR`/`$NAPKIN_SOCKET` and no notice; `None` with a notice when the
/// socket exists but cannot be used, either because another napkin already owns it or because
/// binding failed outright (logged to stderr too, since that failure has no other visible cause).
fn bind_control(cc: &eframe::CreationContext<'_>, bench: bool) -> (Option<Server>, Option<String>) {
    if bench {
        return (None, None);
    }
    let Some(path) = socket_path() else {
        return (None, None);
    };
    let ctx = cc.egui_ctx.clone();
    match Server::bind(&path, move || ctx.request_repaint()) {
        Ok(server) => (Some(server), None),
        Err(BindError::AlreadyRunning) => (
            None,
            Some("another napkin owns the control socket; napkin commands go to it".to_string()),
        ),
        Err(BindError::Io(error)) => {
            eprintln!("napkin: control socket unavailable: {error}");
            (None, Some(format!("control socket unavailable: {error}")))
        }
    }
}

impl NapkinApp {
    /// `content` is what [`storage::open_at_startup`] (or, for `--bench` and a missing
    /// `$HOME`, [`storage::load`]) found at `path`; `path` is `None` when nothing should ever
    /// be written. `notice` is shown for [`NOTICE_DURATION`], such as which requested file
    /// fell back to another one; a control socket bind failure takes priority over it,
    /// so the rarer, more actionable message isn't hidden by a startup one.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        display: storage::DisplayPath,
        path: Option<PathBuf>,
        content: Content,
        notice: Option<String>,
        bench: bool,
    ) -> NapkinApp {
        if let Some(render_state) = &cc.wgpu_render_state {
            callback::install(render_state);
        }
        // Built here, and reused for `measure` below, so registering napkin's fonts in egui
        // doesn't scan the system's fonts a second time just to find the CJK fallback.
        let mut font_measure = FontMeasure::new();
        fonts::install(&cc.egui_ctx, font_measure.db_mut());
        let (control, control_notice) = bind_control(cc, bench);
        let notice = control_notice.or(notice);
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
            display,
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
            reload_check_pending: false,
            control,
            pending: VecDeque::new(),
            measure: Some(font_measure),
            offscreen: None,
            canvas_size: [0.0, 0.0],
            text_edit_buffer: None,
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

    /// Serves requests from the front of `self.pending`, in the order they arrived, until the
    /// queue is empty or the front one has to wait. A mutating request (`apply`) on a
    /// read-write canvas waits for `editor.is_idle()` rather than running mid-gesture or being
    /// skipped over, so it never interrupts something the user is dragging: it is left at the
    /// front, a repaint is requested after [`PENDING_REQUEST_RETRY`] so the wait is retried
    /// without new input, and nothing behind it runs out of order. Every request actually
    /// served gets a fresh `Session` built from the current editor, save and camera state.
    fn serve_requests(&mut self, frame: &eframe::Frame, ctx: &egui::Context, camera: Camera) {
        loop {
            let Some(incoming) = self.pending.front() else {
                return;
            };
            if incoming.is_expired(Instant::now()) {
                // The client already gave up and read a timeout error off the socket, and may
                // have retried by now; running this late reply would apply it a second time.
                self.pending.pop_front();
                continue;
            }
            let Some(editor) = self.editor.as_ref() else {
                // No `Editor` at all: `ui`'s load-error branch already drains `self.pending`
                // every frame in that case, so this is never actually reached; kept as a guard
                // rather than an `expect` since serving requests has nothing to do with that
                // invariant.
                return;
            };
            if handler::is_mutating(&incoming.request)
                && self.unreadable.is_none()
                && !editor.is_idle()
            {
                ctx.request_repaint_after(PENDING_REQUEST_RETRY);
                return;
            }

            let incoming = self
                .pending
                .pop_front()
                .expect("front just matched Some above");
            let path = self.path.as_deref();
            let readonly = self.unreadable.as_deref();
            let save_error = self.autosave.error();
            let editor = self.editor.as_mut().expect(EDITOR_INVARIANT);
            let unsaved = self.autosave.has_unsaved_changes(editor.revision());
            let mut no_measure = NoMeasure;
            let measure: &mut dyn TextMeasure = if handler::is_mutating(&incoming.request) {
                self.measure.get_or_insert_with(FontMeasure::new)
            } else {
                &mut no_measure
            };
            let mut no_gpu = NoGpu;
            let render_state = frame.wgpu_render_state();
            let mut gpu_rasterizer;
            let rasterizer: &mut dyn Rasterize = match &render_state {
                Some(render_state) => {
                    gpu_rasterizer = GpuRasterizer {
                        device: &render_state.device,
                        queue: &render_state.queue,
                        renderer: &mut self.offscreen,
                    };
                    &mut gpu_rasterizer
                }
                None => &mut no_gpu,
            };
            let mut session = Session {
                editor,
                path,
                unsaved,
                save_error,
                readonly,
                camera,
                canvas_size: self.canvas_size,
                measure,
                dark: self.theme.dark,
                rasterizer,
            };
            let response = handle(&mut session, &incoming.request);
            incoming.reply(response);
        }
    }
}

/// Whether `events` holds a fresh (non-repeat) `Ctrl+K`/`Cmd+K` press this frame. Only the
/// initial press opens a terminal; holding the key down must not launch a second one from the
/// OS's key-repeat events.
fn ctrl_k_just_pressed(events: &[egui::Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: egui::Key::K,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if modifiers.command
        )
    })
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

/// Whether a reload check queued by a focus gain ([`NapkinApp::reload_check_pending`]) should
/// run this frame: only once no save is in flight. A pending check that finds a save running
/// stays pending instead of being dropped, so the caller must keep asking (by requesting a
/// repaint) until this returns `true`.
fn reload_check_ready(pending: bool, saving: bool) -> bool {
    pending && !saving
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

/// A read-only canvas (a failed reload) whose file has since been deleted becomes editable
/// again: nothing on disk is left to protect, and the next save recreates the file.
pub fn should_clear_unreadable(unreadable: bool, disk_mtime: Option<SystemTime>) -> bool {
    unreadable && disk_mtime.is_none()
}

/// Ends a text edit in progress by committing `buffer`'s current contents, if there is one: a
/// no-op once `editor.text_editing()` is already `None`, so every caller below can run this
/// unconditionally instead of checking first. Takes each field it needs rather than `&mut self`
/// so it can run from inside a loop that already holds `self.editor` borrowed mutably.
fn commit_pending_text_edit(
    editor: &mut Editor<SystemEnv>,
    buffer: &mut Option<String>,
    measure: &mut Option<FontMeasure>,
) {
    if editor.text_editing().is_none() {
        return;
    }
    let Some(text) = buffer.take() else {
        return;
    };
    let measure = measure.get_or_insert_with(FontMeasure::new);
    editor.commit_text(&text, measure);
}

impl eframe::App for NapkinApp {
    /// Stops the pinch dispatch thread before eframe disconnects the Wayland display it
    /// borrows from (see [`PinchListener`]'s doc comment), finishes any queued save and hands
    /// its result to `autosave`, then, if that still leaves an element or view change
    /// unwritten, saves once more directly on this thread (spec §5.5). A panic skips
    /// `on_exit` entirely, so nothing is saved after one (spec §8).
    fn on_exit(&mut self) {
        while let Some(incoming) = self.pending.pop_front() {
            incoming.reply(Response::error("napkin is closing"));
        }
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
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let Some(camera) = self.camera else {
            return;
        };
        // An in-progress multi-point line's cursor-following point is already in `file` but
        // not yet reflected in `revision`; without this, exiting would either save that
        // uncommitted point as if confirmed, or drop it entirely (spec §8). A text edit in
        // progress needs the same treatment: `finish_pending_gesture` does not touch it, so it
        // is committed here directly instead of being dropped.
        editor.finish_pending_gesture();
        commit_pending_text_edit(editor, &mut self.text_edit_buffer, &mut self.measure);
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

        if let Some(control) = &self.control {
            while let Some(incoming) = control.try_recv() {
                self.pending.push_back(incoming);
            }
        }

        let save_result = self.writer.as_ref().and_then(|writer| writer.try_result());
        if let Some(result) = save_result {
            self.record_save_result(frame_start, result);
        }

        let focused = ui.input(|i| i.focused);
        let gained_focus = focused && !self.focused;
        let lost_focus = !focused && self.focused;
        if gained_focus {
            self.theme = load_theme();
            self.reload_check_pending = true;
        }
        self.focused = focused;
        ui.ctx().set_visuals(self.theme.visuals());

        if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.show_stats = !self.show_stats;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.theme.background))
            .show(ui, |ui| {
                if let Some(error) = self.load_error.clone() {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(egui::Color32::RED, &error);
                    });
                    // There is no `Editor` at all to serve requests against; every request
                    // gets the same answer instead of waiting in the queue forever.
                    let message = format!("napkin could not open the canvas: {error}");
                    while let Some(incoming) = self.pending.pop_front() {
                        incoming.reply(Response::error(message.clone()));
                    }
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
                self.canvas_size = view_size;
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
                        // succeeds. A save in flight when focus returns defers the check
                        // (`reload_check_pending` stays set) instead of skipping it outright,
                        // and asks for a repaint so it runs again once the save finishes,
                        // rather than waiting for the next focus gain and letting that save
                        // (or the next autosave) overwrite the external change unnoticed.
                        if let Some(path) = self.path.clone() {
                            let saving = self.autosave.in_flight();
                            if reload_check_ready(self.reload_check_pending, saving) {
                                self.reload_check_pending = false;
                                let disk_mtime = storage::modified(&path).unwrap_or_else(|error| {
                                    eprintln!("napkin: {}: {error}", path.display());
                                    None
                                });
                                let unsaved = self.autosave.has_unsaved_changes(editor.revision());
                                if should_reload(self.known_mtime, disk_mtime, saving, unsaved) {
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
                                } else if should_clear_unreadable(
                                    self.unreadable.is_some(),
                                    disk_mtime,
                                ) {
                                    self.unreadable = None;
                                    self.known_mtime = None;
                                }
                            } else if self.reload_check_pending {
                                ui.ctx().request_repaint();
                            }
                        }

                        let super_held = self.pinch.as_ref().is_some_and(PinchListener::super_held);
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
                                super_held,
                                pointer_over_ui: ui.ctx().is_pointer_over_egui(),
                                double_clicked: ui.input(|i| {
                                    i.pointer
                                        .button_double_clicked(egui::PointerButton::Primary)
                                }),
                            };
                            for action in edit_input::translate(&frame_input, &mut self.capture) {
                                match action {
                                    EditorInput::Down(event) => {
                                        // A click on the canvas while editing text ends that
                                        // edit first (its own commit paths never run: the click
                                        // reached here instead of the overlay's `TextEdit`
                                        // because `pointer_over_ui` was false), then proceeds
                                        // as this tool's own pointer-down would otherwise.
                                        commit_pending_text_edit(
                                            editor,
                                            &mut self.text_edit_buffer,
                                            &mut self.measure,
                                        );
                                        editor.pointer_down(event);
                                    }
                                    EditorInput::Move(event) => editor.pointer_move(event),
                                    EditorInput::Up(event) => editor.pointer_up(event),
                                    EditorInput::Tool(tool) => editor.set_tool(tool),
                                    EditorInput::Command(command) => {
                                        editor.command(command);
                                    }
                                    EditorInput::Copy => {
                                        if let Some(text) = editor.copy_selection() {
                                            ui.ctx().copy_text(text);
                                        }
                                    }
                                    EditorInput::Paste(text) => {
                                        let at = self.capture.last().unwrap_or_else(|| {
                                            camera.view_to_scene([
                                                self.canvas_size[0] / 2.0,
                                                self.canvas_size[1] / 2.0,
                                            ])
                                        });
                                        let measure =
                                            self.measure.get_or_insert_with(FontMeasure::new);
                                        editor.paste(&text, at, measure);
                                    }
                                    EditorInput::DoubleClick(event) => {
                                        editor.double_click(event);
                                    }
                                }
                            }
                        }
                        ui.ctx().set_cursor_icon(if panning {
                            egui::CursorIcon::Grab
                        } else {
                            edit_input::cursor_icon(editor.cursor())
                        });

                        if !ui.ctx().egui_wants_keyboard_input()
                            && !super_held
                            && ui.input(|i| ctrl_k_just_pressed(&i.events))
                            && let Some(home) = storage::home_dir()
                            && let Err(error) = agent::open(&home)
                        {
                            self.notice = Some((error, Instant::now()));
                        }
                    }

                    // Serving requests here, after this frame's input and reload check but
                    // before the autosave decision below, means a scene change an `apply`
                    // makes is covered by the save this same frame decides to make, rather than
                    // waiting for the next one.
                    let camera_for_requests = *camera;
                    self.serve_requests(frame, ui.ctx(), camera_for_requests);

                    if let Some(editor) = self.editor.as_mut()
                        && self.unreadable.is_none()
                        && let (Some(path), Some(writer)) =
                            (self.path.clone(), self.writer.as_ref())
                    {
                        if lost_focus {
                            // See `on_exit`'s identical comment: a multi-point line's follow
                            // point must be committed before a focus-loss save, or the save
                            // would either capture that uncommitted point as if confirmed, or,
                            // if nothing else changed, miss it entirely. A text edit in progress
                            // needs the same treatment, since `finish_pending_gesture` does not
                            // touch it.
                            editor.finish_pending_gesture();
                            commit_pending_text_edit(
                                editor,
                                &mut self.text_edit_buffer,
                                &mut self.measure,
                            );
                        }
                        let view = camera_view(camera_for_requests);
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
                    if let Some(wake_after) = self.autosave.wake_after(frame_start) {
                        ui.ctx().request_repaint_after(wake_after);
                    }
                }

                // Read back the current value instead of reusing the mutable borrow above:
                // that borrow's scope ends at the last of its own writes, above, so the drawing
                // code below (and `serve_requests`, in between) works from a plain copy.
                let camera = self.camera.expect("camera was initialized above");
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
                    camera,
                    size_px,
                    pixels_per_point,
                    dark: self.theme.dark,
                    generation: self.generation,
                    faded: Arc::new(
                        self.editor
                            .as_ref()
                            .expect(EDITOR_INVARIANT)
                            .pending_erasure()
                            .clone(),
                    ),
                    // The text element being edited is hidden once the text-edit overlay
                    // renders it as an egui `TextEdit` instead; empty while typing a brand new
                    // text or label, which has no element yet to hide.
                    hidden: Arc::new(
                        self.editor
                            .as_ref()
                            .expect(EDITOR_INVARIANT)
                            .text_editing()
                            .and_then(|editing| editing.element_id.clone())
                            .into_iter()
                            .collect(),
                    ),
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
                        &camera,
                        response.rect.min,
                        colors,
                    ));

                    if self.unreadable.is_none()
                        && let Some(editing) = editor.text_editing().cloned()
                    {
                        let first_frame = self.text_edit_buffer.is_none();
                        let buffer = self
                            .text_edit_buffer
                            .get_or_insert_with(|| editing.text.clone());
                        let outcome = text_edit::show(
                            ui,
                            &editing,
                            buffer,
                            camera,
                            response.rect.min,
                            self.theme.dark,
                            first_frame,
                        );
                        if matches!(outcome, TextEditOutcome::Commit(_)) {
                            commit_pending_text_edit(
                                editor,
                                &mut self.text_edit_buffer,
                                &mut self.measure,
                            );
                        }
                    }

                    let toolbar_colors = ToolbarColors::from_theme(&self.theme);
                    // An unreadable file blocks every other edit (see the pointer/keyboard
                    // gate above); the toolbar still draws so the current tool stays visible,
                    // but a click must not change it.
                    if let Some(tool) = toolbar::show(ui.ctx(), editor.tool(), toolbar_colors)
                        && self.unreadable.is_none()
                    {
                        // A tool change ends a text edit in progress rather than abandoning it
                        // (the toolbar itself is a click the canvas never sees, so the usual
                        // click-commits-first path above never runs for it).
                        commit_pending_text_edit(
                            editor,
                            &mut self.text_edit_buffer,
                            &mut self.measure,
                        );
                        editor.set_tool(tool);
                    }

                    let panel_colors = PanelColors::from_theme(&self.theme);
                    let panel_state = editor.panel();
                    if let Some(property) = properties_panel::show(
                        ui.ctx(),
                        &panel_state,
                        panel_colors,
                        self.theme.dark,
                    ) && self.unreadable.is_none()
                    {
                        let measure = self.measure.get_or_insert_with(FontMeasure::new);
                        editor.set_property(property, measure);
                    }

                    if let Some(message) = &self.unreadable {
                        egui::Area::new(egui::Id::new("napkin-unreadable"))
                            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 52.0))
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
                ui.horizontal(|ui| {
                    // No gap between the two labels: together they read as one path.
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let dir = ui.add(
                        egui::Label::new(egui::RichText::new(&self.display.dir).weak())
                            .sense(egui::Sense::click()),
                    );
                    let name = ui.add(
                        egui::Label::new(egui::RichText::new(&self.display.name))
                            .sense(egui::Sense::click()),
                    );
                    if (dir.clicked() || name.clicked())
                        && let Some(path) = &self.path
                    {
                        let absolute = path.display().to_string();
                        ui.ctx().copy_text(absolute.clone());
                        self.notice = Some((format!("Copied {absolute}"), Instant::now()));
                    }
                });
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
    fn ctrl_k_ignores_key_repeat_but_not_a_fresh_press() {
        let key_event =
            |pressed: bool, repeat: bool, modifiers: egui::Modifiers| egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed,
                repeat,
                modifiers,
            };
        assert!(ctrl_k_just_pressed(&[key_event(
            true,
            false,
            egui::Modifiers::COMMAND
        )]));
        assert!(!ctrl_k_just_pressed(&[key_event(
            true,
            true,
            egui::Modifiers::COMMAND
        )]));
        assert!(!ctrl_k_just_pressed(&[]));
        // A release, or the same key without the command modifier, isn't a press either.
        assert!(!ctrl_k_just_pressed(&[key_event(
            false,
            false,
            egui::Modifiers::COMMAND
        )]));
        assert!(!ctrl_k_just_pressed(&[key_event(
            true,
            false,
            egui::Modifiers::NONE
        )]));
    }

    #[test]
    fn reload_check_waits_out_an_in_flight_save() {
        assert!(!reload_check_ready(false, false), "nothing pending");
        assert!(
            !reload_check_ready(true, true),
            "a save is running; stay pending and try again next frame"
        );
        assert!(reload_check_ready(true, false));
    }

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
    fn a_deleted_unreadable_file_makes_the_canvas_editable_again() {
        let t = SystemTime::UNIX_EPOCH;
        assert!(should_clear_unreadable(true, None));
        assert!(!should_clear_unreadable(true, Some(t)));
        assert!(!should_clear_unreadable(false, None));
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
