//! `bin/generator.js`. `opsToPath`, `toPaths` and `fillSketch` are not ported
//! (see the M1 plan's decision 5).

use crate::core::{Drawable, Op, OpSet, OpSetType, Options, Point, ResolvedOptions, Shape};
use crate::js::truthy;
use crate::path_data::PathError;
use crate::points_on_curve;
use crate::points_on_path;
use crate::renderer::{self, Ctx};

const NOS: &str = "none";

/// bin/generator.js `RoughGenerator`.
///
/// Every method takes an options object. rough.js's `_o(undefined)` hands the renderer the
/// generator's own `defaultOptions`, which then keeps a seed-0 randomizer for every later
/// call; Excalidraw always passes options, so that path is not ported.
#[derive(Clone, Debug, Default)]
pub struct RoughGenerator {
    default_options: ResolvedOptions,
}

/// JS `if (o.fill)`: a missing or empty string is falsy.
fn has_fill(o: &Ctx) -> bool {
    o.o.fill.as_deref().is_some_and(|f| !f.is_empty())
}

/// The character class JS regex `\s` matches, used by [`preprocess_path`]'s second step.
/// Differs from `char::is_whitespace` in exactly two codepoints (verified against V8's
/// `/\s/` for every codepoint up to U+FFFF): `char::is_whitespace` matches U+0085 (NEL),
/// which `\s` does not, and `\s` matches U+FEFF (BOM), which `char::is_whitespace` does not.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
        || matches!(
            c,
            '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
}

/// bin/generator.js `path`'s three-step string preprocessing, applied in order:
/// `.replace(/\n/g, ' ').replace(/(-\s)/g, '-').replace('/(\s\s)/g', ' ')`.
///
/// The third call's first argument is a string literal, not a `RegExp`, so
/// `String.prototype.replace` treats it as plain text and rewrites only its first
/// occurrence. Inside a JS string literal `\s` is not a recognized escape sequence, so it
/// collapses to a bare `s` (confirmed with `node -e "console.log('/(\s\s)/g')"`, which
/// prints `/(ss)/g`); the runtime search text is therefore 7 characters, not the 9-character
/// text `bin/generator.js` shows source-side. No path in this crate's baselines contains
/// either spelling, so the two are behaviorally equivalent here, but the runtime value is
/// the one a future caller could actually hit.
fn preprocess_path(d: &str) -> String {
    let step1: String = d.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();

    let mut step2 = String::with_capacity(step1.len());
    let mut chars = step1.chars().peekable();
    while let Some(c) = chars.next() {
        step2.push(c);
        if c == '-' && chars.peek().is_some_and(|&next| is_js_whitespace(next)) {
            chars.next();
        }
    }

    const LITERAL_SEARCH: &str = "/(ss)/g";
    match step2.find(LITERAL_SEARCH) {
        Some(idx) => {
            let mut step3 = String::with_capacity(step2.len());
            step3.push_str(&step2[..idx]);
            step3.push(' ');
            step3.push_str(&step2[idx + LITERAL_SEARCH.len()..]);
            step3
        }
        None => step2,
    }
}

impl RoughGenerator {
    /// bin/generator.js `constructor` without `config`.
    pub fn new() -> Self {
        RoughGenerator {
            default_options: ResolvedOptions::default(),
        }
    }

    /// bin/generator.js `line`.
    pub fn line(&self, x1: f64, y1: f64, x2: f64, y2: f64, options: &Options) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let sets = vec![renderer::line(x1, y1, x2, y2, &mut o)];
        Drawable {
            shape: Shape::Line,
            options: o.o,
            sets,
        }
    }

    /// bin/generator.js `rectangle`.
    pub fn rectangle(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        options: &Options,
    ) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        let outline = renderer::rectangle(x, y, width, height, &mut o);
        if has_fill(&o) {
            let points = vec![
                [x, y],
                [x + width, y],
                [x + width, y + height],
                [x, y + height],
            ];
            if o.o.fill_style == "solid" {
                paths.push(renderer::solid_fill_polygon(&[points], &mut o));
            } else {
                paths.push(renderer::pattern_fill_polygons(&mut [points], &mut o));
            }
        }
        if o.o.stroke != NOS {
            paths.push(outline);
        }
        Drawable {
            shape: Shape::Rectangle,
            options: o.o,
            sets: paths,
        }
    }

    /// bin/generator.js `ellipse`.
    pub fn ellipse(&self, x: f64, y: f64, width: f64, height: f64, options: &Options) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        let ellipse_params = renderer::generate_ellipse_params(width, height, &mut o);
        let ellipse_response = renderer::ellipse_with_params(x, y, &mut o, &ellipse_params);
        if has_fill(&o) {
            if o.o.fill_style == "solid" {
                let mut shape = renderer::ellipse_with_params(x, y, &mut o, &ellipse_params).opset;
                shape.kind = OpSetType::FillPath;
                paths.push(shape);
            } else {
                paths.push(renderer::pattern_fill_polygons(
                    &mut [ellipse_response.estimated_points.clone()],
                    &mut o,
                ));
            }
        }
        if o.o.stroke != NOS {
            paths.push(ellipse_response.opset);
        }
        Drawable {
            shape: Shape::Ellipse,
            options: o.o,
            sets: paths,
        }
    }

    /// bin/generator.js `circle`.
    pub fn circle(&self, x: f64, y: f64, diameter: f64, options: &Options) -> Drawable {
        let mut ret = self.ellipse(x, y, diameter, diameter, options);
        ret.shape = Shape::Circle;
        ret
    }

    /// bin/generator.js `linearPath`.
    pub fn linear_path(&self, points: &[Point], options: &Options) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let sets = vec![renderer::linear_path(points, false, &mut o)];
        Drawable {
            shape: Shape::LinearPath,
            options: o.o,
            sets,
        }
    }

    /// bin/generator.js `arc`.
    #[expect(clippy::too_many_arguments, reason = "mirrors RoughGenerator.arc")]
    pub fn arc(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        start: f64,
        stop: f64,
        closed: bool,
        options: &Options,
    ) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        let outline = renderer::arc(x, y, width, height, start, stop, closed, true, &mut o);
        if closed && has_fill(&o) {
            if o.o.fill_style == "solid" {
                // `Object.assign({}, o)`: the copy shares the randomizer the outline above
                // already created.
                let mut fill_o = o.clone();
                fill_o.o.disable_multi_stroke = true;
                let mut shape =
                    renderer::arc(x, y, width, height, start, stop, true, false, &mut fill_o);
                shape.kind = OpSetType::FillPath;
                paths.push(shape);
            } else {
                paths.push(renderer::pattern_fill_arc(
                    x, y, width, height, start, stop, &mut o,
                ));
            }
        }
        if o.o.stroke != NOS {
            paths.push(outline);
        }
        Drawable {
            shape: Shape::Arc,
            options: o.o,
            sets: paths,
        }
    }

    /// bin/generator.js `curve`.
    ///
    /// # Panics
    ///
    /// Panics if `points` is empty (the JS throws a `TypeError` reading `points[0]`).
    pub fn curve(&self, points: &[Point], options: &Options) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        let outline = renderer::curve(points, &mut o);
        if has_fill(&o) && o.o.fill.as_deref() != Some(NOS) && points.len() >= 3 {
            if o.o.fill_style == "solid" {
                // `Object.assign(Object.assign({}, o), {...})`: the copy shares the randomizer
                // the outline above already created.
                let mut fill_o = o.clone();
                fill_o.o.disable_multi_stroke = true;
                fill_o.o.roughness = if truthy(o.o.roughness) {
                    o.o.roughness + o.o.fill_shape_roughness_gain
                } else {
                    0.0
                };
                let fill_shape = renderer::curve(points, &mut fill_o);
                paths.push(OpSet {
                    kind: OpSetType::FillPath,
                    ops: _merged_shape(fill_shape.ops),
                });
            } else {
                let bcurve = points_on_curve::curve_to_bezier(points, 0.0)
                    .expect("curveToBezier gets at least three points here");
                let poly_points = points_on_curve::points_on_bezier_curves(
                    &bcurve,
                    10.0,
                    Some((1.0 + o.o.roughness) / 2.0),
                );
                paths.push(renderer::pattern_fill_polygons(&mut [poly_points], &mut o));
            }
        }
        if o.o.stroke != NOS {
            paths.push(outline);
        }
        Drawable {
            shape: Shape::Curve,
            options: o.o,
            sets: paths,
        }
    }

    /// bin/generator.js `polygon`.
    ///
    /// # Panics
    ///
    /// Panics if `points` is empty and `options` requests a pattern fill (hachure,
    /// cross-hatch, zigzag, dots, dashed or zigzag-line): `hachure_fill::hachure_lines`
    /// indexes the empty polygon's first vertex (the JS throws a `TypeError` there too).
    pub fn polygon(&self, points: &[Point], options: &Options) -> Drawable {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        let outline = renderer::linear_path(points, true, &mut o);
        if has_fill(&o) {
            if o.o.fill_style == "solid" {
                paths.push(renderer::solid_fill_polygon(&[points.to_vec()], &mut o));
            } else {
                paths.push(renderer::pattern_fill_polygons(
                    &mut [points.to_vec()],
                    &mut o,
                ));
            }
        }
        if o.o.stroke != NOS {
            paths.push(outline);
        }
        Drawable {
            shape: Shape::Polygon,
            options: o.o,
            sets: paths,
        }
    }

    /// bin/generator.js `path`.
    pub fn path(&self, d: &str, options: &Options) -> Result<Drawable, PathError> {
        let mut o = Ctx::new(self.default_options.merge(options));
        let mut paths = Vec::new();
        // JS `if (!d)`: an empty string is falsy; a string of only whitespace is truthy and
        // goes through the full path below.
        if d.is_empty() {
            return Ok(Drawable {
                shape: Shape::Path,
                options: o.o,
                sets: paths,
            });
        }
        let d = preprocess_path(d);
        // JS `hasFill`, renamed to avoid shadowing the `has_fill` helper above.
        let path_has_fill = has_fill(&o)
            && o.o.fill.as_deref() != Some("transparent")
            && o.o.fill.as_deref() != Some(NOS);
        let has_stroke = o.o.stroke != NOS;
        let simplified = o.o.simplification.is_some_and(|s| truthy(s) && s < 1.0);
        let distance = if simplified {
            let simplification_or_1 = o.o.simplification.filter(|&s| truthy(s)).unwrap_or(1.0);
            4.0 - 4.0 * simplification_or_1
        } else {
            (1.0 + o.o.roughness) / 2.0
        };
        // `pointsOnPath` runs before `svgPath`; a parse error is thrown from here first.
        let mut sets = points_on_path::points_on_path(&d, 1.0, Some(distance))?;
        let shape = renderer::svg_path(&d, &mut o)?;
        if path_has_fill {
            if o.o.fill_style == "solid" {
                if sets.len() == 1 {
                    // `Object.assign(Object.assign({}, o), {...})`: the copy shares the
                    // randomizer `shape` above already created.
                    let mut fill_o = o.clone();
                    fill_o.o.disable_multi_stroke = true;
                    fill_o.o.roughness = if truthy(o.o.roughness) {
                        o.o.roughness + o.o.fill_shape_roughness_gain
                    } else {
                        0.0
                    };
                    let fill_shape = renderer::svg_path(&d, &mut fill_o)?;
                    paths.push(OpSet {
                        kind: OpSetType::FillPath,
                        ops: _merged_shape(fill_shape.ops),
                    });
                } else {
                    paths.push(renderer::solid_fill_polygon(&sets, &mut o));
                }
            } else {
                paths.push(renderer::pattern_fill_polygons(&mut sets, &mut o));
            }
        }
        if has_stroke {
            if simplified {
                for set in &sets {
                    paths.push(renderer::linear_path(set, false, &mut o));
                }
            } else {
                paths.push(shape);
            }
        }
        Ok(Drawable {
            shape: Shape::Path,
            options: o.o,
            sets: paths,
        })
    }
}

/// bin/generator.js `_mergedShape`: keeps the first op and drops every later `move`.
fn _merged_shape(input: Vec<Op>) -> Vec<Op> {
    input
        .into_iter()
        .enumerate()
        .filter(|(i, d)| *i == 0 || !matches!(d, Op::Move(_)))
        .map(|(_, d)| d)
        .collect()
}
