//! `bin/generator.js`. `opsToPath`, `toPaths` and `fillSketch` are not ported
//! (m1-global-rules.md, decision 5).

use crate::core::{Drawable, Op, OpSet, OpSetType, Options, Point, ResolvedOptions, Shape};
use crate::js::truthy;
use crate::path_data::PathError;
use crate::points_on_curve;
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

impl RoughGenerator {
    /// bin/generator.js `constructor` without `config`.
    pub fn new() -> Self {
        RoughGenerator {
            default_options: ResolvedOptions::default(),
        }
    }

    /// bin/generator.js `constructor` with `config.options`: `this._o(config.options)`.
    pub fn with_options(options: &Options) -> Self {
        RoughGenerator {
            default_options: ResolvedOptions::default().merge(options),
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
    pub fn ellipse(
        &self,
        _x: f64,
        _y: f64,
        _width: f64,
        _height: f64,
        _options: &Options,
    ) -> Drawable {
        todo!()
    }

    /// bin/generator.js `circle`.
    pub fn circle(&self, _x: f64, _y: f64, _diameter: f64, _options: &Options) -> Drawable {
        todo!()
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
        _x: f64,
        _y: f64,
        _width: f64,
        _height: f64,
        _start: f64,
        _stop: f64,
        _closed: bool,
        _options: &Options,
    ) -> Drawable {
        todo!()
    }

    /// bin/generator.js `curve`.
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
    pub fn path(&self, _d: &str, _options: &Options) -> Result<Drawable, PathError> {
        todo!()
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
