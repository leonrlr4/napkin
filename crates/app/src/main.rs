use app::cli;
use app::document::Document;
use eframe::egui;

fn main() -> eframe::Result {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("napkin: {error}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    let (document, load_error) = match &cli.file {
        None => (Document::empty(), None),
        Some(path) => match Document::load(path) {
            Ok(document) => (document, None),
            Err(error) => (Document::empty(), Some(error)),
        },
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin")
            .with_title(format!("{} - napkin", document.name)),
        renderer: eframe::Renderer::Wgpu,
        multisampling: 4,
        stencil_buffer: 8,
        ..Default::default()
    };
    let bench = cli.bench;
    eframe::run_native(
        "napkin",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::viewer::Viewer::new(
                cc, document, load_error, bench,
            )))
        }),
    )
}
