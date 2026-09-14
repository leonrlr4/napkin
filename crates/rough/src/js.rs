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

/// `Math.round`: halves round towards +∞ (`Math.round(-2.5) == -2`), unlike `f64::round`.
pub fn math_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
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
    fn math_round_rounds_halves_up() {
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
    }
}
