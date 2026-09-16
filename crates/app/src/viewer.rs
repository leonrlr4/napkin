//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title, the camera
//! and the load-error banner. Tessellation and the GPU canvas are added by later M3 tasks.

use eframe::egui;

use crate::camera::{Camera, SceneRect};
use crate::document::Document;
use crate::input::{self, CanvasInput};
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
        _cc: &eframe::CreationContext<'_>,
        document: Document,
        load_error: Option<String>,
        bench: bool,
    ) -> Viewer {
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

/// `appState.napkin` if present, otherwise zoom 1 centered on the union of every non-deleted
/// element's (unrotated) placement rectangle, or the origin for an empty document
/// (decision 5).
fn initial_camera(file: &scene::SceneFile, view_size: [f64; 2]) -> Camera {
    if let Some(view) = file.napkin_view() {
        return Camera {
            scroll_x: view.scroll_x,
            scroll_y: view.scroll_y,
            zoom: view.zoom,
        };
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
    Camera::centered_on(bounds, view_size)
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
                let camera = self
                    .camera
                    .get_or_insert_with(|| initial_camera(&self.document.file, view_size));
                // The input that changed the camera already triggered this repaint, so no
                // `request_repaint` call is needed here.
                input::apply(camera, &CanvasInput::from_egui(ui, &response));

                // TODO(task 6): remove this placeholder readout once the canvas renders the
                // scene itself.
                ui.painter().text(
                    response.rect.left_top(),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "zoom {:.3}  scroll {:.1}, {:.1}",
                        camera.zoom, camera.scroll_x, camera.scroll_y
                    ),
                    egui::FontId::monospace(12.0),
                    self.theme.foreground,
                );
            });

        egui::Area::new(egui::Id::new("napkin-document-name"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.label(&self.document.name);
            });
    }
}
