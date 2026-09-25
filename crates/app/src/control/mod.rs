//! The napkin control protocol: one JSON request per line in, one JSON response per line out.
//! [`handler`] turns a [`Request`] into a [`Response`] without touching the socket or the GUI;
//! [`summary`] formats the `scene` and `selection` listings a `Response::output` carries;
//! [`render`] plans the region and encodes the PNG for a `render` request.

pub mod handler;
pub mod render;
pub mod summary;

use serde::{Deserialize, Serialize};

/// One line read from the control socket.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "lowercase")]
pub enum Request {
    Status,
    Scene {
        #[serde(default)]
        full: bool,
    },
    Selection {
        #[serde(default)]
        full: bool,
    },
    View,
    Apply {
        batch: serde_json::Value,
    },
    Render {
        out: std::path::PathBuf,
        target: RenderTarget,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderTarget {
    All,
    Selection,
    View,
}

/// One line written back. `output` is exactly what the client prints: to stdout when `ok`,
/// to stderr otherwise. Its format is entirely napkin's to decide; the client is thin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub output: String,
}

impl Response {
    pub fn ok(output: impl Into<String>) -> Response {
        Response {
            ok: true,
            output: output.into(),
        }
    }

    pub fn error(output: impl Into<String>) -> Response {
        Response {
            ok: false,
            output: output.into(),
        }
    }
}
