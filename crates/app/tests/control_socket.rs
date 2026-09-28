//! A windowless server (socket + Editor, no GUI) driven by the real `napkin` binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app::camera::Camera;
use app::control::handler::{Session, handle};
use app::control::render::Rasterize;
use app::control::server::{BindError, Server};
use scene::editor::Editor;
use scene::sample::{self, CharWidthMeasure};

struct NoGpu;

impl Rasterize for NoGpu {
    fn rasterize(
        &mut self,
        _: std::sync::Arc<scene::SceneFile>,
        _: Camera,
        _: [u32; 2],
        _: bool,
    ) -> Result<Vec<u8>, String> {
        Err("no GPU in this test".into())
    }
}

fn socket(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("napkin-test-{}-{name}.sock", std::process::id()))
}

/// A running [`spawn_server`] thread plus the flag that tells it to stop.
struct ServerHandle {
    thread: std::thread::JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

/// Binds `path` and serves requests on a dedicated thread until [`stop`] is called: the thread
/// owns the `Editor`, polls `server.try_recv()` every 5 ms and answers each request with
/// `handle`, using a fresh `Session` (`CharWidthMeasure`, `NoGpu`, an 800x600 canvas) each time.
fn spawn_server(path: PathBuf) -> ServerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread = std::thread::spawn(move || {
        let server = Server::bind(&path, || {}).expect("binds the test socket");
        let mut editor = Editor::new(sample::file(vec![]), scene::env::SystemEnv);
        let mut measure = CharWidthMeasure;
        let mut rasterizer = NoGpu;
        while !thread_stop.load(Ordering::Acquire) {
            let Some(incoming) = server.try_recv() else {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            };
            let mut session = Session {
                editor: &mut editor,
                path: None,
                unsaved: false,
                save_error: None,
                readonly: None,
                camera: Camera::default(),
                canvas_size: [800.0, 600.0],
                measure: &mut measure,
                dark: false,
                rasterizer: &mut rasterizer,
            };
            let response = handle(&mut session, &incoming.request);
            incoming.reply(response);
        }
        // Dropping `server` here stops the accept thread and removes the socket file.
    });
    ServerHandle { thread, stop }
}

/// Stops `server`'s thread and waits for it to exit, so its `Server` has been dropped (and the
/// socket file removed) by the time this returns.
fn stop(server: ServerHandle, path: &Path) {
    server.stop.store(true, Ordering::Release);
    server
        .thread
        .join()
        .expect("the server thread does not panic");
    assert!(!path.exists(), "the server removes its socket file on drop");
}

fn napkin(path: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_napkin"))
        .args(args)
        .env("NAPKIN_SOCKET", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn subcommands_talk_to_a_running_server() {
    let path = socket("talk");
    let server = spawn_server(path.clone());
    let out = napkin(
        &path,
        &["apply"],
        r#"{"ops": [{"op": "add", "type": "rectangle", "id": "a",
        "x": 0, "y": 0, "width": 40, "height": 20}]}"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = body["created"]["a"].as_str().unwrap().to_owned();

    let out = napkin(&path, &["scene"], "");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains(&format!("{id} rectangle 0 0 40 20")),
        "{text}"
    );

    let out = napkin(
        &path,
        &["apply"],
        r#"{"ops": [{"op": "delete", "ids": ["nope"]}]}"#,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nope"));

    let out = napkin(&path, &["render", "--out", "/tmp/unused.png"], "");
    assert_eq!(out.status.code(), Some(1));
    stop(server, &path);
}

#[test]
fn not_running_is_an_error_without_side_effects() {
    let path = socket("absent");
    let out = napkin(&path, &["status"], "");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr).trim(),
        "napkin is not running; open it with SUPER+N"
    );
}

#[test]
fn a_stale_socket_is_replaced_and_a_live_one_is_not() {
    let path = socket("stale");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap()); // leaves the file behind
    let first = Server::bind(&path, || {}).expect("stale file replaced");
    assert!(matches!(
        Server::bind(&path, || {}),
        Err(BindError::AlreadyRunning)
    ));
    let mode = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
        0o600
    );
    drop(first);
    assert!(!path.exists(), "the server removes its socket file");
}
