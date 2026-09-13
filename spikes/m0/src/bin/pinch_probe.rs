//! M0 spike (throwaway): can napkin receive touchpad pinch gestures on Hyprland by binding
//! `zwp_pointer_gestures_v1` itself, on its own event queue over eframe's existing Wayland
//! connection, without replacing winit? Findings are recorded in docs/decisions/; this file
//! is deleted once they are.

use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use wayland_backend::client::Backend;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{
    self, ZwpPointerGesturePinchV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;

const LOG_LINES: usize = 24;
const SQUARE_BASE_SIDE: f32 = 120.0;
const SQUARE_MIN_SIDE: f32 = 10.0;
/// Window is 900x640 (see `main`); this comfortably fits above/below the square regardless of
/// zoom, so the square never overruns the window.
const SQUARE_MAX_SIDE: f32 = 300.0;

/// A pinch-gesture event forwarded from the dedicated Wayland dispatch thread to the UI.
enum PinchEvent {
    Begin { fingers: u32 },
    Update { scale: f64 },
    End { cancelled: bool },
}

/// State for our own registry/seat/gestures Dispatch impls, driven from the dedicated thread.
struct GestureState {
    gestures: ZwpPointerGesturesV1,
    ctx: egui::Context,
    tx: Sender<PinchEvent>,
    /// Guards against binding a second `wl_pointer`/pinch gesture if `wl_seat` reports the
    /// pointer capability more than once (e.g. capability lost and regained).
    pointer_gesture_bound: bool,
}

// The registry can add/remove globals after the initial roundtrip; this spike only needs the
// globals present at startup, so dynamic changes are ignored.
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for GestureState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for GestureState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let Ok(capabilities) = capabilities.into_result() else {
                return;
            };
            if state.pointer_gesture_bound || !capabilities.contains(wl_seat::Capability::Pointer)
            {
                return;
            }
            // A second `wl_pointer` on the same seat is valid Wayland; winit keeps its own and
            // never sees this one.
            let pointer = seat.get_pointer(qh, ());
            state.gestures.get_pinch_gesture(&pointer, qh, ());
            state.pointer_gesture_bound = true;
        }
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, ()> for GestureState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let message = match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { fingers, .. } => {
                PinchEvent::Begin { fingers }
            }
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
                PinchEvent::Update { scale }
            }
            zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => {
                PinchEvent::End { cancelled: cancelled != 0 }
            }
            _ => return,
        };
        // The channel receiver lives in the egui App; if it's gone the window is closing.
        let _ = state.tx.send(message);
        state.ctx.request_repaint();
    }
}

// zwp_pointer_gestures_v1 itself has no events, only requests (get_*_gesture).
delegate_noop!(GestureState: ignore ZwpPointerGesturesV1);
// winit's own wl_pointer already handles enter/motion/button/axis; this probe's second pointer
// only exists to obtain the pinch gesture object, so its events are ignored.
delegate_noop!(GestureState: ignore wl_pointer::WlPointer);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin-spike")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "napkin spike: pinch",
        options,
        Box::new(|cc| {
            let rx = start_pinch_gesture_thread(cc);
            Ok(Box::new(PinchProbe::new(rx)))
        }),
    )
}

/// Bind `zwp_pointer_gestures_v1` on a fresh event queue over eframe's own Wayland connection,
/// then hand the queue to a dedicated thread that dispatches it forever.
fn start_pinch_gesture_thread(cc: &eframe::CreationContext<'_>) -> Receiver<PinchEvent> {
    let raw_display = cc
        .display_handle()
        .expect("eframe should always provide a display handle")
        .as_raw();
    let RawDisplayHandle::Wayland(wayland) = raw_display else {
        panic!(
            "pinch_probe requires a Wayland display handle (run under Hyprland with eframe's \
             \"wayland\" feature); got {raw_display:?} instead"
        );
    };

    // SAFETY: `wayland.display` is eframe/winit's live `wl_display*`. winit owns this display
    // for the entire process lifetime (it never disconnects it while the app runs), and this
    // probe never disconnects it either, so the pointer outlives the `Backend`/`Connection`
    // built from it below, including on the dedicated dispatch thread spawned further down.
    let backend = unsafe { Backend::from_foreign_display(wayland.display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);

    // A brand new event queue: winit's own queue on this same connection is never touched.
    let (globals, mut event_queue) = registry_queue_init::<GestureState>(&connection)
        .expect("registry roundtrip should succeed on eframe's live Wayland connection");
    let qh = event_queue.handle();

    let gestures: ZwpPointerGesturesV1 = globals
        .bind(&qh, 1..=3, ())
        .expect("Hyprland advertises zwp_pointer_gestures_v1 (verified for this spike)");
    // wl_seat is a multi-instance global; this spike only cares about the first one, which is
    // the only seat on the test machine. The returned proxy is intentionally dropped: the
    // connection tracks the object by id regardless, and the seat's `capabilities` event (which
    // triggers binding the pointer and pinch gesture, see `Dispatch<WlSeat, ()>` above) is only
    // delivered once the dedicated thread below starts dispatching the queue.
    let _seat: wl_seat::WlSeat = globals
        .bind(&qh, 1..=wl_seat::WlSeat::interface().version, ())
        .expect("Hyprland advertises wl_seat");

    let (tx, rx) = mpsc::channel();
    let mut state = GestureState {
        gestures,
        ctx: cc.egui_ctx.clone(),
        tx,
        pointer_gesture_bound: false,
    };

    std::thread::Builder::new()
        .name("pinch-probe-wayland".to_owned())
        .spawn(move || loop {
            if let Err(err) = event_queue.blocking_dispatch(&mut state) {
                eprintln!("pinch_probe: wayland dispatch error, stopping: {err}");
                break;
            }
        })
        .expect("failed to spawn the pinch-gesture dispatch thread");

    rx
}

struct PinchProbe {
    rx: Receiver<PinchEvent>,
    log: Vec<String>,
    /// Cumulative scale reported by the most recent `update` (or 1.0 after `begin`); used to
    /// turn the protocol's begin-relative `scale` into a per-event factor.
    previous_scale: f64,
    /// Accumulated zoom applied to the square, updated by successive per-event factors.
    zoom: f64,
}

impl PinchProbe {
    fn new(rx: Receiver<PinchEvent>) -> Self {
        Self { rx, log: Vec::new(), previous_scale: 1.0, zoom: 1.0 }
    }

    fn record(&mut self, line: String) {
        println!("{line}");
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }
}

impl eframe::App for PinchProbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                PinchEvent::Begin { fingers } => {
                    self.previous_scale = 1.0;
                    self.record(format!("PINCH begin fingers={fingers}"));
                }
                PinchEvent::Update { scale } => {
                    let factor = scale / self.previous_scale;
                    self.previous_scale = scale;
                    self.zoom *= factor;
                    let zoom = self.zoom;
                    self.record(format!(
                        "PINCH update scale={scale:.3} factor={factor:.3} zoom={zoom:.3}"
                    ));
                }
                PinchEvent::End { cancelled } => {
                    self.record(format!("PINCH end cancelled={cancelled}"));
                }
            }
        }

        let events = ui.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Zoom(factor) = event {
                self.record(format!("EGUI  Zoom({factor})"));
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Pinch on the touchpad over this window");
            ui.label(format!("accumulated zoom: {:.3}", self.zoom));
            ui.add_space(8.0);

            let side =
                (SQUARE_BASE_SIDE * self.zoom as f32).clamp(SQUARE_MIN_SIDE, SQUARE_MAX_SIDE);
            let (rect, _response) =
                ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_rgb(80, 140, 220));

            ui.add_space(8.0);
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in &self.log {
                    ui.monospace(line);
                }
            });
        });
    }
}
