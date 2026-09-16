//! Reads egui's per-frame input events into a [`CanvasInput`] and applies it to a
//! [`Camera`](crate::camera::Camera), mirroring Excalidraw's `handleWheel` (spec §5.4).

use eframe::egui;

use crate::camera::{Camera, normalized_zoom, wheel_zoom};

/// One wheel event, in CSS-pixel convention: positive `y` scrolls down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    pub delta: [f64; 2],
    pub ctrl: bool,
    pub shift: bool,
}

/// This frame's canvas input, gathered by [`CanvasInput::from_egui`] and applied by [`apply`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasInput {
    pub view_size: [f64; 2],
    /// Pointer position in logical points from the canvas origin.
    pub pointer: Option<[f64; 2]>,
    /// Wheel deltas in CSS-pixel convention: positive `y` scrolls down.
    pub wheels: Vec<Wheel>,
    /// Pointer movement while panning with Space+primary or middle drag.
    pub pan_drag: [f64; 2],
    /// Multiplicative zoom from a pinch gesture.
    pub pinch: Option<f64>,
}

/// The point to zoom about: the pointer if present, otherwise the view center.
fn anchor(input: &CanvasInput) -> [f64; 2] {
    input
        .pointer
        .unwrap_or([input.view_size[0] / 2.0, input.view_size[1] / 2.0])
}

/// Applies `input` to `camera`; returns whether the camera changed.
pub fn apply(camera: &mut Camera, input: &CanvasInput) -> bool {
    let before = *camera;

    for wheel in &input.wheels {
        if wheel.ctrl {
            let zoom = normalized_zoom(wheel_zoom(camera.zoom, wheel.delta[1]));
            camera.zoom_at(zoom, anchor(input));
        } else if wheel.shift {
            let delta = if wheel.delta[1] != 0.0 {
                wheel.delta[1]
            } else {
                wheel.delta[0]
            };
            camera.scroll_x -= delta / camera.zoom;
        } else {
            camera.scroll_x -= wheel.delta[0] / camera.zoom;
            camera.scroll_y -= wheel.delta[1] / camera.zoom;
        }
    }

    if input.pan_drag != [0.0, 0.0] {
        camera.pan_view(input.pan_drag);
    }

    if let Some(factor) = input.pinch {
        camera.zoom_at(camera.zoom * factor, anchor(input));
    }

    *camera != before
}

impl CanvasInput {
    /// Reads this frame's events for the canvas `response`.
    pub fn from_egui(ui: &egui::Ui, response: &egui::Response) -> CanvasInput {
        let view_size = [response.rect.width() as f64, response.rect.height() as f64];
        let pointer = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .map(|pos| {
                [
                    (pos.x - response.rect.min.x) as f64,
                    (pos.y - response.rect.min.y) as f64,
                ]
            });

        let line_scroll_speed = ui.ctx().options(|o| o.input_options.line_scroll_speed) as f64;
        let wheels = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } => {
                        // egui's `delta` points the direction the *content* moves; CSS
                        // `deltaY`/`deltaX` point the direction the *wheel* scrolls, the
                        // opposite sign.
                        let raw = match unit {
                            egui::MouseWheelUnit::Point => [delta.x as f64, delta.y as f64],
                            egui::MouseWheelUnit::Line => [
                                delta.x as f64 * line_scroll_speed,
                                delta.y as f64 * line_scroll_speed,
                            ],
                            egui::MouseWheelUnit::Page => {
                                [delta.x as f64 * view_size[1], delta.y as f64 * view_size[1]]
                            }
                        };
                        Some(Wheel {
                            delta: [-raw[0], -raw[1]],
                            ctrl: modifiers.ctrl || modifiers.command,
                            shift: modifiers.shift,
                        })
                    }
                    _ => None,
                })
                .collect()
        });

        let pan_drag = if response.dragged_by(egui::PointerButton::Middle)
            || (ui.input(|i| i.key_down(egui::Key::Space))
                && response.dragged_by(egui::PointerButton::Primary))
        {
            let delta = response.drag_delta();
            [delta.x as f64, delta.y as f64]
        } else {
            [0.0, 0.0]
        };

        CanvasInput {
            view_size,
            pointer,
            wheels,
            pan_drag,
            pinch: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(pointer: [f64; 2]) -> CanvasInput {
        CanvasInput {
            view_size: [800.0, 600.0],
            pointer: Some(pointer),
            ..Default::default()
        }
    }

    #[test]
    fn wheel_pans_by_css_pixels_over_zoom() {
        let mut camera = Camera {
            zoom: 2.0,
            ..Default::default()
        };
        let mut input = at([0.0, 0.0]);
        input.wheels.push(Wheel {
            delta: [10.0, 20.0],
            ctrl: false,
            shift: false,
        });
        assert!(apply(&mut camera, &input));
        assert_eq!((camera.scroll_x, camera.scroll_y), (-5.0, -10.0));
    }

    #[test]
    fn shift_wheel_pans_horizontally() {
        let mut camera = Camera {
            zoom: 2.0,
            ..Default::default()
        };
        let mut input = at([0.0, 0.0]);
        input.wheels.push(Wheel {
            delta: [0.0, 30.0],
            ctrl: false,
            shift: true,
        });
        apply(&mut camera, &input);
        assert_eq!((camera.scroll_x, camera.scroll_y), (-15.0, 0.0));
    }

    #[test]
    fn ctrl_wheel_zooms_about_the_pointer() {
        let mut camera = Camera::default();
        let mut input = at([200.0, 100.0]);
        input.wheels.push(Wheel {
            delta: [0.0, 50.0],
            ctrl: true,
            shift: false,
        });
        apply(&mut camera, &input);
        assert_eq!(camera.zoom, 0.9);
        let anchor = camera.view_to_scene([200.0, 100.0]);
        assert!((anchor[0] - 200.0).abs() < 1e-9 && (anchor[1] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn drag_and_pinch() {
        let mut camera = Camera {
            zoom: 2.0,
            ..Default::default()
        };
        let mut input = at([100.0, 50.0]);
        input.pan_drag = [16.0, -4.0];
        apply(&mut camera, &input);
        assert_eq!((camera.scroll_x, camera.scroll_y), (8.0, -2.0));

        let mut camera = Camera::default();
        let mut input = CanvasInput {
            view_size: [800.0, 600.0],
            pinch: Some(2.0),
            ..Default::default()
        };
        apply(&mut camera, &input);
        // Without a pointer the pinch zooms about the view center.
        assert_eq!(
            (camera.zoom, camera.scroll_x, camera.scroll_y),
            (2.0, -200.0, -150.0)
        );
        input.pinch = None;
        assert!(!apply(&mut camera, &input));
    }
}
