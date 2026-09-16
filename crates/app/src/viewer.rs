//! The eframe [`App`](eframe::App) that hosts the canvas: theme, document title and the
//! load-error banner. The camera and canvas rendering are added by later M3 tasks.

use eframe::egui;

use crate::document::Document;
use crate::theme::{self, Theme};

pub struct Viewer {
    document: Document,
    load_error: Option<String>,
    theme: Theme,
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
            bench,
            focused: false,
        }
    }
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
                }
            });

        egui::Area::new(egui::Id::new("napkin-document-name"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.label(&self.document.name);
            });
    }
}
