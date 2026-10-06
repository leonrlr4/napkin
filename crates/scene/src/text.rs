//! Text measurement as Excalidraw does it, ported from `packages/element/src/textMeasurements.ts`
//! (`measureText`, `getTextWidth`, `getTextHeight`, `normalizeText`),
//! `packages/common/src/utils.ts`'s `normalizeEOL` and `packages/common/src/font-metadata.ts`'s
//! `getLineHeight` at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`. Glyph widths come from
//! the caller's [`TextMeasure`], Excalidraw's `TextMetricsProvider`: `scene` has no fonts.

/// `DEFAULT_FONT_FAMILY` (`FONT_FAMILY.Excalifont`).
pub const DEFAULT_FONT_FAMILY: f64 = 5.0;
/// `DEFAULT_FONT_SIZE`.
pub const DEFAULT_FONT_SIZE: f64 = 20.0;

pub trait TextMeasure {
    /// `TextMetricsProvider.getLineWidth`: the width of `line`, which holds no line break, in
    /// scene units at `font_size`.
    fn line_width(&mut self, line: &str, font_family: f64, font_size: f64) -> f64;
}

/// `getLineHeight`: `FONT_METADATA[fontFamily].metrics.lineHeight`, Excalifont's for an id
/// `FONT_METADATA` has no entry for.
pub fn line_height(font_family: f64) -> f64 {
    if font_family == 2.0 || font_family == 7.0 || font_family == 9.0 {
        1.15
    } else if font_family == 3.0 {
        1.2
    } else {
        1.25
    }
}

/// `normalizeText`: `\r\n` and lone `\r` become `\n` (`normalizeEOL`), tabs become 8 spaces.
pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "        ")
}

/// `measureText` for already-normalized `text`: `[width, height]`, where width is the widest
/// line and height is `lines * fontSize * lineHeight`. An empty line measures as `" "`, as in
/// `measureText`.
pub fn measure_text(
    text: &str,
    font_family: f64,
    font_size: f64,
    line_height: f64,
    measure: &mut dyn TextMeasure,
) -> [f64; 2] {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| if line.is_empty() { " " } else { line })
        .collect();
    let height = font_size * line_height * lines.len() as f64;
    let width = lines
        .iter()
        .map(|line| measure.line_width(line, font_family, font_size))
        .fold(0.0, f64::max);
    [width, height]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::CharWidthMeasure;

    #[test]
    fn line_heights_follow_font_metadata() {
        assert_eq!(line_height(5.0), 1.25);
        assert_eq!(line_height(6.0), 1.25);
        assert_eq!(line_height(7.0), 1.15);
        assert_eq!(line_height(8.0), 1.25);
        assert_eq!(line_height(2.0), 1.15);
        assert_eq!(line_height(3.0), 1.2);
        assert_eq!(line_height(9.0), 1.15);
        assert_eq!(
            line_height(42.0),
            1.25,
            "unknown ids fall back to Excalifont"
        );
    }

    #[test]
    fn normalizes_line_endings_and_tabs() {
        assert_eq!(normalize_text("a\r\nb\rc\nd\te"), "a\nb\nc\nd        e");
    }

    #[test]
    fn measures_the_widest_line_and_counts_empty_lines() {
        let mut measure = CharWidthMeasure;
        // "abc" is 3 * 0.6 * 20 = 36 wide; the empty middle line counts as one line (" ").
        assert_eq!(
            measure_text("ab\n\nabc", 5.0, 20.0, 1.25, &mut measure),
            [36.0, 75.0]
        );
        assert_eq!(measure_text("", 5.0, 10.0, 1.2, &mut measure), [6.0, 12.0]);
    }
}
