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

/// The drag phase's length, after the camera script (`DURATION_S`).
pub const DRAG_S: f64 = 5.0;

/// The circle `drag_pointer_at`'s grab point follows around `start`, in scene units.
const DRAG_RADIUS: f64 = 200.0;

/// The scene point the dragged element's grab point follows `t` seconds into the drag phase:
/// one revolution around `start` with a 200-unit radius. `None` once `t >= DRAG_S`.
///
/// Uses the same `start + radius * (cos θ - 1, sin θ)` form as `camera_at`'s pan phase, so at
/// t = 0 the point is exactly `start` and the drag ends back where it began.
pub fn drag_pointer_at(t: f64, start: [f64; 2]) -> Option<[f64; 2]> {
    if t >= DRAG_S {
        return None;
    }
    let angle = 2.0 * std::f64::consts::PI * (t / DRAG_S);
    Some([
        start[0] + DRAG_RADIUS * (angle.cos() - 1.0),
        start[1] + DRAG_RADIUS * angle.sin(),
    ])
}

/// The element the drag phase moves: the last non-deleted rectangle, diamond or ellipse, and
/// the center of its (unrotated) placement box. `None` when the scene has no such element.
pub fn drag_target(file: &scene::SceneFile) -> Option<(usize, [f64; 2])> {
    file.elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            !element.is_deleted() && matches!(element.kind(), "rectangle" | "diamond" | "ellipse")
        })
        .filter_map(|(index, element)| {
            let placement = element.placement()?;
            Some((
                index,
                [
                    placement.x + placement.width / 2.0,
                    placement.y + placement.height / 2.0,
                ],
            ))
        })
        .next_back()
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

    #[test]
    fn drag_phase_circles_the_start_and_picks_the_last_shape() {
        let start = [100.0, 50.0];
        assert_eq!(drag_pointer_at(0.0, start), Some(start));
        let quarter = drag_pointer_at(DRAG_S / 4.0, start).expect("dragging");
        assert!(((quarter[0] - start[0]).powi(2) + (quarter[1] - start[1]).powi(2)).sqrt() > 100.0);
        assert_eq!(drag_pointer_at(DRAG_S, start), None);

        let file = scene::sample::file(vec![
            scene::sample::generic("rectangle", "a", [0.0, 0.0, 10.0, 10.0]),
            scene::sample::generic("ellipse", "b", [20.0, 0.0, 10.0, 20.0]),
            scene::sample::linear("arrow", "c", [0.0, 0.0], &[[0.0, 0.0], [5.0, 5.0]]),
        ]);
        assert_eq!(drag_target(&file), Some((1, [25.0, 10.0])));
        assert_eq!(drag_target(&scene::SceneFile::new()), None);
    }
}
