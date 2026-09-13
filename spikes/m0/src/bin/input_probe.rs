//! M0 spike (throwaway): does fcitx5 reach an egui `TextEdit` on Hyprland, and does
//! winit deliver touchpad pinch as `egui::Event::Zoom`? Findings are recorded in
//! docs/decisions/; this file is deleted once they are.

use std::sync::Arc;

use eframe::egui;

const CJK_FONT_PATH: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";
/// Face index of "Noto Sans CJK TC" inside the collection (`fc-query` on this machine).
const CJK_FONT_INDEX: u32 = 3;
const LOG_LINES: usize = 24;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin-spike")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "napkin spike: input",
        options,
        Box::new(|cc| {
            install_cjk_font(&cc.egui_ctx);
            Ok(Box::new(InputProbe::default()))
        }),
    )
}

fn install_cjk_font(ctx: &egui::Context) {
    let bytes = std::fs::read(CJK_FONT_PATH).expect("Noto Sans CJK is required for this probe");
    let mut data = egui::FontData::from_owned(bytes);
    data.index = CJK_FONT_INDEX;
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("noto-cjk".to_owned(), Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .expect("egui defines both default families")
            .push("noto-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[derive(Default)]
struct InputProbe {
    text: String,
    log: Vec<String>,
}

impl InputProbe {
    fn record(&mut self, line: String) {
        println!("{line}");
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }
}

impl eframe::App for InputProbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Ime(ime) => self.record(format!("IME  {ime:?}")),
                egui::Event::Zoom(factor) => self.record(format!("ZOOM {factor}")),
                _ => {}
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("1. Click the box, switch fcitx5 to Chinese, type nihao + space");
            // Offset from the window origin so a candidate popup stuck at (0,0) is obvious.
            ui.add_space(160.0);
            ui.horizontal(|ui| {
                ui.add_space(320.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .font(egui::TextStyle::Heading)
                        .desired_width(320.0),
                );
            });
            ui.add_space(24.0);
            ui.heading("2. Pinch on the touchpad anywhere in this window");
            ui.separator();
            for line in &self.log {
                ui.monospace(line);
            }
        });
    }
}
