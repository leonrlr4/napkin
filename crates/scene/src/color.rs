//! Port of tinycolor2@1.6.0's string-input parsing path and Excalidraw's dark-mode color
//! filter, from `packages/common/src/colors.ts` at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! Only the string-input path of tinycolor2 is ported. `apply_dark_mode_filter` and
//! `is_transparent` only ever construct a `tinycolor` from a `&str`, so the object-input
//! branches of `inputToRGB` (a bare `{r,g,b}`/`{h,s,l}`/`{h,s,v}` object, as opposed to the
//! object `stringInputToObject` turns a string into) are never reached and are not ported.
//! Likewise, `TinyColor`'s fields are exactly tinycolor2's post-constructor `_r`, `_g`,
//! `_b`, `_a`, `_ok`: `_originalInput`, `_roundA`, `_format` and `_gradientType` only affect
//! `toString`/`toHex`/`toFilter`, none of which `applyDarkModeFilter` or `isTransparent`
//! call, so they are not tracked, and `stringInputToObject`'s `format`/`named` bookkeeping
//! is dropped for the same reason. `DARK_MODE_COLORS_CACHE` is a memoization cache, not
//! behavior, and is not ported either (per the plan).
//!
//! `clamp01` and the modification/combination functions that use it (`lighten`, `darken`,
//! `saturate`, ...) are in tinycolor2 but outside this reachable path and are not ported;
//! porting an unused function would be dead code under `-D warnings`.

use std::sync::LazyLock;

use regex::{Captures, Regex};
use rough::js::{math_round, to_int32};

// ---------------------------------------------------------------------------
// JS string/number primitives the parser depends on
// ---------------------------------------------------------------------------

/// JS regex `\s`: not Unicode `White_Space` (`regex`'s own `\s` is, and lacks `\u{85}`
/// while including it, and both differ on `\u{FEFF}`, which JS's `\s` includes and Unicode
/// `White_Space` does not). Used verbatim inside the character classes below, and to
/// mirror tinycolor2's `trimLeft`/`trimRight` and `parseFloat`'s leading-whitespace skip.
const JS_WS_CLASS: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";

fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// tinycolor2's `trimLeft`/`trimRight`, applied together as `stringInputToObject` does.
fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// The longest `StrDecimalLiteral`-shaped prefix of `s` (optional sign, digits, optional
/// `.digits`, optional exponent; `Infinity` is not supported, since no matcher capture ever
/// produces it), and the byte length consumed. `None` if `s` has no such prefix.
fn numeric_prefix(s: &str) -> Option<(f64, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        let dot = i;
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - frac_start;
        if int_digits == 0 && frac_digits == 0 {
            i = dot;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    let value: f64 = s[..i]
        .parse()
        .expect("numeric_prefix scanned a valid float literal");
    Some((value, i))
}

/// `parseFloat`: skip JS whitespace, then take the longest numeric prefix, or `NaN`.
fn js_parse_float(s: &str) -> f64 {
    let trimmed = s.trim_start_matches(is_js_whitespace);
    match numeric_prefix(trimmed) {
        Some((value, _consumed)) => value,
        None => f64::NAN,
    }
}

/// `Number(s)` / unary `+s`: like `parseFloat`, but the *entire* trimmed string must be
/// consumed, and an empty string is `0`. Used by `convertToPercentage`'s `n <= 1` check,
/// where JS's implicit `ToNumber` (not `parseFloat`) applies: a string with a trailing `%`
/// (already a percentage) fails this and is left unconverted.
fn js_to_number(s: &str) -> f64 {
    let trimmed = js_trim(s);
    if trimmed.is_empty() {
        return 0.0;
    }
    match numeric_prefix(trimmed) {
        Some((value, consumed)) if consumed == trimmed.len() => value,
        _ => f64::NAN,
    }
}

// ---------------------------------------------------------------------------
// tinycolor2 string-input parsing
// ---------------------------------------------------------------------------

/// A component value as it comes out of `stringInputToObject`: either a regex capture
/// (`rgb`/`rgba`/`hsl`/`hsla`/`hsv`/`hsva`), which is JS-string-shaped for `bound01`'s
/// `isPercentage`/`isOnePointZero` checks, or a value already computed as a number (hex,
/// `transparent`), which is not.
#[derive(Clone)]
enum Component {
    Num(f64),
    Str(String),
}

fn component_to_js_string(component: &Component) -> String {
    match component {
        // Matches JS's implicit `String(number)` for the reachable range of this port
        // (component numbers are always small finite integers or an alpha fraction).
        Component::Num(v) => format!("{v}"),
        Component::Str(s) => s.clone(),
    }
}

fn component_parse_float(component: &Component) -> f64 {
    match component {
        Component::Num(v) => *v,
        Component::Str(s) => js_parse_float(s),
    }
}

/// `isOnePointZero`.
fn is_one_point_zero(component: &Component) -> bool {
    matches!(component, Component::Str(s) if s.contains('.') && js_parse_float(s) == 1.0)
}

/// `isPercentage`.
fn is_percentage(component: &Component) -> bool {
    matches!(component, Component::Str(s) if s.contains('%'))
}

/// `isValidCSSUnit`. Always true for a `Component` this module builds (every one is either
/// a `matchers.CSS_UNIT`-shaped regex capture or a number that stringifies to one), but kept
/// as a real check because `inputToRGB` calls it as a guard.
fn is_valid_css_unit(component: &Component) -> bool {
    CSS_UNIT_RE.is_match(&component_to_js_string(component))
}

/// `convertToPercentage`.
fn convert_to_percentage(component: Component) -> Component {
    let n = match &component {
        Component::Num(v) => *v,
        Component::Str(s) => js_to_number(s),
    };
    if n <= 1.0 {
        Component::Str(format!("{}%", n * 100.0))
    } else {
        component
    }
}

/// `parseIntFromHex`.
fn parse_int_from_hex(hex: &str) -> u32 {
    u32::from_str_radix(hex, 16).expect("regex only captures [0-9a-fA-F]")
}

/// `convertHexToDecimal`.
fn convert_hex_to_decimal(hex: &str) -> f64 {
    parse_int_from_hex(hex) as f64 / 255.0
}

/// `boundAlpha`.
fn bound_alpha(component: &Component) -> f64 {
    let a = component_parse_float(component);
    if a.is_nan() || a < 0.0 || a > 1.0 {
        1.0
    } else {
        a
    }
}

/// `bound01`.
fn bound01(component: &Component, max: f64) -> f64 {
    let effective = if is_one_point_zero(component) {
        Component::Str("100%".to_owned())
    } else {
        component.clone()
    };
    let process_percent = is_percentage(&effective);
    let mut n = component_parse_float(&effective).clamp(0.0, max);
    if process_percent {
        // `parseInt(n * max, 10)`: JS first converts `n * max` to a string. For most
        // values reachable here that string is plain decimal (`n * max` is finite, at
        // most `max * max <= 360 * 360`, nowhere near the `>= 1e21` threshold where
        // `ToString` would switch to exponential notation at the top end), so `parseInt`
        // reads the same leading digits as truncating toward zero. But for
        // `0 < n * max < 1e-6`, `ToString` *does* switch to exponential notation (e.g.
        // "1e-7"), and `parseInt` then stops at the first non-digit character (`.` or
        // `e`), reading only the mantissa's leading digit -- not 0, even though the
        // truncated integer part is 0. `format!("{:e}", x)` prints that same leading
        // digit first (Rust's exponential form is normalized the same way JS's is), so
        // extracting the digits before its first non-digit character reproduces
        // `parseInt`'s answer exactly, without reproducing JS's own number-to-string
        // implementation.
        let x = n * max;
        let truncated = if x > 0.0 && x < 1e-6 {
            let exponential = format!("{x:e}");
            let leading_digits: String = exponential
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            leading_digits.parse().unwrap_or(0.0)
        } else {
            x.trunc()
        };
        n = truncated / 100.0;
    }
    if (n - max).abs() < 0.000001 {
        return 1.0;
    }
    n % max / max
}

/// `rgbToRgb`.
fn rgb_to_rgb(r: &Component, g: &Component, b: &Component) -> (f64, f64, f64) {
    (
        bound01(r, 255.0) * 255.0,
        bound01(g, 255.0) * 255.0,
        bound01(b, 255.0) * 255.0,
    )
}

/// `hslToRgb`'s nested `hue2rgb`.
fn hue2rgb(p: f64, q: f64, t: f64) -> f64 {
    let mut t = t;
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// `hslToRgb`.
fn hsl_to_rgb(h: &Component, s: &Component, l: &Component) -> (f64, f64, f64) {
    let h = bound01(h, 360.0);
    let s = bound01(s, 100.0);
    let l = bound01(l, 100.0);
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            hue2rgb(p, q, h + 1.0 / 3.0),
            hue2rgb(p, q, h),
            hue2rgb(p, q, h - 1.0 / 3.0),
        )
    };
    (r * 255.0, g * 255.0, b * 255.0)
}

/// `hsvToRgb`.
fn hsv_to_rgb(h: &Component, s: &Component, v: &Component) -> (f64, f64, f64) {
    let h = bound01(h, 360.0) * 6.0;
    let s = bound01(s, 100.0);
    let v = bound01(v, 100.0);
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let idx = (i as i64).rem_euclid(6) as usize;
    let rs = [v, q, p, p, t, v];
    let gs = [t, v, v, q, p, p];
    let bs = [p, p, t, v, v, q];
    (rs[idx] * 255.0, gs[idx] * 255.0, bs[idx] * 255.0)
}

/// What `stringInputToObject` returns, minus the `format`/`named` bookkeeping (see the
/// module doc comment). `false` (no match) is `Invalid`.
enum ParsedColor {
    Rgb {
        r: Component,
        g: Component,
        b: Component,
        a: Option<Component>,
    },
    Hsl {
        h: Component,
        s: Component,
        l: Component,
        a: Option<Component>,
    },
    Hsv {
        h: Component,
        s: Component,
        v: Component,
        a: Option<Component>,
    },
    Invalid,
}

const CSS_INTEGER: &str = r"[-+]?[0-9]+%?";
const CSS_NUMBER: &str = r"[-+]?[0-9]*\.[0-9]+%?";

fn css_unit_pattern() -> String {
    format!("(?:{CSS_NUMBER})|(?:{CSS_INTEGER})")
}

fn permissive_match3(prefix: &str) -> String {
    let unit = css_unit_pattern();
    format!(
        r"{prefix}[{JS_WS_CLASS}|\(]+({unit})[,|{JS_WS_CLASS}]+({unit})[,|{JS_WS_CLASS}]+({unit})[{JS_WS_CLASS}]*\)?"
    )
}

fn permissive_match4(prefix: &str) -> String {
    let unit = css_unit_pattern();
    format!(
        r"{prefix}[{JS_WS_CLASS}|\(]+({unit})[,|{JS_WS_CLASS}]+({unit})[,|{JS_WS_CLASS}]+({unit})[,|{JS_WS_CLASS}]+({unit})[{JS_WS_CLASS}]*\)?"
    )
}

/// `matchers.CSS_UNIT`.
static CSS_UNIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&css_unit_pattern()).unwrap());
/// `matchers.rgb`.
static RGB_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match3("rgb")).unwrap());
/// `matchers.rgba`.
static RGBA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match4("rgba")).unwrap());
/// `matchers.hsl`.
static HSL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match3("hsl")).unwrap());
/// `matchers.hsla`.
static HSLA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match4("hsla")).unwrap());
/// `matchers.hsv`.
static HSV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match3("hsv")).unwrap());
/// `matchers.hsva`.
static HSVA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&permissive_match4("hsva")).unwrap());
/// `matchers.hex3`.
static HEX3_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#?([0-9a-fA-F]{1})([0-9a-fA-F]{1})([0-9a-fA-F]{1})$").unwrap());
/// `matchers.hex6`.
static HEX6_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#?([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap());
/// `matchers.hex4`.
static HEX4_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#?([0-9a-fA-F]{1})([0-9a-fA-F]{1})([0-9a-fA-F]{1})([0-9a-fA-F]{1})$").unwrap()
});
/// `matchers.hex8`.
static HEX8_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#?([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap()
});

fn str_component(captures: &Captures, i: usize) -> Component {
    Component::Str(captures[i].to_owned())
}

fn double_hex_digit(captures: &Captures, i: usize) -> String {
    format!("{0}{0}", &captures[i])
}

/// The `matchers.rgb` .. `matchers.hex3` cascade inside `stringInputToObject`, run on an
/// already-trimmed-and-lowercased (and, for a named color, substituted) string.
fn match_color_matchers(color: &str) -> ParsedColor {
    if let Some(c) = RGB_RE.captures(color) {
        return ParsedColor::Rgb {
            r: str_component(&c, 1),
            g: str_component(&c, 2),
            b: str_component(&c, 3),
            a: None,
        };
    }
    if let Some(c) = RGBA_RE.captures(color) {
        return ParsedColor::Rgb {
            r: str_component(&c, 1),
            g: str_component(&c, 2),
            b: str_component(&c, 3),
            a: Some(str_component(&c, 4)),
        };
    }
    if let Some(c) = HSL_RE.captures(color) {
        return ParsedColor::Hsl {
            h: str_component(&c, 1),
            s: str_component(&c, 2),
            l: str_component(&c, 3),
            a: None,
        };
    }
    if let Some(c) = HSLA_RE.captures(color) {
        return ParsedColor::Hsl {
            h: str_component(&c, 1),
            s: str_component(&c, 2),
            l: str_component(&c, 3),
            a: Some(str_component(&c, 4)),
        };
    }
    if let Some(c) = HSV_RE.captures(color) {
        return ParsedColor::Hsv {
            h: str_component(&c, 1),
            s: str_component(&c, 2),
            v: str_component(&c, 3),
            a: None,
        };
    }
    if let Some(c) = HSVA_RE.captures(color) {
        return ParsedColor::Hsv {
            h: str_component(&c, 1),
            s: str_component(&c, 2),
            v: str_component(&c, 3),
            a: Some(str_component(&c, 4)),
        };
    }
    if let Some(c) = HEX8_RE.captures(color) {
        return ParsedColor::Rgb {
            r: Component::Num(parse_int_from_hex(&c[1]) as f64),
            g: Component::Num(parse_int_from_hex(&c[2]) as f64),
            b: Component::Num(parse_int_from_hex(&c[3]) as f64),
            a: Some(Component::Num(convert_hex_to_decimal(&c[4]))),
        };
    }
    if let Some(c) = HEX6_RE.captures(color) {
        return ParsedColor::Rgb {
            r: Component::Num(parse_int_from_hex(&c[1]) as f64),
            g: Component::Num(parse_int_from_hex(&c[2]) as f64),
            b: Component::Num(parse_int_from_hex(&c[3]) as f64),
            a: None,
        };
    }
    if let Some(c) = HEX4_RE.captures(color) {
        let (d1, d2, d3, d4) = (
            double_hex_digit(&c, 1),
            double_hex_digit(&c, 2),
            double_hex_digit(&c, 3),
            double_hex_digit(&c, 4),
        );
        return ParsedColor::Rgb {
            r: Component::Num(parse_int_from_hex(&d1) as f64),
            g: Component::Num(parse_int_from_hex(&d2) as f64),
            b: Component::Num(parse_int_from_hex(&d3) as f64),
            a: Some(Component::Num(convert_hex_to_decimal(&d4))),
        };
    }
    if let Some(c) = HEX3_RE.captures(color) {
        let (d1, d2, d3) = (
            double_hex_digit(&c, 1),
            double_hex_digit(&c, 2),
            double_hex_digit(&c, 3),
        );
        return ParsedColor::Rgb {
            r: Component::Num(parse_int_from_hex(&d1) as f64),
            g: Component::Num(parse_int_from_hex(&d2) as f64),
            b: Component::Num(parse_int_from_hex(&d3) as f64),
            a: None,
        };
    }
    ParsedColor::Invalid
}

/// `stringInputToObject`.
fn string_input_to_object(color: &str) -> ParsedColor {
    let trimmed = js_trim(color).to_lowercase();
    if let Some(hex) = named_color_hex(&trimmed) {
        return match_color_matchers(hex);
    }
    if trimmed == "transparent" {
        return ParsedColor::Rgb {
            r: Component::Num(0.0),
            g: Component::Num(0.0),
            b: Component::Num(0.0),
            a: Some(Component::Num(0.0)),
        };
    }
    match_color_matchers(&trimmed)
}

/// `inputToRGB`, string-input branch only (see the module doc comment).
fn input_to_rgb(color: &str) -> (f64, f64, f64, f64, bool) {
    let parsed = string_input_to_object(color);
    let mut ok = false;
    let mut rgb = (0.0, 0.0, 0.0);
    let mut a_component = None;
    match parsed {
        ParsedColor::Rgb { r, g, b, a }
            if is_valid_css_unit(&r) && is_valid_css_unit(&g) && is_valid_css_unit(&b) =>
        {
            rgb = rgb_to_rgb(&r, &g, &b);
            ok = true;
            a_component = a;
        }
        ParsedColor::Hsv { h, s, v, a }
            if is_valid_css_unit(&h) && is_valid_css_unit(&s) && is_valid_css_unit(&v) =>
        {
            let s = convert_to_percentage(s);
            let v = convert_to_percentage(v);
            rgb = hsv_to_rgb(&h, &s, &v);
            ok = true;
            a_component = a;
        }
        ParsedColor::Hsl { h, s, l, a }
            if is_valid_css_unit(&h) && is_valid_css_unit(&s) && is_valid_css_unit(&l) =>
        {
            let s = convert_to_percentage(s);
            let l = convert_to_percentage(l);
            rgb = hsl_to_rgb(&h, &s, &l);
            ok = true;
            a_component = a;
        }
        _ => {}
    }
    let a = bound_alpha(&a_component.unwrap_or(Component::Num(1.0)));
    (
        rgb.0.clamp(0.0, 255.0),
        rgb.1.clamp(0.0, 255.0),
        rgb.2.clamp(0.0, 255.0),
        a,
        ok,
    )
}

/// tinycolor2's `names`, keyed lowercase, valued as the (hashless) hex string tinycolor2
/// substitutes for the name before running it back through the matchers above.
fn named_color_hex(name: &str) -> Option<&'static str> {
    Some(match name {
        "aliceblue" => "f0f8ff",
        "antiquewhite" => "faebd7",
        "aqua" => "0ff",
        "aquamarine" => "7fffd4",
        "azure" => "f0ffff",
        "beige" => "f5f5dc",
        "bisque" => "ffe4c4",
        "black" => "000",
        "blanchedalmond" => "ffebcd",
        "blue" => "00f",
        "blueviolet" => "8a2be2",
        "brown" => "a52a2a",
        "burlywood" => "deb887",
        "burntsienna" => "ea7e5d",
        "cadetblue" => "5f9ea0",
        "chartreuse" => "7fff00",
        "chocolate" => "d2691e",
        "coral" => "ff7f50",
        "cornflowerblue" => "6495ed",
        "cornsilk" => "fff8dc",
        "crimson" => "dc143c",
        "cyan" => "0ff",
        "darkblue" => "00008b",
        "darkcyan" => "008b8b",
        "darkgoldenrod" => "b8860b",
        "darkgray" => "a9a9a9",
        "darkgreen" => "006400",
        "darkgrey" => "a9a9a9",
        "darkkhaki" => "bdb76b",
        "darkmagenta" => "8b008b",
        "darkolivegreen" => "556b2f",
        "darkorange" => "ff8c00",
        "darkorchid" => "9932cc",
        "darkred" => "8b0000",
        "darksalmon" => "e9967a",
        "darkseagreen" => "8fbc8f",
        "darkslateblue" => "483d8b",
        "darkslategray" => "2f4f4f",
        "darkslategrey" => "2f4f4f",
        "darkturquoise" => "00ced1",
        "darkviolet" => "9400d3",
        "deeppink" => "ff1493",
        "deepskyblue" => "00bfff",
        "dimgray" => "696969",
        "dimgrey" => "696969",
        "dodgerblue" => "1e90ff",
        "firebrick" => "b22222",
        "floralwhite" => "fffaf0",
        "forestgreen" => "228b22",
        "fuchsia" => "f0f",
        "gainsboro" => "dcdcdc",
        "ghostwhite" => "f8f8ff",
        "gold" => "ffd700",
        "goldenrod" => "daa520",
        "gray" => "808080",
        "green" => "008000",
        "greenyellow" => "adff2f",
        "grey" => "808080",
        "honeydew" => "f0fff0",
        "hotpink" => "ff69b4",
        "indianred" => "cd5c5c",
        "indigo" => "4b0082",
        "ivory" => "fffff0",
        "khaki" => "f0e68c",
        "lavender" => "e6e6fa",
        "lavenderblush" => "fff0f5",
        "lawngreen" => "7cfc00",
        "lemonchiffon" => "fffacd",
        "lightblue" => "add8e6",
        "lightcoral" => "f08080",
        "lightcyan" => "e0ffff",
        "lightgoldenrodyellow" => "fafad2",
        "lightgray" => "d3d3d3",
        "lightgreen" => "90ee90",
        "lightgrey" => "d3d3d3",
        "lightpink" => "ffb6c1",
        "lightsalmon" => "ffa07a",
        "lightseagreen" => "20b2aa",
        "lightskyblue" => "87cefa",
        "lightslategray" => "789",
        "lightslategrey" => "789",
        "lightsteelblue" => "b0c4de",
        "lightyellow" => "ffffe0",
        "lime" => "0f0",
        "limegreen" => "32cd32",
        "linen" => "faf0e6",
        "magenta" => "f0f",
        "maroon" => "800000",
        "mediumaquamarine" => "66cdaa",
        "mediumblue" => "0000cd",
        "mediumorchid" => "ba55d3",
        "mediumpurple" => "9370db",
        "mediumseagreen" => "3cb371",
        "mediumslateblue" => "7b68ee",
        "mediumspringgreen" => "00fa9a",
        "mediumturquoise" => "48d1cc",
        "mediumvioletred" => "c71585",
        "midnightblue" => "191970",
        "mintcream" => "f5fffa",
        "mistyrose" => "ffe4e1",
        "moccasin" => "ffe4b5",
        "navajowhite" => "ffdead",
        "navy" => "000080",
        "oldlace" => "fdf5e6",
        "olive" => "808000",
        "olivedrab" => "6b8e23",
        "orange" => "ffa500",
        "orangered" => "ff4500",
        "orchid" => "da70d6",
        "palegoldenrod" => "eee8aa",
        "palegreen" => "98fb98",
        "paleturquoise" => "afeeee",
        "palevioletred" => "db7093",
        "papayawhip" => "ffefd5",
        "peachpuff" => "ffdab9",
        "peru" => "cd853f",
        "pink" => "ffc0cb",
        "plum" => "dda0dd",
        "powderblue" => "b0e0e6",
        "purple" => "800080",
        "rebeccapurple" => "663399",
        "red" => "f00",
        "rosybrown" => "bc8f8f",
        "royalblue" => "4169e1",
        "saddlebrown" => "8b4513",
        "salmon" => "fa8072",
        "sandybrown" => "f4a460",
        "seagreen" => "2e8b57",
        "seashell" => "fff5ee",
        "sienna" => "a0522d",
        "silver" => "c0c0c0",
        "skyblue" => "87ceeb",
        "slateblue" => "6a5acd",
        "slategray" => "708090",
        "slategrey" => "708090",
        "snow" => "fffafa",
        "springgreen" => "00ff7f",
        "steelblue" => "4682b4",
        "tan" => "d2b48c",
        "teal" => "008080",
        "thistle" => "d8bfd8",
        "tomato" => "ff6347",
        "turquoise" => "40e0d0",
        "violet" => "ee82ee",
        "wheat" => "f5deb3",
        "white" => "fff",
        "whitesmoke" => "f5f5f5",
        "yellow" => "ff0",
        "yellowgreen" => "9acd32",
        _ => return None,
    })
}

/// tinycolor2's `_r`/`_g`/`_b`/`_a`/`_ok` fields after the constructor, for a color
/// constructed from a string (see the module doc comment).
pub struct TinyColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
    pub ok: bool,
}

/// `tinycolor(color)`.
pub fn tinycolor(input: &str) -> TinyColor {
    let (r, g, b, a, ok) = input_to_rgb(input);
    // "Don't let the range of [0,255] come back in [0,1]": a component under 1 is rounded
    // right away, everything else is left fractional until `toRgb()` rounds it.
    let round_if_small = |c: f64| if c < 1.0 { math_round(c) } else { c };
    TinyColor {
        r: round_if_small(r),
        g: round_if_small(g),
        b: round_if_small(b),
        a,
        ok,
    }
}

/// `toRgb()`'s `r`/`g`/`b` (its `a` is `_a` unrounded, i.e. `TinyColor::a`).
fn to_rgb_rounded(tc: &TinyColor) -> (f64, f64, f64) {
    (math_round(tc.r), math_round(tc.g), math_round(tc.b))
}

// ---------------------------------------------------------------------------
// Excalidraw dark-mode color filter
// ---------------------------------------------------------------------------

const DARK_MODE_FILTER_INVERT_PERCENT: f64 = 93.0;
const DARK_MODE_FILTER_HUE_ROTATE_DEGREES: f64 = 180.0;

/// `@excalidraw/math`'s `clamp`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

/// `degreesToRadians`.
fn degrees_to_radians(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// `cssHueRotate`.
fn css_hue_rotate(red: f64, green: f64, blue: f64, degrees: f64) -> (f64, f64, f64) {
    let r = red / 255.0;
    let g = green / 255.0;
    let b = blue / 255.0;

    let a = degrees_to_radians(degrees);
    let c = a.cos();
    let s = a.sin();

    let matrix = [
        0.213 + c * 0.787 - s * 0.213,
        0.715 - c * 0.715 - s * 0.715,
        0.072 - c * 0.072 + s * 0.928,
        0.213 - c * 0.213 + s * 0.143,
        0.715 + c * 0.285 + s * 0.14,
        0.072 - c * 0.072 - s * 0.283,
        0.213 - c * 0.213 - s * 0.787,
        0.715 - c * 0.715 + s * 0.715,
        0.072 + c * 0.928 + s * 0.072,
    ];

    let new_r = r * matrix[0] + g * matrix[1] + b * matrix[2];
    let new_g = r * matrix[3] + g * matrix[4] + b * matrix[5];
    let new_b = r * matrix[6] + g * matrix[7] + b * matrix[8];

    (
        math_round(new_r.clamp(0.0, 1.0) * 255.0),
        math_round(new_g.clamp(0.0, 1.0) * 255.0),
        math_round(new_b.clamp(0.0, 1.0) * 255.0),
    )
}

/// `cssInvert`.
fn css_invert(r: f64, g: f64, b: f64, percent: f64) -> (f64, f64, f64) {
    let p = clamp(percent, 0.0, 100.0) / 100.0;
    let invert_component =
        |color: f64| math_round(clamp(color * (1.0 - p) + (255.0 - color) * p, 0.0, 255.0));
    (
        invert_component(r),
        invert_component(g),
        invert_component(b),
    )
}

/// `rgbToHex`.
fn rgb_to_hex(r: f64, g: f64, b: f64, a: Option<f64>) -> String {
    let ri = to_int32(r);
    let gi = to_int32(g);
    let bi = to_int32(b);
    let hex6 = format!("#{:06x}", (ri << 16) | (gi << 8) | bi);
    match a {
        Some(alpha) if alpha < 1.0 => {
            let alpha_byte = math_round(alpha * 255.0) as u32;
            format!("{hex6}{alpha_byte:02x}")
        }
        _ => hex6,
    }
}

/// `applyDarkModeFilter`, always `enable = true` (napkin has no "already dark" state to skip
/// re-filtering, unlike the editor call site).
pub fn apply_dark_mode_filter(color: &str) -> String {
    let tc = tinycolor(color);
    let alpha = tc.a;
    let (r, g, b) = to_rgb_rounded(&tc);
    let inverted = css_invert(r, g, b, DARK_MODE_FILTER_INVERT_PERCENT);
    let rotated = css_hue_rotate(
        inverted.0,
        inverted.1,
        inverted.2,
        DARK_MODE_FILTER_HUE_ROTATE_DEGREES,
    );
    rgb_to_hex(rotated.0, rotated.1, rotated.2, Some(alpha))
}

/// `isTransparent`.
pub fn is_transparent(color: &str) -> bool {
    tinycolor(color).a == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_parse_float_reads_a_prefix_and_stops_at_percent() {
        // tinycolor2's `bound01` relies on this: "50%" bounds to a number via `parseFloat`
        // stopping at the `%`, not failing on it.
        assert_eq!(js_parse_float("50%"), 50.0);
        assert_eq!(js_parse_float("-5.5abc"), -5.5);
        assert_eq!(js_parse_float(".5"), 0.5);
        assert!(js_parse_float("not a number").is_nan());
        assert!(js_parse_float("%50").is_nan());
    }

    #[test]
    fn js_to_number_requires_the_whole_trimmed_string() {
        // `convertToPercentage`'s `n <= 1` uses `ToNumber`, not `parseFloat`: a trailing
        // `%` makes the whole string fail (NaN), unlike `js_parse_float`.
        assert_eq!(js_to_number("1"), 1.0);
        assert_eq!(js_to_number("  0.5  "), 0.5);
        assert!(js_to_number("50%").is_nan());
        assert_eq!(js_to_number(""), 0.0);
    }

    #[test]
    fn is_js_whitespace_matches_the_js_regex_class_not_unicode_white_space() {
        // `﻿` (BOM) is JS `\s` but not Unicode `White_Space`, which `regex`'s own
        // `\s` uses; `` (NEL) is the opposite case.
        assert!(is_js_whitespace('\u{FEFF}'));
        assert!(!is_js_whitespace('\u{85}'));
        assert_eq!(js_trim("\u{FEFF} #fff \u{FEFF}"), "#fff");
    }

    #[test]
    fn is_one_point_zero_requires_a_literal_dot_equal_to_one() {
        assert!(is_one_point_zero(&Component::Str("1.0".to_owned())));
        assert!(!is_one_point_zero(&Component::Str("1".to_owned())));
        assert!(!is_one_point_zero(&Component::Str("2.0".to_owned())));
        assert!(!is_one_point_zero(&Component::Num(1.0)));
    }
}
