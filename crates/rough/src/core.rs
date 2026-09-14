//! `bin/core.d.ts`: options, ops and drawables.

pub type Point = [f64; 2];

/// rough.js `Options`: every field optional. `None` means the key is absent from the JS
/// object, so the generator default applies (`Object.assign({}, defaults, options)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub max_randomness_offset: Option<f64>,
    pub roughness: Option<f64>,
    pub bowing: Option<f64>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    pub curve_fitting: Option<f64>,
    pub curve_tightness: Option<f64>,
    pub curve_step_count: Option<f64>,
    pub fill: Option<String>,
    pub fill_style: Option<String>,
    pub fill_weight: Option<f64>,
    pub hachure_angle: Option<f64>,
    pub hachure_gap: Option<f64>,
    pub simplification: Option<f64>,
    pub dash_offset: Option<f64>,
    pub dash_gap: Option<f64>,
    pub zigzag_offset: Option<f64>,
    pub seed: Option<f64>,
    pub stroke_line_dash: Option<Vec<f64>>,
    pub stroke_line_dash_offset: Option<f64>,
    pub fill_line_dash: Option<Vec<f64>>,
    pub fill_line_dash_offset: Option<f64>,
    pub disable_multi_stroke: Option<bool>,
    pub disable_multi_stroke_fill: Option<bool>,
    pub preserve_vertices: Option<bool>,
    pub fixed_decimal_place_digits: Option<f64>,
    pub fill_shape_roughness_gain: Option<f64>,
}

/// rough.js `ResolvedOptions`, minus the `randomizer` it carries at runtime (the port keeps
/// that in the renderer's private context, so a `Drawable` stays `Send`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedOptions {
    pub max_randomness_offset: f64,
    pub roughness: f64,
    pub bowing: f64,
    pub stroke: String,
    pub stroke_width: f64,
    pub curve_fitting: f64,
    pub curve_tightness: f64,
    pub curve_step_count: f64,
    pub fill_style: String,
    pub fill_weight: f64,
    pub hachure_angle: f64,
    pub hachure_gap: f64,
    pub dash_offset: f64,
    pub dash_gap: f64,
    pub zigzag_offset: f64,
    pub seed: f64,
    pub disable_multi_stroke: bool,
    pub disable_multi_stroke_fill: bool,
    pub preserve_vertices: bool,
    pub fill_shape_roughness_gain: f64,
    pub fill: Option<String>,
    pub simplification: Option<f64>,
    pub stroke_line_dash: Option<Vec<f64>>,
    pub stroke_line_dash_offset: Option<f64>,
    pub fill_line_dash: Option<Vec<f64>>,
    pub fill_line_dash_offset: Option<f64>,
    pub fixed_decimal_place_digits: Option<f64>,
}

impl Default for ResolvedOptions {
    /// `RoughGenerator`'s `defaultOptions` (bin/generator.js).
    fn default() -> Self {
        ResolvedOptions {
            max_randomness_offset: 2.0,
            roughness: 1.0,
            bowing: 1.0,
            stroke: "#000".to_owned(),
            stroke_width: 1.0,
            curve_tightness: 0.0,
            curve_fitting: 0.95,
            curve_step_count: 9.0,
            fill_style: "hachure".to_owned(),
            fill_weight: -1.0,
            hachure_angle: -41.0,
            hachure_gap: -1.0,
            dash_offset: -1.0,
            dash_gap: -1.0,
            zigzag_offset: -1.0,
            seed: 0.0,
            disable_multi_stroke: false,
            disable_multi_stroke_fill: false,
            preserve_vertices: false,
            fill_shape_roughness_gain: 0.8,
            fill: None,
            simplification: None,
            stroke_line_dash: None,
            stroke_line_dash_offset: None,
            fill_line_dash: None,
            fill_line_dash_offset: None,
            fixed_decimal_place_digits: None,
        }
    }
}

impl ResolvedOptions {
    /// `Object.assign({}, self, options)`.
    pub fn merge(&self, options: &Options) -> ResolvedOptions {
        let o = options.clone();
        let d = self.clone();
        ResolvedOptions {
            max_randomness_offset: o.max_randomness_offset.unwrap_or(d.max_randomness_offset),
            roughness: o.roughness.unwrap_or(d.roughness),
            bowing: o.bowing.unwrap_or(d.bowing),
            stroke: o.stroke.unwrap_or(d.stroke),
            stroke_width: o.stroke_width.unwrap_or(d.stroke_width),
            curve_fitting: o.curve_fitting.unwrap_or(d.curve_fitting),
            curve_tightness: o.curve_tightness.unwrap_or(d.curve_tightness),
            curve_step_count: o.curve_step_count.unwrap_or(d.curve_step_count),
            fill_style: o.fill_style.unwrap_or(d.fill_style),
            fill_weight: o.fill_weight.unwrap_or(d.fill_weight),
            hachure_angle: o.hachure_angle.unwrap_or(d.hachure_angle),
            hachure_gap: o.hachure_gap.unwrap_or(d.hachure_gap),
            dash_offset: o.dash_offset.unwrap_or(d.dash_offset),
            dash_gap: o.dash_gap.unwrap_or(d.dash_gap),
            zigzag_offset: o.zigzag_offset.unwrap_or(d.zigzag_offset),
            seed: o.seed.unwrap_or(d.seed),
            disable_multi_stroke: o.disable_multi_stroke.unwrap_or(d.disable_multi_stroke),
            disable_multi_stroke_fill: o
                .disable_multi_stroke_fill
                .unwrap_or(d.disable_multi_stroke_fill),
            preserve_vertices: o.preserve_vertices.unwrap_or(d.preserve_vertices),
            fill_shape_roughness_gain: o
                .fill_shape_roughness_gain
                .unwrap_or(d.fill_shape_roughness_gain),
            fill: o.fill.or(d.fill),
            simplification: o.simplification.or(d.simplification),
            stroke_line_dash: o.stroke_line_dash.or(d.stroke_line_dash),
            stroke_line_dash_offset: o.stroke_line_dash_offset.or(d.stroke_line_dash_offset),
            fill_line_dash: o.fill_line_dash.or(d.fill_line_dash),
            fill_line_dash_offset: o.fill_line_dash_offset.or(d.fill_line_dash_offset),
            fixed_decimal_place_digits: o
                .fixed_decimal_place_digits
                .or(d.fixed_decimal_place_digits),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Move([f64; 2]),
    LineTo([f64; 2]),
    BCurveTo([f64; 6]),
}

impl Op {
    /// rough.js's `op` string.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Move(_) => "move",
            Op::LineTo(_) => "lineTo",
            Op::BCurveTo(_) => "bcurveTo",
        }
    }

    /// rough.js's `data` array.
    pub fn data(&self) -> &[f64] {
        match self {
            Op::Move(d) | Op::LineTo(d) => d,
            Op::BCurveTo(d) => d,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpSetType {
    Path,
    FillPath,
    FillSketch,
}

impl OpSetType {
    pub fn name(&self) -> &'static str {
        match self {
            OpSetType::Path => "path",
            OpSetType::FillPath => "fillPath",
            OpSetType::FillSketch => "fillSketch",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpSet {
    pub kind: OpSetType,
    pub ops: Vec<Op>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Line,
    Rectangle,
    Ellipse,
    Circle,
    LinearPath,
    Arc,
    Curve,
    Polygon,
    Path,
}

impl Shape {
    pub fn name(&self) -> &'static str {
        match self {
            Shape::Line => "line",
            Shape::Rectangle => "rectangle",
            Shape::Ellipse => "ellipse",
            Shape::Circle => "circle",
            Shape::LinearPath => "linearPath",
            Shape::Arc => "arc",
            Shape::Curve => "curve",
            Shape::Polygon => "polygon",
            Shape::Path => "path",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawable {
    pub shape: Shape,
    pub options: ResolvedOptions,
    pub sets: Vec<OpSet>,
}
