//! The deterministic camera script `--bench` drives: 5 s panning a 2000-unit circle, then 5 s
//! zooming between 0.5 and 2 about the view center (spec §6.7, decision 11).

use crate::camera::Camera;

/// The script's total length in seconds.
pub const DURATION_S: f64 = 10.0;

/// The pan phase's duration: `0..PAN_S`.
const PAN_S: f64 = 5.0;

/// The circle `camera_at` pans `start`'s scroll around during the pan phase, in scene units;
/// "a 2000-unit circle" is its diameter.
const PAN_RADIUS: f64 = 1000.0;

/// The zoom phase's midpoint and half-range: oscillates between `1.25 - 0.75` and `1.25 + 0.75`,
/// i.e. `0.5..=2.0`.
const ZOOM_MID: f64 = 1.25;
const ZOOM_HALF_RANGE: f64 = 0.75;

/// Camera for `t` seconds into the benchmark: 5 s of panning a 2000-unit circle, then 5 s
/// of zooming between 0.5 and 2 about the view center. `None` once `t >= DURATION_S`.
///
/// Both phases are computed from `start` directly (not by accumulating across calls), so the
/// caller can call this once per frame with the same `start` (the camera in effect when the
/// script began) and get a smooth, reproducible path regardless of frame timing.
pub fn camera_at(t: f64, start: Camera, view_size: [f64; 2]) -> Option<Camera> {
    if t >= DURATION_S {
        return None;
    }
    if t < PAN_S {
        // A full revolution over the pan phase; at t = 0 the offset is [0, 0], so the camera
        // starts exactly at `start` and returns to it as the phase ends.
        let angle = 2.0 * std::f64::consts::PI * (t / PAN_S);
        Some(Camera {
            scroll_x: start.scroll_x + PAN_RADIUS * (angle.cos() - 1.0),
            scroll_y: start.scroll_y + PAN_RADIUS * angle.sin(),
            zoom: start.zoom,
        })
    } else {
        let phase_t = t - PAN_S;
        let zoom =
            ZOOM_MID + ZOOM_HALF_RANGE * (2.0 * std::f64::consts::PI * phase_t / PAN_S).sin();
        let mut camera = start;
        let center = [view_size[0] / 2.0, view_size[1] / 2.0];
        camera.zoom_at(zoom, center);
        Some(camera)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_covers_pan_then_zoom_then_stops() {
        let start = Camera::default();
        let view = [800.0, 600.0];
        let pan = camera_at(2.5, start, view).expect("panning");
        assert_eq!(pan.zoom, 1.0);
        assert!(pan.scroll_x != 0.0 || pan.scroll_y != 0.0);
        let zoom = camera_at(7.5, start, view).expect("zooming");
        assert!((0.5..=2.0).contains(&zoom.zoom));
        assert_eq!(camera_at(DURATION_S, start, view), None);
    }
}
