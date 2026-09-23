//! Frame-time statistics for the hidden frame-time panel (toggled by F12) and `--bench`
//! (`bench.rs`): a rolling window of per-frame intervals and CPU time, and the nearest-rank
//! percentile over either.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many recent samples [`FrameStats`] keeps: at 120 Hz, 1200 frames is 10 s, matching
/// [`crate::bench::DURATION_S`].
pub const WINDOW: usize = 1200;

/// Nearest-rank percentile of `samples` (unsorted): `rank = ceil(p / 100 * n)`, clamped to
/// `1..=n`, over the ascending sort of `samples`. `None` for no samples.
pub fn percentile(samples: &[f64], p: f64) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| {
        a.partial_cmp(b)
            .expect("frame-time samples are always finite")
    });
    let n = sorted.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    Some(sorted[rank - 1])
}

/// Rolling frame-time statistics: the interval between successive [`frame_started`](Self::frame_started)
/// calls and the CPU time each frame took, both windowed to [`WINDOW`] samples, plus a total
/// frame count that is never trimmed.
pub struct FrameStats {
    intervals_ms: VecDeque<f64>,
    cpu_ms: VecDeque<f64>,
    frames: u64,
    last_started: Option<Instant>,
}

impl FrameStats {
    pub fn new() -> FrameStats {
        FrameStats {
            intervals_ms: VecDeque::new(),
            cpu_ms: VecDeque::new(),
            frames: 0,
            last_started: None,
        }
    }

    /// Call at the start of each `ui()`; records the interval since the previous call (none on
    /// the first call, since there is nothing to measure from) and counts this frame.
    pub fn frame_started(&mut self, now: Instant) {
        if let Some(last) = self.last_started {
            push_windowed(
                &mut self.intervals_ms,
                now.duration_since(last).as_secs_f64() * 1000.0,
            );
        }
        self.last_started = Some(now);
        self.frames += 1;
    }

    pub fn cpu_finished(&mut self, cpu: Duration) {
        push_windowed(&mut self.cpu_ms, cpu.as_secs_f64() * 1000.0);
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn interval_p99(&self) -> Option<f64> {
        percentile(&contiguous(&self.intervals_ms), 99.0)
    }

    pub fn cpu_p99(&self) -> Option<f64> {
        percentile(&contiguous(&self.cpu_ms), 99.0)
    }
}

impl Default for FrameStats {
    fn default() -> FrameStats {
        FrameStats::new()
    }
}

fn push_windowed(buffer: &mut VecDeque<f64>, value: f64) {
    if buffer.len() == WINDOW {
        buffer.pop_front();
    }
    buffer.push_back(value);
}

fn contiguous(buffer: &VecDeque<f64>) -> Vec<f64> {
    buffer.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentile() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&samples, 99.0), Some(99.0));
        assert_eq!(percentile(&samples, 100.0), Some(100.0));
        assert_eq!(percentile(&[], 99.0), None);
    }

    #[test]
    fn intervals_start_on_the_second_frame() {
        let start = std::time::Instant::now();
        let mut stats = FrameStats::new();
        stats.frame_started(start);
        assert_eq!(stats.interval_p99(), None);
        stats.frame_started(start + std::time::Duration::from_millis(8));
        assert_eq!(stats.frames(), 2);
        assert_eq!(stats.interval_p99(), Some(8.0));
    }
}
