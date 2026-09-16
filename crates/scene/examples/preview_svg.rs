//! Renders a `.excalidraw` file to SVG from `scene`'s shape data, for comparing napkin's
//! lines with excalidraw.com by eye before the M3 canvas exists.
//!
//! ```text
//! cargo run -p scene --example preview_svg -- FILE.excalidraw [OUT.svg] [--dark]
//! ```
//!
//! Element placement follows `renderElementToSvg` in
//! `packages/excalidraw/renderer/staticSvgScene.ts` at the pinned commit: each element is
//! translated to its `x`/`y` and rotated about the center of its local bounds, and rough
//! drawables become paths the way roughjs's `RoughSVG.draw` writes them. It is a preview,
//! not a port: frames do not clip, arrow labels stay at their stored position, text uses
//! the bundled fonts with Excalifont's metrics for unknown families, and element types
//! `scene` loads as `Raw` (image, frame, sticky note, ...) are drawn as a dashed box with
//! the type name, as spec §1.2 plans for M3.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use rough::{Drawable, Op, OpSetType, Shape};
use scene::SceneFile;
use scene::color::apply_dark_mode_filter;
use scene::element::{Element, ElementBase, TextElement};
use scene::shape::{ElementShape, PathOp, ShapeContext, generate_element_shape};
use serde_json::Value;

/// `DEFAULT_EXPORT_PADDING`.
const PADDING: f64 = 10.0;
/// Corners farther out than this do not grow the image.
const VIEW_LIMIT: f64 = 1e7;

fn main() {
    let mut dark = false;
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        if arg == "--dark" {
            dark = true;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    let (input, output) = match paths.as_slice() {
        [input] => (input, None),
        [input, output] => (input, Some(output)),
        _ => {
            eprintln!("usage: preview_svg FILE.excalidraw [OUT.svg] [--dark]");
            std::process::exit(2);
        }
    };

    let text = std::fs::read_to_string(input)
        .unwrap_or_else(|e| fail(&format!("{}: {e}", input.display())));
    let file = SceneFile::from_json_str(&text)
        .unwrap_or_else(|e| fail(&format!("{}: {e}", input.display())));
    let svg = render(&file, dark);
    match output {
        Some(path) => {
            std::fs::write(path, svg).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())))
        }
        None => print!("{svg}"),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("preview_svg: {message}");
    std::process::exit(1);
}

/// An element's origin (`x`, `y`), its bounds relative to that origin, and its angle.
struct Placement {
    x: f64,
    y: f64,
    min: [f64; 2],
    max: [f64; 2],
    angle: f64,
}

impl Placement {
    fn from_base(base: &ElementBase) -> Placement {
        Placement {
            x: base.x,
            y: base.y,
            min: [0.0, 0.0],
            max: [base.width, base.height],
            angle: base.angle,
        }
    }

    /// Lines, arrows and freedraw: `getElementAbsoluteCoords` bounds their points.
    fn from_points(base: &ElementBase, points: &[[f64; 2]]) -> Placement {
        let mut min = [0.0_f64, 0.0_f64];
        let mut max = [0.0_f64, 0.0_f64];
        if let Some(first) = points.first() {
            min = *first;
            max = *first;
        }
        for p in points {
            for axis in 0..2 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        Placement {
            x: base.x,
            y: base.y,
            min,
            max,
            angle: base.angle,
        }
    }

    fn of(element: &Element) -> Option<Placement> {
        Some(match element {
            Element::Rectangle(g) | Element::Diamond(g) | Element::Ellipse(g) => {
                Placement::from_base(&g.base)
            }
            Element::Line(l) | Element::Arrow(l) => Placement::from_points(&l.base, &l.points),
            Element::Freedraw(f) => Placement::from_points(&f.base, &f.points),
            Element::Text(t) => Placement::from_base(&t.base),
            Element::Raw(v) => {
                let n = |key: &str| v.get(key).and_then(Value::as_f64);
                Placement {
                    x: n("x")?,
                    y: n("y")?,
                    min: [0.0, 0.0],
                    max: [n("width")?, n("height")?],
                    angle: n("angle").unwrap_or(0.0),
                }
            }
        })
    }

    /// Rotation center in local coordinates: `(x2 - x1) / 2 - (element.x - x1)`.
    fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }

    /// Absolute corners of the rotated local bounds.
    fn corners(&self) -> [[f64; 2]; 4] {
        let [cx, cy] = self.center();
        let (sin, cos) = self.angle.sin_cos();
        [
            [self.min[0], self.min[1]],
            [self.max[0], self.min[1]],
            [self.max[0], self.max[1]],
            [self.min[0], self.max[1]],
        ]
        .map(|[px, py]| {
            let (dx, dy) = (px - cx, py - cy);
            [
                self.x + cx + dx * cos - dy * sin,
                self.y + cy + dx * sin + dy * cos,
            ]
        })
    }
}

fn render(file: &SceneFile, dark: bool) -> String {
    let visible: Vec<(&Element, Placement)> = file
        .elements
        .iter()
        .filter(|e| !e.is_deleted())
        .filter_map(|e| Placement::of(e).map(|p| (e, p)))
        .collect();

    let mut min = [f64::INFINITY, f64::INFINITY];
    let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (_, placement) in &visible {
        // An element with absurd geometry would shrink everything else to nothing.
        for corner in placement.corners() {
            if !corner.iter().all(|v| v.abs() <= VIEW_LIMIT) {
                continue;
            }
            for axis in 0..2 {
                min[axis] = min[axis].min(corner[axis]);
                max[axis] = max[axis].max(corner[axis]);
            }
        }
    }
    if !min[0].is_finite() {
        min = [0.0, 0.0];
        max = [0.0, 0.0];
    }
    let origin = [min[0] - PADDING, min[1] - PADDING];
    let width = max[0] - min[0] + 2.0 * PADDING;
    let height = max[1] - min[1] + 2.0 * PADDING;

    let background = color(file.view_background_color(), dark);
    let ctx = ShapeContext {
        dark_mode: dark,
        canvas_background_color: file.view_background_color(),
    };

    let arrow_ids: HashSet<&str> = file
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Arrow(_)))
        .filter_map(Element::id)
        .collect();

    let mut body = String::new();
    let mut families = Vec::new();
    for (element, placement) in &visible {
        let [cx, cy] = placement.center();
        let transform = format!(
            "translate({} {}) rotate({} {cx} {cy})",
            placement.x - origin[0],
            placement.y - origin[1],
            placement.angle.to_degrees(),
        );
        let opacity = element.base().map_or(1.0, |b| b.opacity / 100.0);
        let opacity_attrs = if opacity == 1.0 {
            String::new()
        } else {
            format!(r#" stroke-opacity="{opacity}" fill-opacity="{opacity}""#)
        };
        match element {
            Element::Text(t) => {
                let font = Font::of(t.font_family);
                if !families.contains(&font.name) {
                    families.push(font.name);
                }
                let _ = write!(body, r#"<g transform="{transform}"{opacity_attrs}>"#);
                // Excalidraw masks the arrow under its label; covering it with the
                // background color looks the same.
                let is_arrow_label = t
                    .container_id
                    .value()
                    .is_some_and(|id| arrow_ids.contains(id.as_str()));
                if is_arrow_label {
                    let _ = write!(
                        body,
                        r#"<rect x="-4" y="-4" width="{}" height="{}" fill="{background}"/>"#,
                        t.base.width + 8.0,
                        t.base.height + 8.0,
                    );
                }
                text(&mut body, t, &font, dark);
                body.push_str("</g>\n");
            }
            Element::Raw(v) => {
                let kind = v.get("type").and_then(Value::as_str).unwrap_or("unknown");
                placeholder_box(&mut body, &transform, placement, kind);
            }
            _ => {
                let _ = write!(
                    body,
                    r#"<g transform="{transform}" stroke-linecap="round"{opacity_attrs}>"#
                );
                match generate_element_shape(element, &ctx) {
                    ElementShape::None => {}
                    ElementShape::Placeholder => {
                        body.push_str("</g>\n");
                        let kind = element.base().map_or("unknown", |b| b.kind.as_str());
                        placeholder_box(&mut body, &transform, placement, kind);
                        continue;
                    }
                    ElementShape::Drawables(drawables) => {
                        for drawable in &drawables {
                            drawable_paths(&mut body, drawable);
                        }
                    }
                    ElementShape::Freedraw { fill, stroke } => {
                        if let Some(fill) = fill {
                            drawable_paths(&mut body, &fill);
                        }
                        let stroke_color = element.base().map_or("#000000", |b| &b.stroke_color);
                        let _ = write!(
                            body,
                            r#"<path d="{}" fill="{}" stroke="none"/>"#,
                            outline_path(&stroke),
                            color(stroke_color, dark),
                        );
                    }
                }
                body.push_str("</g>\n");
            }
        }
    }

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">"#
    );
    if !families.is_empty() {
        svg.push_str("<defs><style>\n");
        for name in families {
            font_face(&mut svg, name);
        }
        svg.push_str("</style></defs>\n");
    }
    let _ = writeln!(
        svg,
        r#"<rect x="0" y="0" width="{width}" height="{height}" fill="{background}"/>"#
    );
    svg.push_str(&body);
    svg.push_str("</svg>\n");
    svg
}

/// The dashed box spec §1.2 draws for elements napkin cannot sketch, labeled with the type.
fn placeholder_box(out: &mut String, transform: &str, placement: &Placement, kind: &str) {
    let width = placement.max[0] - placement.min[0];
    let height = placement.max[1] - placement.min[1];
    let _ = writeln!(
        out,
        r##"<g transform="{transform}"><rect x="{}" y="{}" width="{width}" height="{height}" fill="none" stroke="#868e96" stroke-width="1" stroke-dasharray="6 4"/><text x="{}" y="{}" font-family="sans-serif" font-size="12" fill="#868e96">{}</text></g>"##,
        placement.min[0],
        placement.min[1],
        placement.min[0] + 4.0,
        placement.min[1] + 14.0,
        escape(kind),
    );
}

/// The file stores light-mode colors; dark mode maps them on the way out (spec §6.6).
fn color(value: &str, dark: bool) -> String {
    if dark {
        apply_dark_mode_filter(value)
    } else {
        value.to_owned()
    }
}

/// roughjs `RoughSVG.draw`: one `<path>` per op set.
fn drawable_paths(out: &mut String, drawable: &Drawable) {
    let o = &drawable.options;
    for set in &drawable.sets {
        let d = ops_path(&set.ops);
        match set.kind {
            OpSetType::Path => {
                let _ = write!(
                    out,
                    r#"<path d="{d}" stroke="{}" stroke-width="{}" fill="none"{}/>"#,
                    o.stroke,
                    o.stroke_width,
                    dash(o.stroke_line_dash.as_deref()),
                );
            }
            OpSetType::FillPath => {
                let rule = if matches!(drawable.shape, Shape::Curve | Shape::Polygon) {
                    r#" fill-rule="evenodd""#
                } else {
                    ""
                };
                let _ = write!(
                    out,
                    r#"<path d="{d}" stroke="none" stroke-width="0" fill="{}"{rule}/>"#,
                    o.fill.as_deref().unwrap_or(""),
                );
            }
            OpSetType::FillSketch => {
                let weight = if o.fill_weight < 0.0 {
                    o.stroke_width / 2.0
                } else {
                    o.fill_weight
                };
                let _ = write!(
                    out,
                    r#"<path d="{d}" stroke="{}" stroke-width="{weight}" fill="none"{}/>"#,
                    o.fill.as_deref().unwrap_or(""),
                    dash(o.fill_line_dash.as_deref()),
                );
            }
        }
    }
}

fn dash(pattern: Option<&[f64]>) -> String {
    match pattern {
        Some(p) if !p.is_empty() => {
            let values: Vec<String> = p.iter().map(f64::to_string).collect();
            format!(r#" stroke-dasharray="{}""#, values.join(" "))
        }
        _ => String::new(),
    }
}

/// roughjs `opsToPath`.
fn ops_path(ops: &[Op]) -> String {
    let mut d = String::new();
    for op in ops {
        let _ = match op {
            Op::Move([x, y]) => write!(d, "M{x} {y} "),
            Op::LineTo([x, y]) => write!(d, "L{x} {y} "),
            Op::BCurveTo([x1, y1, x2, y2, x, y]) => write!(d, "C{x1} {y1}, {x2} {y2}, {x} {y} "),
        };
    }
    d.trim_end().to_owned()
}

/// The freedraw outline as the SVG path `getSvgPathFromStroke` writes.
fn outline_path(ops: &[PathOp]) -> String {
    let mut d = String::new();
    for op in ops {
        let _ = match op {
            PathOp::Move([x, y]) => write!(d, "M{x} {y} "),
            PathOp::Quad([x1, y1, x, y]) => write!(d, "Q{x1} {y1} {x} {y} "),
            PathOp::Line([x, y]) => write!(d, "L{x} {y} "),
            PathOp::Close => write!(d, "Z "),
        };
    }
    d.trim_end().to_owned()
}

/// A bundled font (spec §6.3) and the `FONT_METADATA` metrics of the family it stands in for.
struct Font {
    name: &'static str,
    units_per_em: f64,
    ascender: f64,
    descender: f64,
}

impl Font {
    /// spec §6.3's `fontFamily` mapping; metrics from `packages/common/src/font-metadata.ts`.
    fn of(font_family: f64) -> Font {
        match font_family as i64 {
            6 | 2 | 7 | 9 | 10 => Font {
                name: "napkin-sans",
                units_per_em: 1000.0,
                ascender: 1011.0,
                descender: -353.0,
            },
            8 | 3 => Font {
                name: "napkin-code",
                units_per_em: 1000.0,
                ascender: 750.0,
                descender: -250.0,
            },
            _ => Font {
                name: "napkin-hand",
                units_per_em: 1000.0,
                ascender: 886.0,
                descender: -374.0,
            },
        }
    }
}

/// The text branch of `renderElementToSvg`.
fn text(out: &mut String, t: &TextElement, font: &Font, dark: bool) {
    let line_height = t.font_size * t.line_height.unwrap_or(1.25);
    let em = t.font_size / font.units_per_em;
    let line_gap = (line_height - em * font.ascender + em * font.descender) / 2.0;
    let vertical_offset = em * font.ascender + line_gap;
    let (x, anchor) = match t.text_align.as_str() {
        "center" => (t.base.width / 2.0, "middle"),
        "right" => (t.base.width, "end"),
        _ => (0.0, "start"),
    };
    let fill = color(&t.base.stroke_color, dark);
    let normalized = t.text.replace("\r\n", "\n").replace('\r', "\n");
    for (i, line) in normalized.split('\n').enumerate() {
        let _ = write!(
            out,
            r#"<text x="{x}" y="{}" font-family="{}, 'Noto Sans CJK TC', sans-serif" font-size="{}px" fill="{fill}" text-anchor="{anchor}" style="white-space: pre;" dominant-baseline="alphabetic">{}</text>"#,
            i as f64 * line_height + vertical_offset,
            font.name,
            t.font_size,
            escape(line),
        );
    }
}

/// Embeds a bundled font as a data URI: browsers refuse `@font-face` URLs to other local
/// files from an SVG opened with `file://`.
fn font_face(out: &mut String, name: &str) {
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return;
    };
    let path = PathBuf::from(dir)
        .join("../../assets/fonts")
        .join(format!("{name}.ttf"));
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "preview_svg: {} not found, text uses a fallback font",
            path.display()
        );
        return;
    };
    let _ = writeln!(
        out,
        r#"@font-face {{ font-family: "{name}"; src: url("data:font/ttf;base64,{}"); }}"#,
        base64(&bytes)
    );
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
