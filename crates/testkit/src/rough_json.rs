//! JSON forms of rough types, matching what rough.js objects serialize to. Shared by the
//! rough and scene baseline tests.

use rough::{Drawable, Op, Options, ResolvedOptions};
use serde_json::{Map, Value, json};

use crate::{num, to_value};

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(to_value).collect())
}

/// JS options object -> `Options`. Unknown keys fail the case, so a typo in cases.mjs
/// cannot silently fall back to a default.
pub fn options_from(value: &Value) -> Options {
    let mut o = Options::default();
    let object = value.as_object().expect("options object");
    for (key, v) in object {
        let n = || Some(num(v));
        let s = || Some(v.as_str().expect("string option").to_owned());
        let b = || Some(v.as_bool().expect("bool option"));
        let list = || {
            Some(
                v.as_array()
                    .expect("array option")
                    .iter()
                    .map(num)
                    .collect(),
            )
        };
        match key.as_str() {
            "maxRandomnessOffset" => o.max_randomness_offset = n(),
            "roughness" => o.roughness = n(),
            "bowing" => o.bowing = n(),
            "stroke" => o.stroke = s(),
            "strokeWidth" => o.stroke_width = n(),
            "curveFitting" => o.curve_fitting = n(),
            "curveTightness" => o.curve_tightness = n(),
            "curveStepCount" => o.curve_step_count = n(),
            "fill" => o.fill = s(),
            "fillStyle" => o.fill_style = s(),
            "fillWeight" => o.fill_weight = n(),
            "hachureAngle" => o.hachure_angle = n(),
            "hachureGap" => o.hachure_gap = n(),
            "simplification" => o.simplification = n(),
            "dashOffset" => o.dash_offset = n(),
            "dashGap" => o.dash_gap = n(),
            "zigzagOffset" => o.zigzag_offset = n(),
            "seed" => o.seed = n(),
            "strokeLineDash" => o.stroke_line_dash = list(),
            "strokeLineDashOffset" => o.stroke_line_dash_offset = n(),
            "fillLineDash" => o.fill_line_dash = list(),
            "fillLineDashOffset" => o.fill_line_dash_offset = n(),
            "disableMultiStroke" => o.disable_multi_stroke = b(),
            "disableMultiStrokeFill" => o.disable_multi_stroke_fill = b(),
            "preserveVertices" => o.preserve_vertices = b(),
            "fixedDecimalPlaceDigits" => o.fixed_decimal_place_digits = n(),
            "fillShapeRoughnessGain" => o.fill_shape_roughness_gain = n(),
            other => panic!("unknown option {other}"),
        }
    }
    o
}

pub fn resolved_value(o: &ResolvedOptions) -> Value {
    let mut m = Map::new();
    m.insert(
        "maxRandomnessOffset".into(),
        to_value(o.max_randomness_offset),
    );
    m.insert("roughness".into(), to_value(o.roughness));
    m.insert("bowing".into(), to_value(o.bowing));
    m.insert("stroke".into(), json!(o.stroke));
    m.insert("strokeWidth".into(), to_value(o.stroke_width));
    m.insert("curveTightness".into(), to_value(o.curve_tightness));
    m.insert("curveFitting".into(), to_value(o.curve_fitting));
    m.insert("curveStepCount".into(), to_value(o.curve_step_count));
    m.insert("fillStyle".into(), json!(o.fill_style));
    m.insert("fillWeight".into(), to_value(o.fill_weight));
    m.insert("hachureAngle".into(), to_value(o.hachure_angle));
    m.insert("hachureGap".into(), to_value(o.hachure_gap));
    m.insert("dashOffset".into(), to_value(o.dash_offset));
    m.insert("dashGap".into(), to_value(o.dash_gap));
    m.insert("zigzagOffset".into(), to_value(o.zigzag_offset));
    m.insert("seed".into(), to_value(o.seed));
    m.insert("disableMultiStroke".into(), json!(o.disable_multi_stroke));
    m.insert(
        "disableMultiStrokeFill".into(),
        json!(o.disable_multi_stroke_fill),
    );
    m.insert("preserveVertices".into(), json!(o.preserve_vertices));
    m.insert(
        "fillShapeRoughnessGain".into(),
        to_value(o.fill_shape_roughness_gain),
    );
    if let Some(v) = &o.fill {
        m.insert("fill".into(), json!(v));
    }
    if let Some(v) = o.simplification {
        m.insert("simplification".into(), to_value(v));
    }
    if let Some(v) = &o.stroke_line_dash {
        m.insert("strokeLineDash".into(), numbers(v));
    }
    if let Some(v) = o.stroke_line_dash_offset {
        m.insert("strokeLineDashOffset".into(), to_value(v));
    }
    if let Some(v) = &o.fill_line_dash {
        m.insert("fillLineDash".into(), numbers(v));
    }
    if let Some(v) = o.fill_line_dash_offset {
        m.insert("fillLineDashOffset".into(), to_value(v));
    }
    if let Some(v) = o.fixed_decimal_place_digits {
        m.insert("fixedDecimalPlaceDigits".into(), to_value(v));
    }
    Value::Object(m)
}

pub fn op_value(op: &Op) -> Value {
    json!({ "op": op.name(), "data": numbers(op.data()) })
}

pub fn drawable_value(d: &Drawable) -> Value {
    json!({
        "shape": d.shape.name(),
        "options": resolved_value(&d.options),
        "sets": d.sets.iter().map(|set| json!({
            "type": set.kind.name(),
            "ops": set.ops.iter().map(op_value).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// `Options` as the JS object Excalidraw's `generateRoughOptions` returns: only set keys.
pub fn options_value(o: &Options) -> Value {
    let mut m = Map::new();
    let mut n = |key: &str, v: Option<f64>| {
        if let Some(v) = v {
            m.insert(key.into(), to_value(v));
        }
    };
    n("maxRandomnessOffset", o.max_randomness_offset);
    n("roughness", o.roughness);
    n("bowing", o.bowing);
    n("strokeWidth", o.stroke_width);
    n("curveFitting", o.curve_fitting);
    n("curveTightness", o.curve_tightness);
    n("curveStepCount", o.curve_step_count);
    n("fillWeight", o.fill_weight);
    n("hachureAngle", o.hachure_angle);
    n("hachureGap", o.hachure_gap);
    n("simplification", o.simplification);
    n("dashOffset", o.dash_offset);
    n("dashGap", o.dash_gap);
    n("zigzagOffset", o.zigzag_offset);
    n("seed", o.seed);
    n("strokeLineDashOffset", o.stroke_line_dash_offset);
    n("fillLineDashOffset", o.fill_line_dash_offset);
    n("fixedDecimalPlaceDigits", o.fixed_decimal_place_digits);
    n("fillShapeRoughnessGain", o.fill_shape_roughness_gain);
    for (key, v) in [
        ("stroke", &o.stroke),
        ("fill", &o.fill),
        ("fillStyle", &o.fill_style),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), json!(v));
        }
    }
    for (key, v) in [
        ("strokeLineDash", &o.stroke_line_dash),
        ("fillLineDash", &o.fill_line_dash),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), numbers(v));
        }
    }
    for (key, v) in [
        ("disableMultiStroke", o.disable_multi_stroke),
        ("disableMultiStrokeFill", o.disable_multi_stroke_fill),
        ("preserveVertices", o.preserve_vertices),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), json!(v));
        }
    }
    Value::Object(m)
}
