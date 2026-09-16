//! JavaScript number semantics the port depends on. Rust's own operators differ from these
//! in ways that change output (rounding direction, integer wrap-around), so every call site
//! that mirrors one of these JS operations uses the helper, never the Rust look-alike.
//!
//! ## sin/cos
//!
//! `Math.sin`/`Math.cos` in node's V8 build come from `third_party/glibc`'s table-driven
//! implementation, not a portable algorithm, so they are not ported here; call sites use
//! `f64::sin`/`f64::cos` (glibc on this machine too) and document the residual gap instead,
//! pointing back to this paragraph. A last-bit difference is not always harmless: it can
//! flip a loop's continue/stop condition (a `dist_sq > min_distance` filter, an angle
//! comparison, a scanline intersection), changing how many points or lines a shape has, not
//! only where they sit — the same class of bug the `atan2`/`hypot` port above fixed.
//!
//! Measured 2026-09-16 on node 26.7.0 (V8 14.6.202.34-node.28): 2000 mouse-like freedraw
//! strokes (1000 constant-width, 1000 variable-width with simulated pressure) run through
//! JS's `getFreedrawOutlinePoints` and `scene`'s `freedraw_outline_points`, after the
//! `atan2`/`hypot` port — 0 of 2000 differ, in outline point count or in any coordinate by
//! more than 1e-9. A direct, non-outline check of 200,000 random `f64::sin`/`Math.sin`
//! pairs still disagrees in the last bit on about 3.4% of inputs, so this corpus's
//! zero-divergence rate reflects that those 1-ULP gaps rarely land on a loop boundary, not
//! that sin/cos agree.

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

/// `Math.atan`, ported from V8's `atan` (`src/base/ieee754.cc`, itself adapted from
/// fdlibm), V8 14.6.202.34 — the version node 26.7.0 (`process.versions.v8`) embeds, and
/// the one the baseline generator's node runs. Used by [`atan2`] (`x == 1.0`, and every
/// other branch reduces to `atan(fabs(y/x))`) and, through the public [`atan`] wrapper, by
/// `rough`'s dashed and zigzag-line fillers.
///
/// Rust's own `f64::atan`/`atan2` are libm (glibc on this machine), which disagrees with
/// V8's implementation in the last bit often enough to change loop bounds in `scene` (see
/// `crates/rough/tests/baseline/js_math.json`, generated from node's `Math.atan2`, and the
/// `laser_pointer`/`perfect_freehand` call sites in `scene` that switched to this port).
// The constants below are transcribed verbatim (decimal digits included) from
// `src/base/ieee754.cc` so they can be diffed against the source; clippy would rather they
// were trimmed to the shortest round-trippable form, or replaced by `std::f64::consts::*`
// where they happen to coincide with one.
#[allow(clippy::excessive_precision, clippy::approx_constant)]
fn fdlibm_atan(mut x: f64) -> f64 {
    const ATAN_HI: [f64; 4] = [
        4.63647609000806093515e-01, // atan(0.5) hi
        7.85398163397448278999e-01, // atan(1.0) hi
        9.82793723247329054082e-01, // atan(1.5) hi
        1.57079632679489655800e+00, // atan(inf) hi
    ];
    const ATAN_LO: [f64; 4] = [
        2.26987774529616870924e-17, // atan(0.5) lo
        3.06161699786838301793e-17, // atan(1.0) lo
        1.39033110312309984516e-17, // atan(1.5) lo
        6.12323399573676603587e-17, // atan(inf) lo
    ];
    const AT: [f64; 11] = [
        3.33333333333329318027e-01,
        -1.99999999998764832476e-01,
        1.42857142725034663711e-01,
        -1.11111104054623557880e-01,
        9.09088713343650656196e-02,
        -7.69187620504482999495e-02,
        6.66107313738753120669e-02,
        -5.83357013379057348645e-02,
        4.97687799461593236017e-02,
        -3.65315727442169155270e-02,
        1.62858201153657823623e-02,
    ];
    const ONE: f64 = 1.0;
    const HUGE: f64 = 1.0e300;

    let hx = (x.to_bits() >> 32) as i32;
    let ix = hx & 0x7FFFFFFF;

    if ix >= 0x44100000 {
        // |x| >= 2^66.
        let low = x.to_bits() as u32;
        if ix > 0x7FF00000 || (ix == 0x7FF00000 && low != 0) {
            return x + x; // NaN
        }
        return if hx > 0 {
            ATAN_HI[3] + ATAN_LO[3]
        } else {
            -ATAN_HI[3] - ATAN_LO[3]
        };
    }

    let id: i32;
    if ix < 0x3FDC0000 {
        // |x| < 0.4375
        if ix < 0x3E400000 && HUGE + x > ONE {
            // |x| < 2^-27, raise inexact.
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3FF30000 {
            // |x| < 1.1875
            if ix < 0x3FE60000 {
                // 7/16 <= |x| < 11/16
                id = 0;
                x = (2.0 * x - ONE) / (2.0 + x);
            } else {
                // 11/16 <= |x| < 19/16
                id = 1;
                x = (x - ONE) / (x + ONE);
            }
        } else if ix < 0x40038000 {
            // |x| < 2.4375
            id = 2;
            x = (x - 1.5) / (ONE + 1.5 * x);
        } else {
            // 2.4375 <= |x| < 2^66
            id = 3;
            x = -1.0 / x;
        }
    }

    // End of argument reduction: break the sum from i=0 to 10 of aT[i] * z^(i+1) into odd
    // and even polynomials.
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));

    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let z = ATAN_HI[id as usize] - ((x * (s1 + s2) - ATAN_LO[id as usize]) - x);
        if hx < 0 { -z } else { z }
    }
}

/// `Math.atan`, ported from V8's `fdlibm_atan` like [`atan2`]. Used at `rough`'s
/// `fillers/dashed.rs` and `fillers/zigzag_line.rs` call sites instead of `f64::atan`.
pub fn atan(x: f64) -> f64 {
    fdlibm_atan(x)
}

/// `Math.atan2`, ported from V8's `atan2` (`src/base/ieee754.cc`), same V8 version and
/// provenance as [`fdlibm_atan`], which this calls.
///
/// One departure from a literal transcription: the source's NaN test is a branch-free bit
/// trick (`(ix | ((lx | -lx) >> 31)) > 0x7FF00000`, and the same for `y`) built on C++'s
/// mixed signed/unsigned integer promotion, which Rust does not have and would be easy to
/// mistranslate. `f64::is_nan` decides the identical predicate (IEEE 754 "exponent all
/// ones, mantissa non-zero", independent of which mantissa bits are set, exactly what the
/// bit trick tests) and is used instead; every other branch below — zero/sign detection,
/// infinity detection, the `x == 1.0` fast path, and the magnitude comparison `k` — is a
/// direct line-for-line port, including using `i32` arithmetic right shifts (which, like
/// the source's, sign-extend) where the source does.
#[allow(clippy::excessive_precision, clippy::approx_constant)]
pub fn atan2(y: f64, x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const PI_O_4: f64 = 7.8539816339744827900E-01;
    const PI_O_2: f64 = 1.5707963267948965580E+00;
    const PI: f64 = 3.1415926535897931160E+00;
    const PI_LO: f64 = 1.2246467991473531772E-16;

    if x.is_nan() || y.is_nan() {
        return x + y;
    }

    if x.to_bits() == 1.0_f64.to_bits() {
        // x == 1.0 (positive, exactly).
        return fdlibm_atan(y);
    }

    let hx = (x.to_bits() >> 32) as i32;
    let hy = (y.to_bits() >> 32) as i32;

    let mut m = ((hy >> 31) & 1) | ((hx >> 30) & 2); // 2*sign(x) + sign(y)

    if y == 0.0 {
        return match m {
            0 | 1 => y, // atan(+-0, +anything) = +-0
            2 => PI + TINY,
            _ => -PI - TINY, // 3
        };
    }

    if x == 0.0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }

    // `x`/`y` are not NaN (checked above); `is_infinite` below is therefore equivalent to
    // the source's high-word-only infinity test.
    if x.is_infinite() {
        if y.is_infinite() {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY, // 3
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY, // 3
        };
    }
    if y.is_infinite() {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }

    // Compute y/x.
    let ix = hx & 0x7FFFFFFF;
    let iy = hy & 0x7FFFFFFF;
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        // |y/x| > 2**60
        m &= 1;
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        // 0 > |y|/x > -2**-60
        0.0
    } else {
        fdlibm_atan((y / x).abs()) // safe to do y/x
    };

    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI, // 3
    }
}

/// `Math.hypot(x, y)` (the two-argument form; napkin never calls it with more or fewer
/// arguments), ported from V8's Torque `FastMathHypot`/`MathHypot`'s two-argument path
/// (`src/builtins/math.tq`), V8 14.6.202.34 (same provenance as [`atan2`]). The three-and-up
/// argument path (which adds a Kahan compensation term) is not ported: dropped along with
/// every other arity, since every call site passes exactly two arguments. `f64::hypot`
/// (libm's algorithm) differs from this in the last bit often enough to matter (see
/// [`atan2`]'s doc comment), and infinity beats NaN here (checked first): `Math.hypot(NaN,
/// Infinity) === Infinity`.
pub fn hypot(x: f64, y: f64) -> f64 {
    let a = x.abs();
    let b = y.abs();
    if a == f64::INFINITY || b == f64::INFINITY {
        return f64::INFINITY;
    }
    let max = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    };
    if max.is_nan() {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    ((a / max) * (a / max) + (b / max) * (b / max)).sqrt() * max
}

/// `Number.prototype.toFixed(digits)` for `|x| < 1e21`. JS rounds the exact binary value
/// half away from zero; Rust's `{:.N}` rounds ties to even, so `0.0009765625` (exactly
/// representable) gives `0.000976563` here and `0.000976562` with `format!`.
///
/// # Panics
///
/// Panics if `x` is NaN or infinite: `format!("{:.1100}", x.abs())` prints `"NaN"` or
/// `"inf"`, which has no `.` for the split below to find. JS does not throw here;
/// `toFixed` on NaN returns `"NaN"` and on an infinity returns `"Infinity"`.
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
