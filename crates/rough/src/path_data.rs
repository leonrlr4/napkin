//! Port of `path-data-parser@0.1.0`: `lib/parser.js`, `lib/absolutize.js`, `lib/normalize.js`.
//! `lib/serialize.js` is not ported (see the M1 plan's decision 5).

use crate::js::{self, truthy};

/// A single path-data command: a letter key plus its numeric parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub key: char,
    pub data: Vec<f64>,
}

/// Errors `parse_path` raises, matching the messages `path-data-parser` throws.
#[derive(Clone, Debug, PartialEq)]
pub enum PathError {
    /// JS: an unmatched character makes `tokenize` return `[]`, and `parsePath` then reads
    /// `undefined.type`, throwing a `TypeError` with no further message.
    InvalidCharacter,
    ParamNotNumber {
        mode: char,
        token: String,
    },
    EndedShort,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::InvalidCharacter => write!(f, "TypeError"),
            PathError::ParamNotNumber { mode, token } => {
                write!(f, "Param not a number: {mode},{token}")
            }
            PathError::EndedShort => write!(f, "Path data ended short"),
        }
    }
}

impl std::error::Error for PathError {}

/// Number of numeric parameters each command letter takes. `lib/parser.js` `PARAMS`.
fn params_count(mode: char) -> usize {
    match mode {
        'A' | 'a' => 7,
        'C' | 'c' => 6,
        'H' | 'h' => 1,
        'L' | 'l' => 2,
        'M' | 'm' => 2,
        'Q' | 'q' => 4,
        'S' | 's' => 4,
        'T' | 't' => 2,
        'V' | 'v' => 1,
        'Z' | 'z' => 0,
        other => unreachable!("mode is always a command letter produced by tokenize: {other}"),
    }
}

fn is_command_char(c: u8) -> bool {
    matches!(
        c,
        b'a' | b'A'
            | b'c'
            | b'C'
            | b'h'
            | b'H'
            | b'l'
            | b'L'
            | b'm'
            | b'M'
            | b'q'
            | b'Q'
            | b's'
            | b'S'
            | b't'
            | b'T'
            | b'v'
            | b'V'
            | b'z'
            | b'Z'
    )
}

/// Matches `[-+]?[0-9]+(\.[0-9]*)?` or `[-+]?\.[0-9]+`, optionally followed by
/// `[eE][-+]?[0-9]+`; an `e`/`E` with no digits after it is not part of the number. Returns
/// the byte length matched, or `None` if `s` does not start with a number.
fn match_number(s: &[u8]) -> Option<usize> {
    let mut i = 0;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    let int_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let has_int = i > int_start;
    if has_int {
        if i < s.len() && s[i] == b'.' {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
        }
    } else if i < s.len() && s[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j == frac_start {
            return None;
        }
        i = j;
    } else {
        return None;
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < s.len() && (s[j] == b'-' || s[j] == b'+') {
            j += 1;
        }
        let exp_digits_start = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_digits_start {
            i = j;
        }
    }
    Some(i)
}

enum Token {
    Command(char),
    Number(f64),
    Eod,
}

/// `lib/parser.js` `tokenize`. JS returns `[]` (discarding tokens collected so far) on the
/// first unmatched character; here that becomes `Err(PathError::InvalidCharacter)`.
fn tokenize(d: &str) -> Result<Vec<Token>, PathError> {
    let bytes = d.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b',') {
            i += 1;
        } else if is_command_char(c) {
            tokens.push(Token::Command(c as char));
            i += 1;
        } else if let Some(len) = match_number(&bytes[i..]) {
            // JS: `parseFloat(text)` to a string via a template literal, and later
            // `+token.text` to get the number back out. That round trip is a no-op for
            // every value seen in the baselines, so this parses the matched span directly.
            // One known divergence: for "-0", JS's `${parseFloat("-0")}` stringifies to
            // "0" (`String(-0) === "0"`), so `+token.text` yields `+0`, while parsing the
            // span directly here keeps `-0.0`. No baseline exercises "-0", and no shape
            // rendering distinguishes signed zero, so this is left as is.
            let value: f64 = d[i..i + len]
                .parse()
                .expect("match_number only matches valid float syntax");
            tokens.push(Token::Number(value));
            i += len;
        } else {
            return Err(PathError::InvalidCharacter);
        }
    }
    tokens.push(Token::Eod);
    Ok(tokens)
}

/// `lib/parser.js` `parsePath`.
pub fn parse_path(d: &str) -> Result<Vec<Segment>, PathError> {
    let tokens = tokenize(d)?;
    let mut segments = Vec::new();
    let mut mode: Option<char> = None; // JS: 'BOD'
    let mut index = 0usize;
    let mut token = &tokens[index];
    while !matches!(token, Token::Eod) {
        let count;
        match (mode, token) {
            (None, Token::Command(c)) if *c == 'M' || *c == 'm' => {
                index += 1;
                count = params_count(*c);
                mode = Some(*c);
            }
            (None, _) => {
                return parse_path(&format!("M0,0{d}"));
            }
            (Some(m), Token::Number(_)) => {
                count = params_count(m);
            }
            (Some(_), Token::Command(c)) => {
                index += 1;
                count = params_count(*c);
                mode = Some(*c);
            }
            (Some(_), Token::Eod) => unreachable!("loop condition excludes Eod"),
        }

        if index + count < tokens.len() {
            let mut params = Vec::with_capacity(count);
            for t in &tokens[index..index + count] {
                match t {
                    Token::Number(v) => params.push(*v),
                    Token::Command(c) => {
                        return Err(PathError::ParamNotNumber {
                            mode: mode.expect("mode set before params are read"),
                            token: c.to_string(),
                        });
                    }
                    Token::Eod => {
                        return Err(PathError::ParamNotNumber {
                            mode: mode.expect("mode set before params are read"),
                            token: String::new(),
                        });
                    }
                }
            }
            let key = mode.expect("mode set before params are read");
            segments.push(Segment { key, data: params });
            index += count;
            token = &tokens[index];
            if key == 'M' {
                mode = Some('L');
            }
            if key == 'm' {
                mode = Some('l');
            }
        } else {
            return Err(PathError::EndedShort);
        }
    }
    Ok(segments)
}

/// The `i % 2 == 1 ? d + cy : d + cx` map shared by the `c`, `q`, and `s` cases of
/// `lib/absolutize.js` `absolutize`: x/y pairs shifted by the current point.
fn shift_pairs(data: &[f64], dx: f64, dy: f64) -> Vec<f64> {
    data.iter()
        .enumerate()
        .map(|(i, d)| if i % 2 == 1 { d + dy } else { d + dx })
        .collect()
}

/// `lib/absolutize.js` `absolutize`.
pub fn absolutize(segments: &[Segment]) -> Vec<Segment> {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut subx = 0.0;
    let mut suby = 0.0;
    let mut out = Vec::new();
    for Segment { key, data } in segments {
        match key {
            'M' => {
                out.push(Segment {
                    key: 'M',
                    data: data.clone(),
                });
                cx = data[0];
                cy = data[1];
                subx = data[0];
                suby = data[1];
            }
            'm' => {
                cx += data[0];
                cy += data[1];
                out.push(Segment {
                    key: 'M',
                    data: vec![cx, cy],
                });
                subx = cx;
                suby = cy;
            }
            'L' => {
                out.push(Segment {
                    key: 'L',
                    data: data.clone(),
                });
                cx = data[0];
                cy = data[1];
            }
            'l' => {
                cx += data[0];
                cy += data[1];
                out.push(Segment {
                    key: 'L',
                    data: vec![cx, cy],
                });
            }
            'C' => {
                out.push(Segment {
                    key: 'C',
                    data: data.clone(),
                });
                cx = data[4];
                cy = data[5];
            }
            'c' => {
                let newdata = shift_pairs(data, cx, cy);
                cx = newdata[4];
                cy = newdata[5];
                out.push(Segment {
                    key: 'C',
                    data: newdata,
                });
            }
            'Q' => {
                out.push(Segment {
                    key: 'Q',
                    data: data.clone(),
                });
                cx = data[2];
                cy = data[3];
            }
            'q' => {
                let newdata = shift_pairs(data, cx, cy);
                cx = newdata[2];
                cy = newdata[3];
                out.push(Segment {
                    key: 'Q',
                    data: newdata,
                });
            }
            'A' => {
                out.push(Segment {
                    key: 'A',
                    data: data.clone(),
                });
                cx = data[5];
                cy = data[6];
            }
            'a' => {
                cx += data[5];
                cy += data[6];
                out.push(Segment {
                    key: 'A',
                    data: vec![data[0], data[1], data[2], data[3], data[4], cx, cy],
                });
            }
            'H' => {
                out.push(Segment {
                    key: 'H',
                    data: data.clone(),
                });
                cx = data[0];
            }
            'h' => {
                cx += data[0];
                out.push(Segment {
                    key: 'H',
                    data: vec![cx],
                });
            }
            'V' => {
                out.push(Segment {
                    key: 'V',
                    data: data.clone(),
                });
                cy = data[0];
            }
            'v' => {
                cy += data[0];
                out.push(Segment {
                    key: 'V',
                    data: vec![cy],
                });
            }
            'S' => {
                out.push(Segment {
                    key: 'S',
                    data: data.clone(),
                });
                cx = data[2];
                cy = data[3];
            }
            's' => {
                let newdata = shift_pairs(data, cx, cy);
                cx = newdata[2];
                cy = newdata[3];
                out.push(Segment {
                    key: 'S',
                    data: newdata,
                });
            }
            'T' => {
                out.push(Segment {
                    key: 'T',
                    data: data.clone(),
                });
                cx = data[0];
                cy = data[1];
            }
            't' => {
                cx += data[0];
                cy += data[1];
                out.push(Segment {
                    key: 'T',
                    data: vec![cx, cy],
                });
            }
            'Z' | 'z' => {
                out.push(Segment {
                    key: 'Z',
                    data: vec![],
                });
                cx = subx;
                cy = suby;
            }
            _ => {}
        }
    }
    out
}

/// `lib/normalize.js` `degToRad`.
fn deg_to_rad(degrees: f64) -> f64 {
    std::f64::consts::PI * degrees / 180.0
}

/// `lib/normalize.js` `rotate`.
fn rotate(x: f64, y: f64, angle_rad: f64) -> (f64, f64) {
    (
        x * angle_rad.cos() - y * angle_rad.sin(),
        x * angle_rad.sin() + y * angle_rad.cos(),
    )
}

/// `lib/normalize.js` `arcToCubicCurves`, kept to the shape it returns when called with a
/// `recursive` argument: the flat `[m2, m3, m4, ...deeper points]` list, before the
/// caller-only rotate-and-group-by-3 step. Both of the JS function's return branches build
/// this same list; only the top-level (non-recursive) call transforms it further, which
/// [`arc_to_cubic_curves`] does.
#[expect(clippy::too_many_arguments, reason = "mirrors arcToCubicCurves")]
fn arc_points(
    mut x1: f64,
    mut y1: f64,
    mut x2: f64,
    mut y2: f64,
    mut r1: f64,
    mut r2: f64,
    angle: f64,
    large_arc_flag: f64,
    sweep_flag: f64,
    recursive: Option<[f64; 4]>,
) -> Vec<[f64; 2]> {
    let angle_rad = deg_to_rad(angle);
    let mut deeper: Vec<[f64; 2]> = Vec::new();
    let (mut f1, mut f2, cx, cy);
    if let Some([rf1, rf2, rcx, rcy]) = recursive {
        f1 = rf1;
        f2 = rf2;
        cx = rcx;
        cy = rcy;
    } else {
        let (rx1, ry1) = rotate(x1, y1, -angle_rad);
        let (rx2, ry2) = rotate(x2, y2, -angle_rad);
        x1 = rx1;
        y1 = ry1;
        x2 = rx2;
        y2 = ry2;
        let x = (x1 - x2) / 2.0;
        let y = (y1 - y2) / 2.0;
        let mut h = (x * x) / (r1 * r1) + (y * y) / (r2 * r2);
        if h > 1.0 {
            h = h.sqrt();
            r1 *= h;
            r2 *= h;
        }
        let sign = if large_arc_flag == sweep_flag {
            -1.0
        } else {
            1.0
        };
        let r1_pow = r1 * r1;
        let r2_pow = r2 * r2;
        let left = r1_pow * r2_pow - r1_pow * y * y - r2_pow * x * x;
        let right = r1_pow * y * y + r2_pow * x * x;
        let k = sign * (left / right).abs().sqrt();
        let ccx = k * r1 * y / r2 + (x1 + x2) / 2.0;
        let ccy = k * -r2 * x / r1 + (y1 + y2) / 2.0;
        cx = ccx;
        cy = ccy;
        f1 = js::to_fixed((y1 - cy) / r2, 9)
            .parse::<f64>()
            .expect("to_fixed always produces a valid float literal")
            .asin();
        f2 = js::to_fixed((y2 - cy) / r2, 9)
            .parse::<f64>()
            .expect("to_fixed always produces a valid float literal")
            .asin();
        if x1 < cx {
            f1 = std::f64::consts::PI - f1;
        }
        if x2 < cx {
            f2 = std::f64::consts::PI - f2;
        }
        if f1 < 0.0 {
            f1 += std::f64::consts::PI * 2.0;
        }
        if f2 < 0.0 {
            f2 += std::f64::consts::PI * 2.0;
        }
        if truthy(sweep_flag) && f1 > f2 {
            f1 -= std::f64::consts::PI * 2.0;
        }
        if !truthy(sweep_flag) && f2 > f1 {
            f2 -= std::f64::consts::PI * 2.0;
        }
    }

    let mut df = f2 - f1;
    if df.abs() > (std::f64::consts::PI * 120.0 / 180.0) {
        let f2old = f2;
        let x2old = x2;
        let y2old = y2;
        if truthy(sweep_flag) && f2 > f1 {
            f2 = f1 + (std::f64::consts::PI * 120.0 / 180.0);
        } else {
            f2 = f1 - (std::f64::consts::PI * 120.0 / 180.0);
        }
        x2 = cx + r1 * f2.cos();
        y2 = cy + r2 * f2.sin();
        deeper = arc_points(
            x2,
            y2,
            x2old,
            y2old,
            r1,
            r2,
            angle,
            0.0,
            sweep_flag,
            Some([f2, f2old, cx, cy]),
        );
    }
    df = f2 - f1;
    let c1 = f1.cos();
    let s1 = f1.sin();
    let c2 = f2.cos();
    let s2 = f2.sin();
    let t = (df / 4.0).tan();
    let hx = 4.0 / 3.0 * r1 * t;
    let hy = 4.0 / 3.0 * r2 * t;
    let m1 = [x1, y1];
    let mut m2 = [x1 + hx * s1, y1 - hy * c1];
    let m3 = [x2 + hx * s2, y2 - hy * c2];
    let m4 = [x2, y2];
    m2[0] = 2.0 * m1[0] - m2[0];
    m2[1] = 2.0 * m1[1] - m2[1];

    let mut points = vec![m2, m3, m4];
    points.extend(deeper);
    points
}

/// `lib/normalize.js` `arcToCubicCurves`, top-level (non-recursive) call: rotates
/// [`arc_points`]'s flat point list back into world space and groups it into cubic
/// Bezier control-point triples.
#[expect(clippy::too_many_arguments, reason = "mirrors arcToCubicCurves")]
fn arc_to_cubic_curves(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    r1: f64,
    r2: f64,
    angle: f64,
    large_arc_flag: f64,
    sweep_flag: f64,
) -> Vec<[f64; 6]> {
    let angle_rad = deg_to_rad(angle);
    let points = arc_points(
        x1,
        y1,
        x2,
        y2,
        r1,
        r2,
        angle,
        large_arc_flag,
        sweep_flag,
        None,
    );
    points
        .chunks(3)
        .map(|chunk| {
            let (x1, y1) = rotate(chunk[0][0], chunk[0][1], angle_rad);
            let (x2, y2) = rotate(chunk[1][0], chunk[1][1], angle_rad);
            let (x3, y3) = rotate(chunk[2][0], chunk[2][1], angle_rad);
            [x1, y1, x2, y2, x3, y3]
        })
        .collect()
}

/// `lib/normalize.js` `normalize`.
pub fn normalize(segments: &[Segment]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut last_type: Option<char> = None; // JS: ''
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut subx = 0.0;
    let mut suby = 0.0;
    let mut lcx = 0.0;
    let mut lcy = 0.0;
    for Segment { key, data } in segments {
        match key {
            'M' => {
                out.push(Segment {
                    key: 'M',
                    data: data.clone(),
                });
                cx = data[0];
                cy = data[1];
                subx = data[0];
                suby = data[1];
            }
            'C' => {
                out.push(Segment {
                    key: 'C',
                    data: data.clone(),
                });
                cx = data[4];
                cy = data[5];
                lcx = data[2];
                lcy = data[3];
            }
            'L' => {
                out.push(Segment {
                    key: 'L',
                    data: data.clone(),
                });
                cx = data[0];
                cy = data[1];
            }
            'H' => {
                cx = data[0];
                out.push(Segment {
                    key: 'L',
                    data: vec![cx, cy],
                });
            }
            'V' => {
                cy = data[0];
                out.push(Segment {
                    key: 'L',
                    data: vec![cx, cy],
                });
            }
            'S' => {
                let (cx1, cy1) = if last_type == Some('C') || last_type == Some('S') {
                    (cx + (cx - lcx), cy + (cy - lcy))
                } else {
                    (cx, cy)
                };
                let mut newdata = vec![cx1, cy1];
                newdata.extend_from_slice(data);
                out.push(Segment {
                    key: 'C',
                    data: newdata,
                });
                lcx = data[0];
                lcy = data[1];
                cx = data[2];
                cy = data[3];
            }
            'T' => {
                let x = data[0];
                let y = data[1];
                let (x1, y1) = if last_type == Some('Q') || last_type == Some('T') {
                    (cx + (cx - lcx), cy + (cy - lcy))
                } else {
                    (cx, cy)
                };
                let cx1 = cx + 2.0 * (x1 - cx) / 3.0;
                let cy1 = cy + 2.0 * (y1 - cy) / 3.0;
                let cx2 = x + 2.0 * (x1 - x) / 3.0;
                let cy2 = y + 2.0 * (y1 - y) / 3.0;
                out.push(Segment {
                    key: 'C',
                    data: vec![cx1, cy1, cx2, cy2, x, y],
                });
                lcx = x1;
                lcy = y1;
                cx = x;
                cy = y;
            }
            'Q' => {
                let x1 = data[0];
                let y1 = data[1];
                let x = data[2];
                let y = data[3];
                let cx1 = cx + 2.0 * (x1 - cx) / 3.0;
                let cy1 = cy + 2.0 * (y1 - cy) / 3.0;
                let cx2 = x + 2.0 * (x1 - x) / 3.0;
                let cy2 = y + 2.0 * (y1 - y) / 3.0;
                out.push(Segment {
                    key: 'C',
                    data: vec![cx1, cy1, cx2, cy2, x, y],
                });
                lcx = x1;
                lcy = y1;
                cx = x;
                cy = y;
            }
            'A' => {
                let r1 = data[0].abs();
                let r2 = data[1].abs();
                let angle = data[2];
                let large_arc_flag = data[3];
                let sweep_flag = data[4];
                let x = data[5];
                let y = data[6];
                if r1 == 0.0 || r2 == 0.0 {
                    out.push(Segment {
                        key: 'C',
                        data: vec![cx, cy, x, y, x, y],
                    });
                    cx = x;
                    cy = y;
                } else if cx != x || cy != y {
                    let curves = arc_to_cubic_curves(
                        cx,
                        cy,
                        x,
                        y,
                        r1,
                        r2,
                        angle,
                        large_arc_flag,
                        sweep_flag,
                    );
                    for curve in curves {
                        out.push(Segment {
                            key: 'C',
                            data: curve.to_vec(),
                        });
                    }
                    cx = x;
                    cy = y;
                }
            }
            'Z' | 'z' => {
                out.push(Segment {
                    key: 'Z',
                    data: vec![],
                });
                cx = subx;
                cy = suby;
            }
            _ => {}
        }
        last_type = Some(*key);
    }
    out
}
