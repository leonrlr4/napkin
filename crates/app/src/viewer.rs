//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title, the camera,
//! the load-error banner and the GPU canvas itself.

use eframe::egui;

use crate::camera::{Camera, SceneRect, normalized_zoom};
use crate::document::Document;
use crate::input::{self, CanvasInput};
use crate::render::callback::{self, CanvasCallback};
use crate::render::color::render_color;
use crate::render::gpu::CanvasFrame;
use crate::theme::{self, Theme};

pub struct Viewer {
    document: Document,
    load_error: Option<String>,
    theme: Theme,
    /// `None` until the first frame, when the canvas's `view_size` becomes known and the
    /// camera is set from `appState.napkin` or the document's bounds (decision 5).
    camera: Option<Camera>,
    /// Set from `--bench`; unread until a later M3 task wires up the hidden frame-time
    /// panel (spec §9.3).
    #[allow(dead_code)]
    bench: bool,
    /// Last frame's `ui.input(|i| i.focused)`, to detect the false-to-true edge that
    /// triggers a theme re-read (spec §7.6).
    focused: bool,
}

impl Viewer {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        document: Document,
        load_error: Option<String>,
        bench: bool,
    ) -> Viewer {
        if let Some(render_state) = &cc.wgpu_render_state {
            callback::install(render_state);
        }
        Viewer {
            document,
            load_error,
            theme: load_theme(),
            camera: None,
            bench,
            focused: false,
        }
    }
}

/// `appState.napkin` if present and valid, otherwise zoom 1 centered on the union of every
/// non-deleted element's (unrotated) placement rectangle, or the origin for an empty
/// document (decision 5).
///
/// `None` when `view_size` has zero (or negative) width or height: the canvas hasn't been
/// laid out with a real size yet, so there is nothing sensible to center on, and the caller
/// should keep waiting rather than latch onto a degenerate camera.
///
/// A stored `appState.napkin` is used only when its `scrollX`/`scrollY`/`zoom` are all
/// finite and `zoom` is positive; `zoom` is then passed through [`normalized_zoom`] so an
/// out-of-range value (a hand-edited file, or a future Excalidraw with a wider zoom range)
/// still clamps to `MIN_ZOOM..=MAX_ZOOM` instead of producing a degenerate transform. A
/// non-finite or non-positive stored zoom falls back to the bounds-centered camera below,
/// same as a missing `appState.napkin`.
fn initial_camera(file: &scene::SceneFile, view_size: [f64; 2]) -> Option<Camera> {
    if !(view_size[0] > 0.0 && view_size[1] > 0.0) {
        return None;
    }
    if let Some(view) = file.napkin_view()
        && view.scroll_x.is_finite()
        && view.scroll_y.is_finite()
        && view.zoom.is_finite()
        && view.zoom > 0.0
    {
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

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let focused = ui.input(|i| i.focused);
        if focused && !self.focused {
            self.theme = load_theme();
        }
        self.focused = focused;
        ui.ctx().set_visuals(self.theme.visuals());

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.theme.background))
            .show(ui, |ui| {
                if let Some(error) = &self.load_error {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(egui::Color32::RED, error);
                    });
                    return;
                }

                let (_, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                let view_size = [response.rect.width() as f64, response.rect.height() as f64];
                if self.camera.is_none() {
                    self.camera = initial_camera(&self.document.file, view_size);
                }
                // Still `None` on a zero-size first frame (window not yet laid out): wait
                // for a later frame with a real size instead of latching onto a degenerate
                // camera.
                let Some(camera) = self.camera.as_mut() else {
                    return;
                };
                // The input that changed the camera already triggered this repaint, so no
                // `request_repaint` call is needed here.
                input::apply(camera, &CanvasInput::from_egui(ui, &response));

                let background =
                    render_color(self.document.file.view_background_color(), self.theme.dark);
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
                    file: self.document.file.clone(),
                    camera: *camera,
                    size_px,
                    pixels_per_point,
                    dark: self.theme.dark,
                };
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    response.rect,
                    CanvasCallback { frame },
                ));
            });

        egui::Area::new(egui::Id::new("napkin-document-name"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.label(&self.document.name);
            });
    }
}

#[cfg(test)]
mod tests {
    use scene::file::NapkinView;

    use super::*;
    use crate::camera::MAX_ZOOM;

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
