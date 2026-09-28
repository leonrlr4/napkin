//! The `napkin` binary's client half of the control protocol: connect, write one JSON request
//! line, read one JSON response line back.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::control::{Request, Response};

/// How long to wait for a reply after the request line is written. Matches the server's own
/// wait for the UI thread to answer, so a request that is legitimately queued behind a user
/// gesture (spec: modifying requests wait for `Editor::is_idle`) has time to complete.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub enum ClientError {
    NotRunning,
    Failed(String),
    Io(std::io::Error),
}

pub const NOT_RUNNING: &str = "napkin is not running; open it with SUPER+N";

/// Sends one request and waits up to 120 s for napkin's reply. `Ok(output)` when napkin
/// answered `ok`; `Err(ClientError::Failed(output))` when it answered with an error.
pub fn send(path: &Path, request: &Request) -> Result<String, ClientError> {
    let stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(ClientError::NotRunning);
        }
        Err(error) => return Err(ClientError::Io(error)),
    };
    stream
        .set_read_timeout(Some(REPLY_TIMEOUT))
        .map_err(ClientError::Io)?;

    let mut writer = stream.try_clone().map_err(ClientError::Io)?;
    let mut line = serde_json::to_string(request).expect("Request always serializes");
    line.push('\n');
    writer.write_all(line.as_bytes()).map_err(ClientError::Io)?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(ClientError::Io)?;

    let response: Response = serde_json::from_str(response_line.trim_end())
        .map_err(|error| ClientError::Io(std::io::Error::other(error)))?;
    if response.ok {
        Ok(response.output)
    } else {
        Err(ClientError::Failed(response.output))
    }
}
