//! Touchpad pinch-zoom via `zwp_pointer_gestures_v1`, bound on a second event queue over
//! eframe's own Wayland connection (see [`PinchListener`]). Feeds
//! [`CanvasInput::pinch`](crate::input::CanvasInput::pinch) alongside egui's own
//! `Event::Zoom` (trackpad pinch reported through the platform, e.g. macOS).
//!
//! Derived from the M0 spike (`spikes/m0/src/bin/pinch_probe.rs`, commit `7e14330`), which
//! proved the binding flow works on this machine (Hyprland 0.56, `zwp_pointer_gestures_v1`
//! version 3).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use eframe::egui;
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use rustix::event::{PollFd, PollFlags};
use rustix::fd::OwnedFd;
use wayland_backend::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gesture_pinch_v1::{
    self, ZwpPointerGesturePinchV1,
};
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;

/// One event from `zwp_pointer_gesture_pinch_v1`, stripped of the fields this app doesn't use
/// (finger count, rotation, the begin/end surface, cancellation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinchEvent {
    Begin,
    Update { scale: f64 },
    End,
}

/// Turns the protocol's begin-relative `scale` into per-event zoom factors.
#[derive(Debug)]
pub struct PinchTracker {
    previous: f64,
}

/// Starts as if a gesture had just begun (`previous` = 1), so an `Update` that arrives
/// without a `Begin` still yields a finite factor.
impl Default for PinchTracker {
    fn default() -> Self {
        PinchTracker { previous: 1.0 }
    }
}

impl PinchTracker {
    /// `Begin` and `End` reset the tracker and yield no zoom of their own; `Update` yields
    /// the multiplicative factor since the previous `Update` (or since `Begin`/start).
    pub fn factor(&mut self, event: PinchEvent) -> Option<f64> {
        match event {
            PinchEvent::Begin | PinchEvent::End => {
                self.previous = 1.0;
                None
            }
            PinchEvent::Update { scale } => {
                let factor = scale / self.previous;
                self.previous = scale;
                Some(factor)
            }
        }
    }
}

/// State for the dedicated queue's registry/seat/gestures `Dispatch` impls.
struct GestureState {
    gestures: ZwpPointerGesturesV1,
    ctx: egui::Context,
    tx: Sender<PinchEvent>,
    /// Guards against binding a second `wl_pointer`/pinch gesture if `wl_seat` reports the
    /// pointer capability more than once (e.g. capability lost and regained).
    pointer_gesture_bound: bool,
}

// The registry can add/remove globals after the initial roundtrip; this listener only needs
// the globals present at startup, so dynamic changes are ignored.
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
            if state.pointer_gesture_bound || !capabilities.contains(wl_seat::Capability::Pointer) {
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
            zwp_pointer_gesture_pinch_v1::Event::Begin { .. } => PinchEvent::Begin,
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
                PinchEvent::Update { scale }
            }
            zwp_pointer_gesture_pinch_v1::Event::End { .. } => PinchEvent::End,
            _ => return,
        };
        // The receiver lives in the `PinchListener`; if it's gone the window is closing and
        // `stop` will end this thread shortly.
        let _ = state.tx.send(message);
        state.ctx.request_repaint();
    }
}

// zwp_pointer_gestures_v1 itself has no events, only requests (get_*_gesture).
delegate_noop!(GestureState: ignore ZwpPointerGesturesV1);
// winit's own wl_pointer already handles enter/motion/button/axis; this listener's second
// pointer only exists to obtain the pinch gesture object, so its events are ignored.
delegate_noop!(GestureState: ignore wl_pointer::WlPointer);

/// Binds `zwp_pointer_gestures_v1` on a second event queue over eframe's own Wayland
/// connection and dispatches it from a dedicated thread, forwarding [`PinchEvent`]s.
///
/// # Lifetime of the borrowed `wl_display`
///
/// winit disconnects its Wayland connection (`wl_display_disconnect`) only when eframe's
/// event loop returns, and [`eframe::App::on_exit`] runs before that. `Viewer::on_exit` calls
/// [`PinchListener::stop`], which joins the dispatch thread before returning, so once
/// `on_exit` has run, nothing is left touching the borrowed `wl_display` pointer. For the
/// entire lifetime of the dispatch thread (from `start` until that join completes), the
/// pointer is therefore still valid. [`Drop`] calls `stop` again as a second line of defense
/// in case `on_exit` is ever skipped.
pub struct PinchListener {
    receiver: Receiver<PinchEvent>,
    /// Written to (with the value 1) by [`PinchListener::stop`] to wake the dispatch thread
    /// out of its `poll`; the thread polls the same fd for readability alongside the Wayland
    /// connection's fd.
    stop_fd: Arc<OwnedFd>,
    /// `None` once [`PinchListener::stop`] has joined the thread.
    join_handle: Option<JoinHandle<()>>,
}

impl PinchListener {
    /// `Err` when the display is not Wayland or the compositor lacks
    /// `zwp_pointer_gestures_v1`; the caller keeps Ctrl+wheel zoom only.
    pub fn start(cc: &eframe::CreationContext<'_>) -> Result<PinchListener, String> {
        let raw_display = cc
            .display_handle()
            .map_err(|error| format!("no display handle: {error}"))?
            .as_raw();
        let RawDisplayHandle::Wayland(wayland) = raw_display else {
            return Err("not running on Wayland".to_owned());
        };

        // SAFETY: `wayland.display` is eframe/winit's live `wl_display*`. winit owns this
        // display for the entire process lifetime (it never disconnects it while the app
        // runs), and this listener never disconnects it either, so the pointer outlives the
        // `Backend`/`Connection` built from it below, including on the dispatch thread
        // spawned further down (see the lifetime note on this struct's doc comment).
        let backend = unsafe { Backend::from_foreign_display(wayland.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);

        // A brand new event queue: winit's own queue on this same connection is never touched.
        let (globals, mut event_queue) = registry_queue_init::<GestureState>(&connection)
            .map_err(|error| format!("registry roundtrip failed: {error}"))?;
        let qh = event_queue.handle();

        let gestures: ZwpPointerGesturesV1 = globals
            .bind(&qh, 1..=3, ())
            .map_err(|_| "compositor does not advertise zwp_pointer_gestures_v1".to_owned())?;
        // wl_seat is a multi-instance global; this only cares about the first one. The
        // returned proxy is intentionally dropped: the connection tracks the object by id
        // regardless, and the seat's `capabilities` event (which triggers binding the pointer
        // and pinch gesture, see `Dispatch<WlSeat, ()>` above) is only delivered once the
        // dispatch thread below starts dispatching the queue.
        let _seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=wl_seat::WlSeat::interface().version, ())
            .map_err(|_| "compositor does not advertise wl_seat".to_owned())?;

        let stop_fd = Arc::new(
            rustix::event::eventfd(0, rustix::event::EventfdFlags::CLOEXEC)
                .map_err(|error| format!("eventfd failed: {error}"))?,
        );
        let thread_stop = Arc::clone(&stop_fd);

        let (tx, rx) = mpsc::channel();
        let mut state = GestureState {
            gestures,
            ctx: cc.egui_ctx.clone(),
            tx,
            pointer_gesture_bound: false,
        };

        let join_handle = std::thread::Builder::new()
            .name("napkin-pinch-wayland".to_owned())
            .spawn(move || {
                let stop = thread_stop;
                loop {
                    event_queue.flush().ok();
                    let Some(guard) = event_queue.prepare_read() else {
                        if event_queue.dispatch_pending(&mut state).is_err() {
                            break;
                        }
                        continue;
                    };
                    let ready = {
                        let fd = guard.connection_fd();
                        let mut fds = [
                            PollFd::new(&fd, PollFlags::IN),
                            PollFd::new(&stop, PollFlags::IN),
                        ];
                        if rustix::event::poll(&mut fds, None).is_err() {
                            break;
                        }
                        if fds[1].revents().contains(PollFlags::IN) {
                            break;
                        }
                        fds[0].revents().contains(PollFlags::IN)
                    };
                    // `guard` is dropped here on `break` (whether from the read failing or
                    // from the loop's other breaks above), which abandons this read attempt,
                    // matching libwayland's `prepare_read`/`cancel_read` contract.
                    if ready && guard.read().is_err() {
                        break;
                    }
                    if event_queue.dispatch_pending(&mut state).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| format!("failed to spawn the pinch dispatch thread: {error}"))?;

        Ok(PinchListener {
            receiver: rx,
            stop_fd,
            join_handle: Some(join_handle),
        })
    }

    /// This frame's pinch events, in arrival order.
    pub fn events(&self) -> Vec<PinchEvent> {
        self.receiver.try_iter().collect()
    }

    /// Wakes the dispatch thread and joins it. Idempotent.
    pub fn stop(&mut self) {
        let Some(join_handle) = self.join_handle.take() else {
            // Already stopped.
            return;
        };
        // A write of 1 to an eventfd is always 8 bytes and always succeeds unless the fd
        // itself is broken, in which case the thread is already gone or gone shortly; either
        // way `join` below still returns once the thread's loop notices and exits.
        let _ = rustix::io::write(&self.stop_fd, &1u64.to_ne_bytes());
        let _ = join_handle.join();
    }
}

impl Drop for PinchListener {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_becomes_per_event_factors() {
        let mut tracker = PinchTracker::default();
        assert_eq!(tracker.factor(PinchEvent::Begin), None);
        assert_eq!(tracker.factor(PinchEvent::Update { scale: 1.2 }), Some(1.2));
        let factor = tracker
            .factor(PinchEvent::Update { scale: 1.5 })
            .expect("factor");
        assert!((factor - 1.25).abs() < 1e-12);
        assert_eq!(tracker.factor(PinchEvent::End), None);
        assert_eq!(tracker.factor(PinchEvent::Begin), None);
        assert_eq!(tracker.factor(PinchEvent::Update { scale: 0.8 }), Some(0.8));
    }
}
