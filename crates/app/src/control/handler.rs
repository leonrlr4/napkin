//! Turns a [`Request`] into a [`Response`] against one [`Session`]: no socket I/O, no GPU
//! rasterization (decision 4). The caller (the socket server, or a test) builds a fresh
//! `Session` per request and, for a mutating request, waits for `Editor::is_idle` first
//! ([`is_mutating`]).

use std::path::Path;

use scene::editor::Editor;
use scene::env::Env;
use scene::text::TextMeasure;
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::control::summary::{element_lines, format_number, full_lines};
use crate::control::{Request, Response};

/// Everything one request needs beyond the wire protocol.
pub struct Session<'a, E: Env> {
    pub editor: &'a mut Editor<E>,
    pub path: Option<&'a Path>,
    pub unsaved: bool,
    pub save_error: Option<&'a str>,
    /// Why the canvas is read-only (an unparseable reload, spec §8), if it is.
    pub readonly: Option<&'a str>,
    pub camera: Camera,
    /// The canvas size in logical points.
    pub canvas_size: [f64; 2],
    pub measure: &'a mut dyn TextMeasure,
}

/// Whether the request changes the scene, and so must wait for `Editor::is_idle`.
pub fn is_mutating(request: &Request) -> bool {
    matches!(request, Request::Apply { .. })
}

pub fn handle<E: Env>(session: &mut Session<'_, E>, request: &Request) -> Response {
    match request {
        Request::Status => status(session),
        Request::Scene { full } => scene(session, *full),
        Request::Selection { full } => selection(session, *full),
        Request::View => view(session),
        Request::Apply { batch } => apply(session, batch),
        Request::Render { .. } => Response::error("render is not available"),
    }
}

fn file_line<E: Env>(session: &Session<'_, E>) -> String {
    match session.path {
        Some(path) => format!("file {}", path.display()),
        None => "file (none)".to_string(),
    }
}

fn status<E: Env>(session: &mut Session<'_, E>) -> Response {
    let elements = session
        .editor
        .file()
        .elements
        .iter()
        .filter(|element| !element.is_deleted())
        .count();
    let readonly = match session.readonly {
        Some(reason) => format!("readonly yes: {reason}"),
        None => "readonly no".to_string(),
    };
    Response::ok(format!(
        "{}\nunsaved {}\nsave_error {}\nelements {elements}\n{readonly}",
        file_line(session),
        if session.unsaved { "yes" } else { "no" },
        session.save_error.unwrap_or("none"),
    ))
}

fn scene<E: Env>(session: &mut Session<'_, E>, full: bool) -> Response {
    let file = session.editor.file();
    let positions: Vec<usize> = (0..file.elements.len()).collect();
    let lines = if full {
        full_lines(file, &positions)
    } else {
        element_lines(file, &positions)
    };
    Response::ok(join_output(&file_line(session), &lines))
}

fn selection<E: Env>(session: &mut Session<'_, E>, full: bool) -> Response {
    let file = session.editor.file();
    let base = session.editor.selection().positions(file);
    let lines = if full {
        let mut positions = base.clone();
        for &position in &base {
            let element = &file.elements[position];
            for (id, kind) in element.bound_elements() {
                if kind != "text" {
                    continue;
                }
                let text_position = file
                    .elements
                    .iter()
                    .position(|e| !e.is_deleted() && e.id() == Some(id));
                if let Some(text_position) = text_position
                    && !positions.contains(&text_position)
                {
                    positions.push(text_position);
                }
            }
        }
        positions.sort_unstable();
        full_lines(file, &positions)
    } else {
        element_lines(file, &base)
    };
    Response::ok(join_output(&file_line(session), &lines))
}

fn join_output(header: &str, lines: &[String]) -> String {
    let mut output = header.to_string();
    for line in lines {
        output.push('\n');
        output.push_str(line);
    }
    output
}

fn view<E: Env>(session: &mut Session<'_, E>) -> Response {
    let rect = session.camera.visible_rect(session.canvas_size);
    Response::ok(format!(
        "{} {} {} {} zoom={}",
        format_number(rect.min[0]),
        format_number(rect.min[1]),
        format_number(rect.max[0] - rect.min[0]),
        format_number(rect.max[1] - rect.min[1]),
        format_number(session.camera.zoom),
    ))
}

fn apply<E: Env>(session: &mut Session<'_, E>, batch: &Value) -> Response {
    if let Some(reason) = session.readonly {
        let output = json!({"errors": [{"message": format!("the canvas is read-only: {reason}")}]});
        return Response::error(output.to_string());
    }
    match session.editor.apply_batch(batch, &mut *session.measure) {
        Ok(report) => {
            let output = json!({
                "revision": session.editor.revision(),
                "created": report.created,
                "added": report.added,
                "updated": report.updated,
                "deleted": report.deleted,
                "warnings": report.warnings,
            });
            Response::ok(output.to_string())
        }
        Err(errors) => Response::error(json!({"errors": errors}).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use scene::sample::{self, CharWidthMeasure};

    use super::*;
    use crate::control::RenderTarget;

    struct FixedEnv;

    impl Env for FixedEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(7);
        }

        fn now_ms(&mut self) -> f64 {
            0.0
        }
    }

    fn session<'a>(
        editor: &'a mut Editor<FixedEnv>,
        measure: &'a mut CharWidthMeasure,
    ) -> Session<'a, FixedEnv> {
        Session {
            editor,
            path: Some(Path::new("/tmp/napkin-test.excalidraw")),
            unsaved: false,
            save_error: None,
            readonly: None,
            camera: Camera {
                scroll_x: 0.0,
                scroll_y: 0.0,
                zoom: 1.0,
            },
            canvas_size: [800.0, 600.0],
            measure,
        }
    }

    #[test]
    fn status_scene_and_view() {
        let mut editor = Editor::new(
            sample::file(vec![sample::generic(
                "rectangle",
                "r",
                [0.0, 0.0, 10.0, 10.0],
            )]),
            FixedEnv,
        );
        let mut measure = CharWidthMeasure;
        let mut s = session(&mut editor, &mut measure);
        assert_eq!(
            handle(&mut s, &Request::Status),
            Response::ok(
                "file /tmp/napkin-test.excalidraw\nunsaved no\nsave_error none\nelements 1\nreadonly no"
            )
        );
        assert_eq!(
            handle(&mut s, &Request::Scene { full: false }),
            Response::ok("file /tmp/napkin-test.excalidraw\nr rectangle 0 0 10 10")
        );
        assert_eq!(
            handle(&mut s, &Request::View),
            Response::ok("0 0 800 600 zoom=1")
        );
    }

    #[test]
    fn apply_reports_ids_and_rejects_on_a_readonly_canvas() {
        let mut editor = Editor::new(sample::file(vec![]), FixedEnv);
        let mut measure = CharWidthMeasure;
        let mut s = session(&mut editor, &mut measure);
        let batch = json!({"ops": [{"op": "add", "type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 10, "height": 10}]});
        let response = handle(
            &mut s,
            &Request::Apply {
                batch: batch.clone(),
            },
        );
        assert!(response.ok, "{response:?}");
        let body: Value = serde_json::from_str(&response.output).unwrap();
        assert_eq!(body["revision"], json!(1));
        assert!(body["created"]["a"].is_string());

        s.readonly = Some("invalid JSON");
        let response = handle(&mut s, &Request::Apply { batch });
        assert!(!response.ok);
        assert!(
            response.output.contains("read-only: invalid JSON"),
            "{}",
            response.output
        );
    }

    #[test]
    fn requests_round_trip_as_json() {
        let request = Request::Render {
            out: "/tmp/x.png".into(),
            target: RenderTarget::Selection,
        };
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(
            line,
            r#"{"command":"render","out":"/tmp/x.png","target":"selection"}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), request);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"command":"scene"}"#).unwrap(),
            Request::Scene { full: false }
        );
        assert!(is_mutating(&Request::Apply { batch: json!({}) }));
        assert!(!is_mutating(&Request::Scene { full: true }));
    }
}
