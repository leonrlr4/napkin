//! Turns a [`Request`] into a [`Response`] against one [`Session`]: no socket I/O, no GPU
//! rasterization. The caller (`NapkinApp`, or a test) builds a fresh `Session` per request
//! and, for a mutating request, waits for `Editor::is_idle` first ([`is_mutating`]).

use std::path::Path;
use std::sync::Arc;

use scene::editor::Editor;
use scene::env::Env;
use scene::text::TextMeasure;
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::control::render::{self, Rasterize};
use crate::control::summary::{element_lines, format_number, full_lines};
use crate::control::{RenderTarget, Request, Response};

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
    /// Whether a `render` should draw the scene in dark mode.
    pub dark: bool,
    pub rasterizer: &'a mut dyn Rasterize,
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
        Request::Render { out, target } => render(session, out, *target),
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
            });
            Response::ok(output.to_string())
        }
        Err(errors) => Response::error(json!({"errors": errors}).to_string()),
    }
}

fn render<E: Env>(session: &mut Session<'_, E>, out: &Path, target: RenderTarget) -> Response {
    let plan = match render::plan(
        session.editor.file(),
        session.editor.selection(),
        target,
        session.camera,
        session.canvas_size,
    ) {
        Ok(plan) => plan,
        Err(message) => return Response::error(message),
    };
    let rgba = match session.rasterizer.rasterize(
        Arc::new(plan.scene),
        plan.camera,
        plan.size_px,
        session.dark,
    ) {
        Ok(rgba) => rgba,
        Err(message) => return Response::error(message),
    };
    let bytes = render::encode_png(&rgba, plan.size_px);
    match std::fs::write(out, bytes) {
        Ok(()) => Response::ok(format!(
            "{} {}x{}",
            out.display(),
            plan.size_px[0],
            plan.size_px[1]
        )),
        Err(error) => Response::error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use scene::SceneFile;
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

    /// A [`Rasterize`] that records every call and hands back an opaque white image, so tests
    /// can check `render` plans, dispatches and writes without a GPU.
    #[derive(Default)]
    struct FakeRasterizer {
        calls: Vec<(Arc<SceneFile>, Camera, [u32; 2], bool)>,
    }

    impl Rasterize for FakeRasterizer {
        fn rasterize(
            &mut self,
            scene: Arc<SceneFile>,
            camera: Camera,
            size_px: [u32; 2],
            dark: bool,
        ) -> Result<Vec<u8>, String> {
            self.calls.push((scene, camera, size_px, dark));
            Ok(vec![255; (size_px[0] * size_px[1] * 4) as usize])
        }
    }

    fn session<'a>(
        editor: &'a mut Editor<FixedEnv>,
        measure: &'a mut CharWidthMeasure,
        rasterizer: &'a mut dyn Rasterize,
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
            dark: false,
            rasterizer,
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
        let mut rasterizer = FakeRasterizer::default();
        let mut s = session(&mut editor, &mut measure, &mut rasterizer);
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
        let mut rasterizer = FakeRasterizer::default();
        let mut s = session(&mut editor, &mut measure, &mut rasterizer);
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

    #[test]
    fn render_plans_rasterizes_and_writes_a_decodable_png() {
        let mut editor = Editor::new(
            sample::file(vec![sample::generic(
                "rectangle",
                "r",
                [0.0, 0.0, 100.0, 50.0],
            )]),
            FixedEnv,
        );
        let mut measure = CharWidthMeasure;
        let mut rasterizer = FakeRasterizer::default();
        let mut s = session(&mut editor, &mut measure, &mut rasterizer);
        let out = std::env::temp_dir().join(format!(
            "napkin-handler-render-test-{}.png",
            std::process::id()
        ));

        let response = handle(
            &mut s,
            &Request::Render {
                out: out.clone(),
                target: RenderTarget::All,
            },
        );

        assert!(response.ok, "{}", response.output);
        assert_eq!(response.output, format!("{} 232x132", out.display()));
        assert_eq!(rasterizer.calls.len(), 1);
        assert_eq!(rasterizer.calls[0].2, [232, 132]);
        assert!(!rasterizer.calls[0].3, "dark defaults to false");

        let bytes = std::fs::read(&out).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (232, 132));
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn render_reports_a_plan_error_without_calling_the_rasterizer() {
        let mut editor = Editor::new(sample::file(vec![]), FixedEnv);
        let mut measure = CharWidthMeasure;
        let mut rasterizer = FakeRasterizer::default();
        let mut s = session(&mut editor, &mut measure, &mut rasterizer);

        let response = handle(
            &mut s,
            &Request::Render {
                out: "/tmp/napkin-handler-render-unreachable.png".into(),
                target: RenderTarget::All,
            },
        );

        assert!(!response.ok);
        assert!(rasterizer.calls.is_empty());
    }
}
