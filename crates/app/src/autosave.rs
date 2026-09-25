//! Autosave scheduling: when to write, kept separate from writing itself (`writer.rs`).

use std::time::{Duration, Instant};

use scene::file::NapkinView;

/// Wait after the last element change before writing, while idle (spec §5.5).
pub const DEBOUNCE: Duration = Duration::from_millis(500);
/// Wait between retries of a failed save (spec §8).
pub const RETRY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Frame,
    FocusLost,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DocumentState {
    pub revision: u64,
    pub view: NapkinView,
    pub idle: bool,
}

/// When to autosave. `should_save` is a pure function of the state passed in each call;
/// `started`/`finished` report a save's outcome back so the schedule can track what is
/// still unwritten and back off after a failure.
#[derive(Debug)]
pub struct Autosave {
    saved_revision: u64,
    saved_view: NapkinView,
    /// The most recent revision seen by `should_save`, to detect a new change.
    last_seen_revision: u64,
    /// When `last_seen_revision` last changed.
    changed_at: Option<Instant>,
    /// The state a save is running for, if one is in flight.
    in_flight: Option<DocumentState>,
    error: Option<String>,
    /// When the last save attempt failed.
    failed_at: Option<Instant>,
}

impl Autosave {
    /// Everything counts as saved.
    pub fn new(revision: u64, view: NapkinView) -> Autosave {
        Autosave {
            saved_revision: revision,
            saved_view: view,
            last_seen_revision: revision,
            changed_at: None,
            in_flight: None,
            error: None,
            failed_at: None,
        }
    }

    /// Call every frame with `Trigger::Frame`, and once on focus loss or exit (spec §5.5).
    pub fn should_save(&mut self, now: Instant, state: DocumentState, trigger: Trigger) -> bool {
        if state.revision != self.last_seen_revision {
            self.last_seen_revision = state.revision;
            self.changed_at = Some(now);
        }
        if self.in_flight.is_some() {
            return false;
        }
        match trigger {
            Trigger::Frame => {
                let elements_changed = state.revision != self.saved_revision;
                let debounced = self
                    .changed_at
                    .is_some_and(|at| now.duration_since(at) >= DEBOUNCE);
                let retry_ok = self
                    .failed_at
                    .is_none_or(|at| now.duration_since(at) >= RETRY);
                elements_changed && state.idle && debounced && retry_ok
            }
            Trigger::FocusLost | Trigger::Exit => {
                state.revision != self.saved_revision || state.view != self.saved_view
            }
        }
    }

    pub fn started(&mut self, state: DocumentState) {
        self.in_flight = Some(state);
    }

    pub fn finished(&mut self, now: Instant, result: Result<(), String>) {
        let started = self.in_flight.take();
        match result {
            Ok(()) => {
                if let Some(state) = started {
                    self.saved_revision = state.revision;
                    self.saved_view = state.view;
                }
                self.error = None;
                self.failed_at = None;
            }
            Err(message) => {
                self.error = Some(message);
                self.failed_at = Some(now);
            }
        }
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Element changes not yet written (a failed or running save counts as not written).
    pub fn has_unsaved_changes(&self, revision: u64) -> bool {
        revision != self.saved_revision
    }

    /// How long until `should_save(Trigger::Frame)` can turn true without new input.
    pub fn wake_after(&self, now: Instant) -> Option<Duration> {
        if self.in_flight.is_some() {
            return None;
        }
        let debounce_wait = (self.last_seen_revision != self.saved_revision)
            .then(|| {
                self.changed_at
                    .map(|at| (at + DEBOUNCE).saturating_duration_since(now))
            })
            .flatten();
        let retry_wait = self
            .failed_at
            .map(|at| (at + RETRY).saturating_duration_since(now));
        match (debounce_wait, retry_wait) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// After a reload: everything counts as saved, errors cleared.
    pub fn reset(&mut self, revision: u64, view: NapkinView) {
        *self = Autosave::new(revision, view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> NapkinView {
        NapkinView {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 1.0,
        }
    }

    fn state(revision: u64) -> DocumentState {
        DocumentState {
            revision,
            view: view(),
            idle: true,
        }
    }

    #[test]
    fn saves_500ms_after_the_last_change_when_idle() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut a = Autosave::new(0, view());
        assert!(!a.should_save(t0, state(0), Trigger::Frame));
        assert!(!a.should_save(t0, state(1), Trigger::Frame));
        assert_eq!(a.wake_after(t0), Some(DEBOUNCE));
        assert!(
            !a.should_save(ms(300), state(2), Trigger::Frame),
            "a new change restarts the wait"
        );
        assert!(!a.should_save(
            ms(700),
            DocumentState {
                idle: false,
                ..state(2)
            },
            Trigger::Frame
        ));
        assert!(!a.should_save(ms(799), state(2), Trigger::Frame));
        assert!(a.should_save(ms(800), state(2), Trigger::Frame));
        a.started(state(2));
        assert!(a.in_flight() && a.has_unsaved_changes(2));
        assert!(
            !a.should_save(ms(900), state(2), Trigger::Frame),
            "one write at a time"
        );
        a.finished(ms(950), Ok(()));
        assert!(!a.has_unsaved_changes(2));
        assert!(!a.should_save(ms(2000), state(2), Trigger::Frame));
        assert_eq!(a.wake_after(ms(2000)), None);
    }

    #[test]
    fn focus_loss_and_exit_write_view_changes_at_once() {
        let t0 = Instant::now();
        let mut a = Autosave::new(0, view());
        let panned = DocumentState {
            view: NapkinView {
                scroll_x: 5.0,
                ..view()
            },
            ..state(0)
        };
        assert!(
            !a.should_save(t0, panned, Trigger::Frame),
            "panning alone waits for focus loss"
        );
        assert!(!a.has_unsaved_changes(0));
        assert!(a.should_save(t0, panned, Trigger::FocusLost));
        a.started(panned);
        a.finished(t0, Ok(()));
        assert!(!a.should_save(t0, panned, Trigger::Exit));
        assert!(a.should_save(
            t0,
            DocumentState {
                idle: false,
                ..state(3)
            },
            Trigger::Exit
        ));
    }

    #[test]
    fn failures_stay_reported_and_retry_every_five_seconds() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut a = Autosave::new(0, view());
        a.should_save(t0, state(1), Trigger::Frame);
        assert!(a.should_save(ms(500), state(1), Trigger::Frame));
        a.started(state(1));
        a.finished(ms(510), Err("disk full".into()));
        assert_eq!(a.error(), Some("disk full"));
        assert!(a.has_unsaved_changes(1));
        assert!(!a.should_save(ms(5000), state(1), Trigger::Frame));
        assert_eq!(a.wake_after(ms(5000)), Some(Duration::from_millis(510)));
        assert!(a.should_save(ms(5510), state(1), Trigger::Frame));
        a.started(state(1));
        a.finished(ms(5520), Ok(()));
        assert_eq!(a.error(), None);
    }

    #[test]
    fn reset_after_a_reload_counts_as_saved() {
        let t0 = Instant::now();
        let mut a = Autosave::new(0, view());
        a.should_save(t0, state(4), Trigger::Frame);
        a.reset(7, view());
        assert!(!a.has_unsaved_changes(7));
        assert!(!a.should_save(t0 + DEBOUNCE, state(7), Trigger::FocusLost));
    }
}
