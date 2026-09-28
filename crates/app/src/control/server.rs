//! Serves the control protocol over a Unix socket: one named thread (`napkin-control`) accepts
//! connections one at a time and hands each request to the caller through a channel, so the UI
//! thread stays in charge of when a request actually runs. `$NAPKIN_SOCKET` overrides the socket
//! path; it exists for the integration test in `tests/control_socket.rs`, which cannot share
//! `$XDG_RUNTIME_DIR` with a real running napkin.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::control::{Request, Response};

/// A request line longer than this is rejected instead of read into memory.
const MAX_LINE_BYTES: u64 = 16 * 1024 * 1024;
/// How long a connection may take to send its request line.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the accept thread waits for the UI thread to answer a queued request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

/// `$NAPKIN_SOCKET` when set (tests), else `$XDG_RUNTIME_DIR/napkin.sock`; `None` when neither.
pub fn socket_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NAPKIN_SOCKET") {
        return Some(PathBuf::from(path));
    }
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(|dir| Path::new(&dir).join("napkin.sock"))
}

/// One request waiting for the UI thread's answer.
pub struct Incoming {
    pub request: Request,
    reply: mpsc::Sender<Response>,
}

impl Incoming {
    pub fn reply(self, response: Response) {
        // The connection may already be gone (client timed out and disconnected); nothing to
        // do about that here.
        let _ = self.reply.send(response);
    }
}

#[derive(Debug)]
pub enum BindError {
    AlreadyRunning,
    Io(std::io::Error),
}

pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    incoming: mpsc::Receiver<Incoming>,
}

impl Server {
    /// Binds `path` (mode 0600), replacing a stale socket file nobody is listening on. `wake`
    /// runs on the accept thread right after a request is queued, so the caller can nudge its
    /// event loop (the app passes `egui::Context::request_repaint`).
    pub fn bind(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Server, BindError> {
        if path.exists() {
            match UnixStream::connect(path) {
                Ok(_) => return Err(BindError::AlreadyRunning),
                Err(_) => {
                    // Nobody is listening: a leftover file from a crash, or from a listener
                    // that was bound but never accepted from. Replace it.
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        let listener = UnixListener::bind(path).map_err(BindError::Io)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(BindError::Io)?;

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("napkin-control".to_string())
            .spawn(move || accept_loop(listener, &thread_stop, &tx, wake))
            .map_err(BindError::Io)?;

        Ok(Server {
            path: path.to_path_buf(),
            stop,
            thread: Some(thread),
            incoming: rx,
        })
    }

    /// The next queued request, if the accept thread has one waiting.
    pub fn try_recv(&self) -> Option<Incoming> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // The accept thread is blocked in `accept()`; connecting to our own socket is the only
        // portable way to wake it so it can observe the stop flag and return.
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn accept_loop(
    listener: UnixListener,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Incoming>,
    wake: impl Fn(),
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let Ok(stream) = stream else { continue };
        handle_connection(stream, tx, &wake);
    }
}

/// Reads one request line, queues it and waits for the reply, then writes one response line.
/// One connection at a time; a connection that sends nothing (including the self-wake connect
/// `Server::drop` makes) is silently dropped.
fn handle_connection(stream: UnixStream, tx: &mpsc::Sender<Incoming>, wake: &impl Fn()) {
    if stream.set_read_timeout(Some(READ_TIMEOUT)).is_err() {
        return;
    }
    let Ok(write_half) = stream.try_clone() else {
        return;
    };

    let mut reader = BufReader::new(stream).take(MAX_LINE_BYTES + 1);
    let mut line = String::new();
    let response = match reader.read_line(&mut line) {
        Ok(0) => return,
        Ok(_) if line.len() as u64 > MAX_LINE_BYTES => {
            Response::error("bad request: line too long")
        }
        Ok(_) => match serde_json::from_str::<Request>(line.trim_end()) {
            Ok(request) => match dispatch(request, tx, wake) {
                Some(response) => response,
                None => return,
            },
            Err(error) => Response::error(format!("bad request: {error}")),
        },
        Err(error) => Response::error(format!("bad request: {error}")),
    };

    write_response(write_half, &response);
}

/// Queues `request` and blocks for its reply, up to [`REPLY_TIMEOUT`]. `None` when the queue
/// itself is gone (the `Server` is being dropped).
fn dispatch(request: Request, tx: &mpsc::Sender<Incoming>, wake: &impl Fn()) -> Option<Response> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(Incoming {
        request,
        reply: reply_tx,
    })
    .ok()?;
    wake();
    Some(
        reply_rx
            .recv_timeout(REPLY_TIMEOUT)
            .unwrap_or_else(|_| Response::error("napkin did not answer in time")),
    )
}

fn write_response(mut writer: UnixStream, response: &Response) {
    let mut body = serde_json::to_string(response).expect("Response always serializes");
    body.push('\n');
    let _ = writer.write_all(body.as_bytes());
}
