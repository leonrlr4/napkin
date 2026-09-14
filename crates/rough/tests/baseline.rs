//! Compares the port with roughjs@4.6.4 output recorded in `tests/baseline/*.json`.
//! One test per baseline group, so each porting task turns exactly its groups green.

use std::fmt::Display;
use std::path::PathBuf;

use rough::math::Random;
use rough::path_data::{self, Segment};
use rough::{RoughGenerator, hachure_fill, points_on_curve, points_on_path};
use serde_json::{Value, json};
use testkit::rough_json::{drawable_value, options_from};
use testkit::{Case, check_group, num, point_value, points_from, to_value};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
}

fn throws(error: impl Display) -> Value {
    json!({ "throws": error.to_string() })
}

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(to_value).collect())
}

fn segments_value(segments: &[Segment]) -> Value {
    Value::Array(
        segments
            .iter()
            .map(|s| json!({ "key": s.key.to_string(), "data": numbers(&s.data) }))
            .collect(),
    )
}

/// Dispatches a RoughGenerator call. The options object is always the last argument.
fn generate(case: &Case) -> Value {
    let g = RoughGenerator::new();
    let o = options_from(case.args.last().expect("options"));
    let n = |i| case.num(i);
    let drawable = match case.call.as_str() {
        "line" => g.line(n(0), n(1), n(2), n(3), &o),
        "rectangle" => g.rectangle(n(0), n(1), n(2), n(3), &o),
        "ellipse" => g.ellipse(n(0), n(1), n(2), n(3), &o),
        "circle" => g.circle(n(0), n(1), n(2), &o),
        "linearPath" => g.linear_path(&points_from(&case.args[0]), &o),
        "arc" => {
            let closed = case.args[6].as_bool().expect("closed");
            g.arc(n(0), n(1), n(2), n(3), n(4), n(5), closed, &o)
        }
        "curve" => g.curve(&points_from(&case.args[0]), &o),
        "polygon" => g.polygon(&points_from(&case.args[0]), &o),
        "path" => match g.path(case.args[0].as_str().expect("path string"), &o) {
            Ok(d) => d,
            Err(e) => return throws(e),
        },
        other => panic!("unknown generator call {other}"),
    };
    drawable_value(&drawable)
}

fn points_value(points: &[[f64; 2]]) -> Value {
    Value::Array(points.iter().copied().map(point_value).collect())
}

fn optional_num(case: &Case, i: usize) -> Option<f64> {
    case.args.get(i).map(num)
}

#[test]
fn random() {
    check_group(&dir(), "random", |case| {
        let mut random = Random::new(case.num(0));
        Value::Array(
            (0..case.num(1) as usize)
                .map(|_| to_value(random.next()))
                .collect(),
        )
    });
}

#[test]
fn path_data() {
    check_group(&dir(), "path_data", |case| {
        let parsed = match path_data::parse_path(case.args[0].as_str().expect("path")) {
            Ok(segments) => segments,
            Err(e) => return throws(e),
        };
        match case.call.as_str() {
            "parsePath" => segments_value(&parsed),
            "absolutize" => segments_value(&path_data::absolutize(&parsed)),
            "normalize" => segments_value(&path_data::normalize(&path_data::absolutize(&parsed))),
            other => panic!("unknown call {other}"),
        }
    });
}

#[test]
fn points_on_curve() {
    check_group(&dir(), "points_on_curve", |case| {
        let points = points_from(&case.args[0]);
        match case.call.as_str() {
            "pointsOnBezierCurves" => points_value(&points_on_curve::points_on_bezier_curves(
                &points,
                case.num(1),
                optional_num(case, 2),
            )),
            "simplify" => points_value(&points_on_curve::simplify(&points, case.num(1))),
            "curveToBezier" => match points_on_curve::curve_to_bezier(&points, case.num(1)) {
                Some(out) => points_value(&out),
                None => throws("A curve must have at least three points."),
            },
            other => panic!("unknown call {other}"),
        }
    });
}

#[test]
fn points_on_path() {
    check_group(&dir(), "points_on_path", |case| {
        let d = case.args[0].as_str().expect("path");
        match points_on_path::points_on_path(d, case.num(1), optional_num(case, 2)) {
            Ok(sets) => Value::Array(sets.iter().map(|set| points_value(set)).collect()),
            Err(e) => throws(e),
        }
    });
}

#[test]
fn hachure_fill() {
    check_group(&dir(), "hachure_fill", |case| {
        let mut polygons: Vec<Vec<[f64; 2]>> = case.args[0]
            .as_array()
            .expect("polygons")
            .iter()
            .map(points_from)
            .collect();
        let lines =
            hachure_fill::hachure_lines(&mut polygons, case.num(1), case.num(2), case.num(3));
        json!({
            "lines": lines.iter().map(|l| points_value(l)).collect::<Vec<_>>(),
            "polygons": polygons.iter().map(|p| points_value(p)).collect::<Vec<_>>(),
        })
    });
}

#[test]
fn outline_linear() {
    check_group(&dir(), "outline_linear", generate);
}

#[test]
fn outline_elliptic() {
    check_group(&dir(), "outline_elliptic", generate);
}

#[test]
fn outline_path() {
    check_group(&dir(), "outline_path", generate);
}

#[test]
fn fill_solid() {
    check_group(&dir(), "fill_solid", generate);
}
