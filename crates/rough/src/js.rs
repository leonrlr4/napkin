//! JavaScript number semantics the port depends on. Rust's own operators differ from these
//! in ways that change output (rounding direction, integer wrap-around), so every call site
//! that mirrors one of these JS operations uses the helper, never the Rust look-alike.

/// ECMAScript `ToInt32`, as applied by `Math.imul` and bitwise operators.
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    (if m >= 2_147_483_648.0 {
        m - 4_294_967_296.0
    } else {
        m
    }) as i32
}

/// JS truthiness of a number (`if (x)`, `x || y`, `x ? a : b`): `0`, `-0` and `NaN` are
/// falsy, every other value, infinities included, is truthy.
pub fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `Math.round`: halves round towards +∞ (`Math.round(-2.5) == -2`), unlike `f64::round`.
pub fn math_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Number.prototype.toFixed(digits)` for `|x| < 1e21`. JS rounds the exact binary value
/// half away from zero; Rust's `{:.N}` rounds ties to even, so `0.0009765625` (exactly
/// representable) gives `0.000976563` here and `0.000976562` with `format!`.
pub fn to_fixed(x: f64, digits: usize) -> String {
    // 1100 fractional digits hold the exact expansion of any f64.
    let exact = format!("{:.1100}", x.abs());
    let (int_part, frac) = exact.split_once('.').expect("fixed-point formatting");
    let mut kept: Vec<u8> = int_part.bytes().chain(frac.bytes().take(digits)).collect();
    if frac.as_bytes()[digits] >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let split = kept.len() - digits;
    let digits_str = String::from_utf8(kept).expect("ascii digits");
    let sign = if x < 0.0 { "-" } else { "" };
    if digits == 0 {
        format!("{sign}{digits_str}")
    } else {
        format!("{sign}{}.{}", &digits_str[..split], &digits_str[split..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int32_wraps_like_javascript() {
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(-7.9), -7);
        assert_eq!(to_int32(f64::NAN), 0);
    }

    #[test]
    fn truthy_treats_only_zero_and_nan_as_false() {
        assert!(!truthy(0.0));
        assert!(!truthy(-0.0));
        assert!(!truthy(f64::NAN));
        assert!(truthy(1e-300));
        assert!(truthy(-1.0));
        assert!(truthy(f64::INFINITY));
    }

    #[test]
    fn math_round_rounds_halves_up() {
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
    }

    #[test]
    fn to_fixed_matches_javascript() {
        // Expected strings from node: `x.toFixed(9)`.
        for (x, expected) in [
            (0.0009765625, "0.000976563"),
            (-0.0009765625, "-0.000976563"),
            (0.1, "0.100000000"),
            (-0.5000000005, "-0.500000001"),
            (1.0 / 3.0, "0.333333333"),
            (0.9999999995, "0.999999999"),
            (1.0000000005, "1.000000001"),
            (-2.0 / 3.0, "-0.666666667"),
            (123.4560000005, "123.456000000"),
            (0.0, "0.000000000"),
            (3.0517578125e-5, "0.000030518"),
            (-0.0000000001, "-0.000000000"),
            (0.99999999999, "1.000000000"),
        ] {
            assert_eq!(to_fixed(x, 9), expected, "{x}");
        }
    }
}
