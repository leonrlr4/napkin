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
use std::time::{Duration, Instant};

use crate::control::{Request, Response};

/// A request line longer than this is rejected instead of read into memory.
const MAX_LINE_BYTES: u64 = 16 * 1024 * 1024;
/// How long a connection may take to send its request line.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the accept thread waits for the UI thread to answer a queued request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// How often `dispatch` re-checks the stop flag while waiting for a reply, so dropping the
/// `Server` never has to wait out the full [`REPLY_TIMEOUT`] for a request nobody will answer.
const REPLY_POLL: Duration = Duration::from_millis(100);

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
    /// The instant `dispatch` gives up waiting and answers its client with a timeout error.
    /// Past this point the client already has a response, and may already have retried the
    /// request; running this one anyway would apply it a second time on top of that retry.
    deadline: Instant,
}

impl Incoming {
    pub fn reply(self, response: Response) {
        // The connection may already be gone (client timed out and disconnected); nothing to
        // do about that here.
        let _ = self.reply.send(response);
    }

    /// Whether `now` is at or past the point `dispatch` stopped waiting for this request's
    /// reply. The queue drops an expired request unexecuted instead of applying it late.
    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.deadline
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
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            let _ = std::fs::remove_file(path);
            return Err(BindError::Io(error));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_stop = stop.clone();
        let thread = match std::thread::Builder::new()
            .name("napkin-control".to_string())
            .spawn(move || accept_loop(listener, &thread_stop, &tx, wake))
        {
            Ok(thread) => thread,
            Err(error) => {
                let _ = std::fs::remove_file(path);
                return Err(BindError::Io(error));
            }
        };

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
        // The accept thread may be blocked in `accept()` (connecting to our own socket is the
        // only portable way to wake it) or, if a connection is mid-request, inside `dispatch`'s
        // reply wait; `dispatch` polls `stop` on its own, so either way `join` below returns
        // within one `REPLY_POLL` interval instead of waiting out `REPLY_TIMEOUT`.
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
        handle_connection(stream, tx, &wake, stop);
    }
}

/// Reads one request line, queues it and waits for the reply, then writes one response line.
/// One connection at a time; a connection that sends nothing (including the self-wake connect
/// `Server::drop` makes) is silently dropped.
fn handle_connection(
    stream: UnixStream,
    tx: &mpsc::Sender<Incoming>,
    wake: &impl Fn(),
    stop: &AtomicBool,
) {
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
            Ok(request) => match dispatch(request, tx, wake, stop) {
                Some(response) => response,
                None => return,
            },
            Err(error) => Response::error(format!("bad request: {error}")),
        },
        Err(error) => Response::error(format!("bad request: {error}")),
    };

    write_response(write_half, &response);
}

/// Queues `request` and waits for the reply, up to [`REPLY_TIMEOUT`], re-checking `stop` every
/// [`REPLY_POLL`] so a `Server` being dropped while this is waiting never has to wait out the
/// full timeout: `Drop` sets `stop` before it can possibly join this thread, so within one poll
/// interval this returns an error response instead of blocking `Server::drop`. `None` when the
/// queue itself is gone (the `Server`'s `Receiver` was dropped, which cannot normally happen
/// before this call returns, since `Drop` joins this thread first).
fn dispatch(
    request: Request,
    tx: &mpsc::Sender<Incoming>,
    wake: &impl Fn(),
    stop: &AtomicBool,
) -> Option<Response> {
    let (reply_tx, reply_rx) = mpsc::channel();
    let deadline = Instant::now() + REPLY_TIMEOUT;
    tx.send(Incoming {
        request,
        reply: reply_tx,
        deadline,
    })
    .ok()?;
    wake();

    let mut waited = Duration::ZERO;
    loop {
        if stop.load(Ordering::Acquire) {
            return Some(Response::error("napkin is closing"));
        }
        match reply_rx.recv_timeout(REPLY_POLL) {
            Ok(response) => return Some(response),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Some(Response::error("napkin is closing"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                waited += REPLY_POLL;
                if waited >= REPLY_TIMEOUT {
                    return Some(Response::error("napkin did not answer in time"));
                }
            }
        }
    }
}

fn write_response(mut writer: UnixStream, response: &Response) {
    let mut body = serde_json::to_string(response).expect("Response always serializes");
    body.push('\n');
    let _ = writer.write_all(body.as_bytes());
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    fn test_socket(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "napkin-server-test-{}-{name}.sock",
            std::process::id()
        ))
    }

    /// A request that is queued but never `try_recv`'d (so nothing ever calls `Incoming::reply`)
    /// must not make `Server::drop` wait: `dispatch`'s poll of `stop` should answer it with an
    /// error instead, so `drop` returns almost immediately.
    #[test]
    fn dropping_the_server_answers_a_stuck_request_instead_of_hanging() {
        let path = test_socket("stuck-drop");
        let (wake_tx, wake_rx) = mpsc::channel();
        let server = Server::bind(&path, move || {
            let _ = wake_tx.send(());
        })
        .expect("binds the test socket");

        let client_path = path.clone();
        let client = std::thread::spawn(move || {
            let stream = UnixStream::connect(&client_path).expect("connects to the bound socket");
            let mut writer = stream.try_clone().expect("cloning a connected UnixStream");
            writer
                .write_all(b"{\"command\":\"status\"}\n")
                .expect("writes the request line");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("reads the response line");
            line
        });

        // `wake` runs right after the request is queued, before `dispatch` starts waiting for a
        // reply that this test deliberately never sends (no `Server::try_recv` call at all).
        wake_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the request reaches dispatch's reply wait");

        let start = Instant::now();
        drop(server);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "dropping the server must not wait for a request nobody will ever answer"
        );

        let line = client.join().expect("the client thread does not panic");
        let response: Response =
            serde_json::from_str(line.trim_end()).expect("the server still answers with JSON");
        assert!(!response.ok, "{response:?}");
    }

    #[test]
    fn an_incoming_past_its_deadline_reports_itself_expired() {
        let (reply_tx, _reply_rx) = mpsc::channel();
        let expired = Incoming {
            request: Request::Status,
            reply: reply_tx,
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert!(expired.is_expired(Instant::now()));

        let (reply_tx, _reply_rx) = mpsc::channel();
        let fresh = Incoming {
            request: Request::Status,
            reply: reply_tx,
            deadline: Instant::now() + Duration::from_secs(60),
        };
        assert!(!fresh.is_expired(Instant::now()));
    }
}
