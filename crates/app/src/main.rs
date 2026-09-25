use std::path::{Path, PathBuf};

use app::cli;
use app::storage::{self, Content};
use eframe::egui;

/// The file stem, or the whole path when it has none.
fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// [`storage::load`] translated into the same [`Content`] `storage::open_at_startup` produces,
/// for the paths that call it directly: `--bench`, and no `$HOME` with a file argument.
fn content_from_load(loaded: storage::Loaded) -> Content {
    match loaded {
        storage::Loaded::Missing => Content::Editable {
            file: scene::SceneFile::new(),
            mtime: None,
        },
        storage::Loaded::Parsed { file, mtime } => Content::Editable {
            file,
            mtime: Some(mtime),
        },
        storage::Loaded::Invalid(message) => Content::Unreadable(message),
    }
}

/// `(name, path, content, notice)`. `path` is `None` when nothing should be written: `--bench`
/// (spec §9.3, never writes anything, not even `last`), and no file argument with no `$HOME`.
fn open(cli: &cli::Cli) -> (String, Option<PathBuf>, Content, Option<String>) {
    if cli.bench {
        return match &cli.file {
            Some(path) => (
                name_of(path),
                None,
                content_from_load(storage::load(path)),
                None,
            ),
            None => (
                "untitled".to_string(),
                None,
                Content::Editable {
                    file: scene::SceneFile::new(),
                    mtime: None,
                },
                None,
            ),
        };
    }

    match storage::Paths::from_env() {
        Some(paths) => {
            let opened = storage::open_at_startup(&paths, cli.file.as_deref());
            if let Content::Editable { .. } = &opened.content
                && let Err(error) = storage::remember(&paths, &opened.path)
            {
                eprintln!("napkin: {}: {error}", paths.last.display());
            }
            (
                opened.name,
                Some(opened.path),
                opened.content,
                opened.notice,
            )
        }
        None => match &cli.file {
            Some(path) => (
                name_of(path),
                Some(path.clone()),
                content_from_load(storage::load(path)),
                None,
            ),
            None => (
                "untitled".to_string(),
                None,
                Content::Editable {
                    file: scene::SceneFile::new(),
                    mtime: None,
                },
                Some("HOME is not set; changes are not saved".to_string()),
            ),
        },
    }
}

fn main() -> eframe::Result {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("napkin: {error}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    let bench = cli.bench;
    let (name, path, content, notice) = open(&cli);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin")
            .with_title(format!("{name} - napkin")),
        renderer: eframe::Renderer::Wgpu,
        multisampling: 4,
        stencil_buffer: 8,
        ..Default::default()
    };
    eframe::run_native(
        "napkin",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::napkin_app::NapkinApp::new(
                cc, name, path, content, notice, bench,
            )))
        }),
    )
}
