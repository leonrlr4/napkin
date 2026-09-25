//! Field-level validation for a `batch::add` skeleton op: small functions that check one JSON
//! value and return an error message with no field name (the caller records which field it was
//! for in [`super::OpError::field`]).

use serde_json::Value;

pub(crate) fn finite(value: &Value) -> Result<f64, String> {
    match value.as_f64() {
        Some(n) if n.is_finite() => Ok(n),
        _ => Err("must be a finite number".to_owned()),
    }
}

pub(crate) fn positive(value: &Value) -> Result<f64, String> {
    let n = finite(value)?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err("must be a number greater than 0".to_owned())
    }
}

pub(crate) fn string(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "must be a string".to_owned())
}

pub(crate) fn non_empty_string(value: &Value) -> Result<String, String> {
    let s = string(value)?;
    if s.is_empty() {
        Err("must be a non-empty string".to_owned())
    } else {
        Ok(s)
    }
}

pub(crate) fn one_of(value: &Value, allowed: &[&str]) -> Result<String, String> {
    let s = string(value)?;
    if allowed.contains(&s.as_str()) {
        Ok(s)
    } else {
        Err(format!("must be one of: {}", allowed.join(", ")))
    }
}

/// `FONT_FAMILY` (`packages/common/src/constants.ts`): 4 is a retired id, deliberately absent.
const FONT_FAMILIES: &[f64] = &[1.0, 2.0, 3.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

pub(crate) fn font_family(value: &Value) -> Result<f64, String> {
    let n = finite(value)?;
    if FONT_FAMILIES.contains(&n) {
        Ok(n)
    } else {
        let names: Vec<String> = FONT_FAMILIES.iter().map(f64::to_string).collect();
        Err(format!("must be one of: {}", names.join(", ")))
    }
}

pub(crate) fn opacity(value: &Value) -> Result<f64, String> {
    let n = finite(value)?;
    if (0.0..=100.0).contains(&n) {
        Ok(n)
    } else {
        Err("must be a number from 0 to 100".to_owned())
    }
}

pub(crate) fn roughness(value: &Value) -> Result<f64, String> {
    let n = finite(value)?;
    if n >= 0.0 {
        Ok(n)
    } else {
        Err("must be a number 0 or greater".to_owned())
    }
}

pub(crate) fn group_ids(value: &Value) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "must be an array of strings".to_owned())?;
    arr.iter()
        .map(string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "must be an array of strings".to_owned())
}

/// An array of `[x, y]` finite pairs with at least `min` entries; `Err((index, message))` names
/// the bad entry (`None` for the array itself).
pub(crate) fn points(value: &Value, min: usize) -> Result<Vec<[f64; 2]>, (Option<usize>, String)> {
    let arr = value
        .as_array()
        .ok_or_else(|| (None, "must be an array of [x, y] points".to_owned()))?;
    if arr.len() < min {
        return Err((None, format!("must have at least {min} points")));
    }
    arr.iter()
        .enumerate()
        .map(|(i, p)| {
            let pair = p
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| (Some(i), "must be a [x, y] pair".to_owned()))?;
            let x = finite(&pair[0]).map_err(|_| (Some(i), "must be a [x, y] pair".to_owned()))?;
            let y = finite(&pair[1]).map_err(|_| (Some(i), "must be a [x, y] pair".to_owned()))?;
            Ok([x, y])
        })
        .collect()
}

pub(crate) const FILL_STYLES: &[&str] = &["hachure", "cross-hatch", "solid", "zigzag"];
pub(crate) const STROKE_STYLES: &[&str] = &["solid", "dashed", "dotted"];
pub(crate) const TEXT_ALIGNS: &[&str] = &["left", "center", "right"];
pub(crate) const VERTICAL_ALIGNS: &[&str] = &["top", "middle", "bottom"];
/// `Arrowhead` and `ArrowheadLegacy` (`packages/element/src/types.ts`).
pub(crate) const ARROWHEADS: &[&str] = &[
    "arrow",
    "bar",
    "circle",
    "circle_outline",
    "triangle",
    "triangle_outline",
    "diamond",
    "diamond_outline",
    "cardinality_one",
    "cardinality_many",
    "cardinality_one_or_many",
    "cardinality_exactly_one",
    "cardinality_zero_or_one",
    "cardinality_zero_or_many",
    "dot",
    "crowfoot_one",
    "crowfoot_many",
    "crowfoot_one_or_many",
];

/// The style keys every element type accepts, in `add` and `update`.
pub(crate) const STYLE_KEYS: &[&str] = &[
    "strokeColor",
    "backgroundColor",
    "fillStyle",
    "strokeWidth",
    "strokeStyle",
    "roughness",
    "opacity",
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finite_accepts_numbers_and_rejects_the_rest() {
        assert_eq!(finite(&json!(1.5)), Ok(1.5));
        assert!(finite(&json!("1.5")).is_err());
        assert!(finite(&json!(f64::NAN)).is_err());
    }

    #[test]
    fn positive_rejects_zero_and_negative() {
        assert_eq!(positive(&json!(0.1)), Ok(0.1));
        assert!(positive(&json!(0)).is_err());
        assert!(positive(&json!(-1)).is_err());
    }

    #[test]
    fn string_helpers_reject_non_strings_and_empty() {
        assert_eq!(string(&json!("a")), Ok("a".to_owned()));
        assert!(string(&json!(1)).is_err());
        assert_eq!(non_empty_string(&json!("a")), Ok("a".to_owned()));
        assert!(non_empty_string(&json!("")).is_err());
    }

    #[test]
    fn one_of_checks_membership() {
        assert_eq!(one_of(&json!("b"), &["a", "b"]), Ok("b".to_owned()));
        assert!(one_of(&json!("c"), &["a", "b"]).is_err());
    }

    #[test]
    fn font_family_checks_the_known_ids() {
        assert_eq!(font_family(&json!(6)), Ok(6.0));
        assert!(font_family(&json!(4)).is_err());
    }

    #[test]
    fn opacity_checks_the_0_to_100_range() {
        assert_eq!(opacity(&json!(50)), Ok(50.0));
        assert!(opacity(&json!(101)).is_err());
        assert!(opacity(&json!(-1)).is_err());
    }

    #[test]
    fn roughness_rejects_negative() {
        assert_eq!(roughness(&json!(0)), Ok(0.0));
        assert!(roughness(&json!(-0.1)).is_err());
    }

    #[test]
    fn group_ids_checks_array_of_strings() {
        assert_eq!(
            group_ids(&json!(["a", "b"])),
            Ok(vec!["a".to_owned(), "b".to_owned()])
        );
        assert!(group_ids(&json!(["a", 1])).is_err());
        assert!(group_ids(&json!("a")).is_err());
    }

    #[test]
    fn points_checks_minimum_count_and_pair_shape() {
        assert_eq!(
            points(&json!([[0, 0], [1, 2]]), 2),
            Ok(vec![[0.0, 0.0], [1.0, 2.0]])
        );
        assert_eq!(points(&json!([[0, 0]]), 2).unwrap_err().0, None);
        assert_eq!(
            points(&json!([[0, 0], [1, "x"]]), 2).unwrap_err().0,
            Some(1)
        );
    }
}
