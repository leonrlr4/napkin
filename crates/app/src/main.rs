use std::io::Read as _;
use std::path::{Path, PathBuf};

use app::cli;
use app::control::client::{self, ClientError};
use app::control::server;
use app::storage::{self, Content};
use eframe::egui;

/// `path`'s [`storage::DisplayPath`], or `dir` empty and `name` "untitled" when there is none.
fn name_of(path: Option<&Path>, home: Option<&Path>) -> storage::DisplayPath {
    match path {
        Some(path) => storage::display_path(path, home),
        None => storage::DisplayPath {
            dir: String::new(),
            name: "untitled".to_string(),
        },
    }
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

/// `$HOME`, when it is set and non-empty (the same check [`storage::Paths::from_env`] makes).
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `(display, path, content, notice)`. `path` is `None` when nothing should be written:
/// `--bench` (spec §9.3, never writes anything, not even `last`), and no file argument with no
/// `$HOME`.
fn open(
    file: Option<&Path>,
    bench: bool,
) -> (
    storage::DisplayPath,
    Option<PathBuf>,
    Content,
    Option<String>,
) {
    let home = home_dir();
    if bench {
        return match file {
            Some(path) => (
                name_of(Some(path), home.as_deref()),
                None,
                content_from_load(storage::load(path)),
                None,
            ),
            None => (
                name_of(None, home.as_deref()),
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
            let opened = storage::open_at_startup(&paths, file);
            if let Content::Editable { .. } = &opened.content
                && let Err(error) = storage::remember(&paths, &opened.path)
            {
                eprintln!("napkin: {}: {error}", paths.last.display());
            }
            (
                name_of(Some(&opened.path), home.as_deref()),
                Some(opened.path),
                opened.content,
                opened.notice,
            )
        }
        None => match file {
            Some(path) => (
                name_of(Some(path), home.as_deref()),
                Some(path.to_path_buf()),
                content_from_load(storage::load(path)),
                None,
            ),
            None => (
                name_of(None, home.as_deref()),
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

/// Turns a control subcommand into a [`app::control::Request`] and, when it is `apply`, the
/// batch to send along with it: read from stdin, which must be JSON (any shape; the server
/// validates it as a batch). Exits the process directly on a stdin read or parse failure.
fn control_request(command: cli::ControlCommand) -> app::control::Request {
    use app::control::Request;
    match command {
        cli::ControlCommand::Status => Request::Status,
        cli::ControlCommand::Scene { full } => Request::Scene { full },
        cli::ControlCommand::Selection { full } => Request::Selection { full },
        cli::ControlCommand::View => Request::View,
        cli::ControlCommand::Apply => {
            let mut input = String::new();
            let batch = std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| error.to_string())
                .and_then(|_| serde_json::from_str(&input).map_err(|error| error.to_string()));
            match batch {
                Ok(batch) => Request::Apply { batch },
                Err(error) => {
                    eprintln!("napkin: stdin is not JSON: {error}");
                    std::process::exit(2);
                }
            }
        }
        cli::ControlCommand::Render { out, target } => Request::Render { out, target },
    }
}

/// Runs a control subcommand against an already-running napkin over its Unix socket and exits:
/// 0 on success (the response's `output` printed to stdout), 1 when napkin is not running or
/// answered with an error (printed to stderr).
fn run_control(command: cli::ControlCommand) -> ! {
    let request = control_request(command);
    let path = match server::socket_path() {
        Some(path) => path,
        None => {
            eprintln!("{}", client::NOT_RUNNING);
            std::process::exit(1);
        }
    };
    match client::send(&path, &request) {
        Ok(output) => {
            print!("{output}");
            if !output.ends_with('\n') {
                println!();
            }
            std::process::exit(0);
        }
        Err(ClientError::NotRunning) => {
            eprintln!("{}", client::NOT_RUNNING);
            std::process::exit(1);
        }
        Err(ClientError::Failed(output)) => {
            eprint!("{output}");
            if !output.ends_with('\n') {
                eprintln!();
            }
            std::process::exit(1);
        }
        Err(ClientError::Io(error)) => {
            eprintln!("napkin: {error}");
            std::process::exit(1);
        }
    }
}

fn main() -> eframe::Result {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("napkin: {error}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    let (file, bench) = match command {
        cli::Command::Gui { file, bench } => (file, bench),
        cli::Command::Control(control_command) => run_control(control_command),
    };
    let (display, path, content, notice) = open(file.as_deref(), bench);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("napkin")
            .with_title(format!("{}{} - napkin", display.dir, display.name)),
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
                cc, display, path, content, notice, bench,
            )))
        }),
    )
}
