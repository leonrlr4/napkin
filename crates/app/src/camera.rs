//! The scene <-> view transform and scene-space rectangles, mirroring Excalidraw's `zoom`,
//! `scrollX`/`scrollY` and wheel-zoom math (`packages/excalidraw/components/App.tsx`
//! `handleWheel`, spec §5.4).

/// Excalidraw's `MIN_ZOOM` (`packages/common/src/constants.ts`).
pub const MIN_ZOOM: f64 = 0.1;
/// Excalidraw's `MAX_ZOOM`.
pub const MAX_ZOOM: f64 = 30.0;
/// Excalidraw's `ZOOM_STEP`, used only to derive [`wheel_zoom`]'s `MAX_STEP`.
const ZOOM_STEP: f64 = 0.1;

/// An axis-aligned rectangle in scene coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneRect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl SceneRect {
    pub fn intersects(&self, other: &SceneRect) -> bool {
        self.min[0] < other.max[0]
            && other.min[0] < self.max[0]
            && self.min[1] < other.max[1]
            && other.min[1] < self.max[1]
    }

    pub fn union(&self, other: &SceneRect) -> SceneRect {
        SceneRect {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    pub fn expand(&self, margin: f64) -> SceneRect {
        SceneRect {
            min: [self.min[0] - margin, self.min[1] - margin],
            max: [self.max[0] + margin, self.max[1] + margin],
        }
    }

    pub fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }

    /// The axis-aligned bounds of `placement`'s rectangle rotated about `center`
    /// (local coordinates relative to the placement origin).
    pub fn of_rotated(
        placement: &scene::Placement,
        local_min: [f64; 2],
        local_max: [f64; 2],
        center: [f64; 2],
    ) -> SceneRect {
        let corners = [
            [local_min[0], local_min[1]],
            [local_max[0], local_min[1]],
            [local_max[0], local_max[1]],
            [local_min[0], local_max[1]],
        ];
        let (sin, cos) = placement.angle.sin_cos();
        let mut rect: Option<SceneRect> = None;
        for [lx, ly] in corners {
            let dx = lx - center[0];
            let dy = ly - center[1];
            let x = placement.x + center[0] + dx * cos - dy * sin;
            let y = placement.y + center[1] + dx * sin + dy * cos;
            let point = SceneRect {
                min: [x, y],
                max: [x, y],
            };
            rect = Some(match rect {
                Some(rect) => rect.union(&point),
                None => point,
            });
        }
        rect.expect("four corners always produce a rect")
    }
}

/// The camera: where the scene sits in the view, mirroring Excalidraw's `appState.scrollX`,
/// `scrollY` and `zoom.value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// `(p + scroll) * zoom`, in logical points from the canvas origin.
    pub fn scene_to_view(&self, p: [f64; 2]) -> [f64; 2] {
        [
            (p[0] + self.scroll_x) * self.zoom,
            (p[1] + self.scroll_y) * self.zoom,
        ]
    }

    pub fn view_to_scene(&self, p: [f64; 2]) -> [f64; 2] {
        [
            p[0] / self.zoom - self.scroll_x,
            p[1] / self.zoom - self.scroll_y,
        ]
    }

    /// Moves the content by `delta` logical points.
    pub fn pan_view(&mut self, delta: [f64; 2]) {
        self.scroll_x += delta[0] / self.zoom;
        self.scroll_y += delta[1] / self.zoom;
    }

    /// Sets `normalized_zoom(zoom)`, keeping the scene point under `anchor` in place.
    pub fn zoom_at(&mut self, zoom: f64, anchor: [f64; 2]) {
        let scene_anchor = self.view_to_scene(anchor);
        self.zoom = normalized_zoom(zoom);
        let new_anchor = self.scene_to_view(scene_anchor);
        self.scroll_x += (anchor[0] - new_anchor[0]) / self.zoom;
        self.scroll_y += (anchor[1] - new_anchor[1]) / self.zoom;
    }

    pub fn visible_rect(&self, view_size: [f64; 2]) -> SceneRect {
        SceneRect {
            min: self.view_to_scene([0.0, 0.0]),
            max: self.view_to_scene(view_size),
        }
    }

    /// Zoom 1 with `rect`'s center in the middle of the view.
    pub fn centered_on(rect: SceneRect, view_size: [f64; 2]) -> Camera {
        let center = rect.center();
        Camera {
            scroll_x: view_size[0] / 2.0 - center[0],
            scroll_y: view_size[1] / 2.0 - center[1],
            zoom: 1.0,
        }
    }

    /// `ceil(log2(zoom * pixels_per_point))`, clamped to -8..=8.
    pub fn bucket(&self, pixels_per_point: f64) -> i32 {
        let value = (self.zoom * pixels_per_point).log2().ceil() as i32;
        value.clamp(-8, 8)
    }
}

/// Excalidraw's `getNormalizedZoom` (`packages/excalidraw/scene/normalize.ts`): rounded to 6
/// decimals, clamped to `MIN_ZOOM..=MAX_ZOOM`.
pub fn normalized_zoom(zoom: f64) -> f64 {
    (rough::js::math_round(zoom * 1e6) / 1e6).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The ctrl/cmd branch of `handleWheel`: the next zoom for a wheel `delta_y` in CSS pixels,
/// before normalization.
pub fn wheel_zoom(zoom: f64, delta_y: f64) -> f64 {
    // `Math.sign(0) == 0`; `f64::signum` returns 1.0 for 0.0, so it is not used here.
    let sign = if delta_y > 0.0 {
        1.0
    } else if delta_y < 0.0 {
        -1.0
    } else {
        0.0
    };
    let max_step = ZOOM_STEP * 100.0;
    let abs_delta = delta_y.abs();
    let delta = if abs_delta > max_step {
        max_step * sign
    } else {
        delta_y
    };

    let mut new_zoom = zoom - delta / 100.0;
    new_zoom += (1.0_f64.max(zoom)).log10() * -sign * 1.0_f64.min(abs_delta / 20.0);
    new_zoom.max(MIN_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_matches_excalidraw() {
        let camera = Camera {
            scroll_x: 10.0,
            scroll_y: -5.0,
            zoom: 2.0,
        };
        assert_eq!(camera.scene_to_view([0.0, 0.0]), [20.0, -10.0]);
        assert_eq!(camera.view_to_scene([20.0, -10.0]), [0.0, 0.0]);
    }

    #[test]
    fn zoom_at_keeps_the_anchor_fixed() {
        let mut camera = Camera::default();
        camera.zoom_at(2.0, [100.0, 50.0]);
        assert_eq!(camera.zoom, 2.0);
        assert_eq!((camera.scroll_x, camera.scroll_y), (-50.0, -25.0));
        camera.zoom_at(100.0, [0.0, 0.0]);
        assert_eq!(camera.zoom, MAX_ZOOM);
    }

    #[test]
    fn wheel_zoom_follows_handle_wheel() {
        // deltaY 50 is capped at MAX_STEP 10: 1 - 10/100; log10(1) adds nothing.
        assert!((wheel_zoom(1.0, 50.0) - 0.9).abs() < 1e-12);
        // 2 + 5/100 + log10(2) * 1 * min(1, 5/20).
        assert!((wheel_zoom(2.0, -5.0) - 2.125_257_498_915_995).abs() < 1e-12);
        // Math.sign(0) is 0, so a zero delta changes nothing.
        assert_eq!(wheel_zoom(3.0, 0.0), 3.0);
    }

    #[test]
    fn centered_on_and_buckets() {
        let rect = SceneRect {
            min: [0.0, 0.0],
            max: [200.0, 100.0],
        };
        let camera = Camera::centered_on(rect, [800.0, 600.0]);
        assert_eq!(camera.scene_to_view([100.0, 50.0]), [400.0, 300.0]);
        assert_eq!(Camera::default().bucket(1.25), 1);
        assert_eq!(
            Camera {
                zoom: 0.1,
                ..Default::default()
            }
            .bucket(1.0),
            -3
        );
    }

    #[test]
    fn of_rotated_bounds_a_quarter_turn() {
        let placement = scene::Placement {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            angle: std::f64::consts::FRAC_PI_2,
        };
        let rect = SceneRect::of_rotated(&placement, [0.0, 0.0], [100.0, 50.0], [50.0, 25.0]);
        assert!((rect.min[0] - 35.0).abs() < 1e-9);
        assert!((rect.max[0] - 85.0).abs() < 1e-9);
        assert!((rect.min[1] - (-5.0)).abs() < 1e-9);
        assert!((rect.max[1] - 95.0).abs() < 1e-9);
    }
}
