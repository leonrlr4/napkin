//! Text layout on top of Excalidraw's own metrics: which bundled font a `fontFamily` id maps
//! to, the per-line baseline `renderElement.ts`'s text branch computes, and shaping that layout
//! into `glyphon`/`cosmic-text` buffers `gpu.rs` turns into `TextArea`s.

use glyphon::cosmic_text::Align;

/// `head.unitsPerEm`, `hhea.ascender`, `hhea.descender` for one font, as Excalidraw's
/// `FONT_METADATA` records them (`packages/common/src/font-metadata.ts` at the pinned commit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: f64,
    pub ascender: f64,
    pub descender: f64,
}

/// spec §6.3: 5, 1 -> napkin-hand; 6, 2, 7, 9, 10 -> napkin-sans; 8, 3 -> napkin-code; anything
/// else (including an unknown id) -> napkin-hand.
pub fn bundled_family(font_family: f64) -> &'static str {
    match font_family as i64 {
        5 | 1 => "napkin-hand",
        6 | 2 | 7 | 9 | 10 => "napkin-sans",
        8 | 3 => "napkin-code",
        _ => "napkin-hand",
    }
}

/// `FONT_METADATA[fontFamily].metrics`, falling back to Excalifont's metrics (id 5) for an id
/// `FONT_METADATA` has no entry for, exactly like `getVerticalOffset` does.
pub fn excalidraw_metrics(font_family: f64) -> FontMetrics {
    match font_family as i64 {
        // Virgil (deprecated, but still a valid `fontFamily` on an old file).
        1 => FontMetrics {
            units_per_em: 1000.0,
            ascender: 886.0,
            descender: -374.0,
        },
        // Helvetica (deprecated, local-only).
        2 => FontMetrics {
            units_per_em: 2048.0,
            ascender: 1577.0,
            descender: -471.0,
        },
        // Cascadia (deprecated).
        3 => FontMetrics {
            units_per_em: 2048.0,
            ascender: 1900.0,
            descender: -480.0,
        },
        // Nunito.
        6 => FontMetrics {
            units_per_em: 1000.0,
            ascender: 1011.0,
            descender: -353.0,
        },
        // Lilita One.
        7 => FontMetrics {
            units_per_em: 1000.0,
            ascender: 923.0,
            descender: -220.0,
        },
        // Comic Shanns.
        8 => FontMetrics {
            units_per_em: 1000.0,
            ascender: 750.0,
            descender: -250.0,
        },
        // Liberation Sans (private, Helvetica's local-font substitute).
        9 => FontMetrics {
            units_per_em: 2048.0,
            ascender: 1854.0,
            descender: -434.0,
        },
        // Assistant (private).
        10 => FontMetrics {
            units_per_em: 2048.0,
            ascender: 1021.0,
            descender: -287.0,
        },
        // Excalifont (5), and the fallback for any other id.
        _ => FontMetrics {
            units_per_em: 1000.0,
            ascender: 886.0,
            descender: -374.0,
        },
    }
}

/// A text element's `lineHeight` when the file omits it (an old Excalidraw file may lack the
/// field): `getLineHeight`'s own fallback, Excalifont's hardcoded line height.
const DEFAULT_LINE_HEIGHT: f64 = 1.25;

/// `getVerticalOffset` (`packages/common/src/font-metadata.ts`): the baseline offset, in the
/// same units as `font_size`, from a line's top to its alphabetic baseline.
fn vertical_offset(font_family: f64, font_size: f64, line_height_px: f64) -> f64 {
    let metrics = excalidraw_metrics(font_family);
    let font_size_em = font_size / metrics.units_per_em;
    let line_gap =
        (line_height_px - font_size_em * metrics.ascender + font_size_em * metrics.descender) / 2.0;
    font_size_em * metrics.ascender + line_gap
}

/// One line of a text element's local layout: its text and the local y (relative to the
/// element's own origin) of its alphabetic baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct LineLayout {
    pub text: String,
    pub baseline: f64,
}

/// Splits `text` the way `renderElement.ts`'s text branch does: `\r\n`, lone `\r` and lone `\n`
/// all count as one line break (its regex `/\r\n?/g` normalizes every `\r`-led ending to `\n`
/// before splitting on `\n`).
fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines.push(&text[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
            }
            b'\n' => {
                lines.push(&text[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    lines.push(&text[start..]);
    lines
}

/// `renderElement.ts`'s text branch: lines split on `\r\n`, `\r` and `\n`; baseline of line i is
/// `i * fontSize * lineHeight + getVerticalOffset(...)`.
pub fn layout_lines(text: &scene::element::TextElement) -> Vec<LineLayout> {
    let line_height = text.line_height.unwrap_or(DEFAULT_LINE_HEIGHT);
    let line_height_px = text.font_size * line_height;
    let offset = vertical_offset(text.font_family, text.font_size, line_height_px);
    split_lines(&text.text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| LineLayout {
            text: line.to_owned(),
            baseline: (index as f64) * line_height_px + offset,
        })
        .collect()
}

/// Placeholder type label (spec §1.2): napkin-sans 12, baseline 16, left 4, color `#868e96`.
/// The left offset and color are applied by whoever positions this line (`render/gpu.rs`), not
/// carried on `LineLayout` itself.
pub fn placeholder_label(kind: &str) -> LineLayout {
    LineLayout {
        text: kind.to_owned(),
        baseline: 16.0,
    }
}

/// A `FontSystem` with system fonts (for CJK fallback) and the three bundled fonts loaded.
pub fn font_system() -> glyphon::FontSystem {
    let mut font_system = glyphon::FontSystem::new();
    let db = font_system.db_mut();
    db.load_font_data(include_bytes!("../../../../assets/fonts/napkin-hand.ttf").to_vec());
    db.load_font_data(include_bytes!("../../../../assets/fonts/napkin-sans.ttf").to_vec());
    db.load_font_data(include_bytes!("../../../../assets/fonts/napkin-code.ttf").to_vec());
    font_system
}

/// One shaped line, ready to become a `glyphon::TextArea` once its screen position is known.
#[derive(Debug)]
pub struct ShapedLine {
    pub buffer: glyphon::Buffer,
    /// `line_y` of the buffer's first (only) layout run, in local buffer units: cosmic-text's
    /// own idea of where this line's baseline sits within the buffer.
    pub baseline_in_buffer: f32,
    /// The same line's baseline from `layout_lines`/`placeholder_label` (Excalidraw's own
    /// vertical offset), copied through so the caller can align the two without looking the
    /// line back up.
    pub baseline: f64,
}

/// Shapes `lines` as one `glyphon::Buffer` per line (so each line's own `line_y` can be
/// compared against its `LineLayout::baseline`; seeing them line up is what confirms the port
/// against Excalidraw, spec §6.3). `family` selects a bundled font by name; system CJK fallback
/// still applies within it.
pub fn shape_lines(
    font_system: &mut glyphon::FontSystem,
    lines: &[LineLayout],
    family: &str,
    font_size: f32,
    line_height_px: f32,
    width: f32,
    align: Option<Align>,
) -> Vec<ShapedLine> {
    let attrs = glyphon::Attrs::new().family(glyphon::Family::Name(family));
    lines
        .iter()
        .map(|line| {
            let mut buffer = glyphon::Buffer::new(
                font_system,
                glyphon::Metrics::new(font_size, line_height_px),
            );
            buffer.set_wrap(glyphon::Wrap::None);
            buffer.set_size(Some(width), None);
            buffer.set_text(&line.text, &attrs, glyphon::Shaping::Advanced, align);
            buffer.shape_until_scroll(font_system, false);
            let baseline_in_buffer = buffer
                .layout_runs()
                .next()
                .map(|run| run.line_y)
                .unwrap_or(0.0);
            ShapedLine {
                buffer,
                baseline_in_buffer,
                baseline: line.baseline,
            }
        })
        .collect()
}

/// `render/gpu.rs`'s `glyphon::TextArea::top` for one line: chosen so that, however cosmic-text
/// places this particular font's `baseline_in_buffer` (which does not always match Excalidraw's
/// own `getVerticalOffset`; see `shaped_baselines_match_excalidraw_for_all_bundled_fonts`), the
/// line's baseline renders at physical pixel `origin_px + baseline * scale`: glyphon draws a run
/// at `top + run.line_y * scale`, and `baseline_in_buffer` is exactly that `line_y`, so
/// subtracting it here and re-adding it at render time cancels out.
pub fn text_area_top(origin_px: f32, baseline: f64, baseline_in_buffer: f32, scale: f32) -> f32 {
    origin_px + baseline as f32 * scale - baseline_in_buffer * scale
}

#[cfg(test)]
mod tests {
    use scene::Element;
    use serde_json::json;

    use super::*;
    use scene::sample;

    fn text_element(overrides: serde_json::Value) -> scene::element::TextElement {
        match Element::from_value(sample::with(
            sample::text("t", [0.0, 0.0, 100.0, 50.0], "a", None),
            overrides,
        )) {
            Element::Text(text) => text,
            other => panic!("not text: {other:?}"),
        }
    }

    #[test]
    fn families_and_metrics() {
        assert_eq!(bundled_family(5.0), "napkin-hand");
        assert_eq!(bundled_family(2.0), "napkin-sans");
        assert_eq!(bundled_family(3.0), "napkin-code");
        assert_eq!(bundled_family(42.0), "napkin-hand");
        assert_eq!(
            excalidraw_metrics(5.0),
            FontMetrics {
                units_per_em: 1000.0,
                ascender: 886.0,
                descender: -374.0
            }
        );
        assert_eq!(excalidraw_metrics(42.0), excalidraw_metrics(5.0));
    }

    #[test]
    fn lines_follow_get_vertical_offset() {
        let text = text_element(
            json!({ "text": "one\r\ntwo\rthree", "fontSize": 20, "lineHeight": 1.25 }),
        );
        let lines = layout_lines(&text);
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["one", "two", "three"]);
        assert!((lines[0].baseline - 17.62).abs() < 1e-9);
        assert!((lines[2].baseline - (50.0 + 17.62)).abs() < 1e-9);
    }

    #[test]
    fn shaped_baselines_match_excalidraw_for_all_bundled_fonts() {
        // cosmic-text's own `line_y` matches `getVerticalOffset` for napkin-hand and
        // napkin-code (both read hhea ascender/descender), but not for napkin-sans: that font
        // sets the OS/2 "prefer typo metrics" flag, yet cosmic-text still measures it off
        // usWinAscent/usWinDescent (1077/300) instead of the sTypoAscender/Descender
        // (1011/-353) that matches Excalidraw's own table, so its `line_y` for a 20px line is
        // 20.26 instead of 19.08. That divergence is harmless: `render/gpu.rs` never trusts
        // `line_y` as an absolute position, only as the anchor `text_area_top` resolves against,
        // so the rendered baseline always lands at `origin + baseline * scale` regardless of
        // which metrics cosmic-text used internally.
        //
        // This checks two independent facts instead of just asserting `text_area_top`'s own
        // algebra (which would hold for any `baseline_in_buffer`, wrong or not):
        // (a) `shape_lines` records the same `baseline_in_buffer` a fresh, independently shaped
        //     `Buffer` (not going through `shape_lines`) measures as its `line_y` -- so a bug
        //     that read the wrong run, or a stale/default value, would be caught here even
        //     though it wouldn't be caught by (b) alone.
        // (b) feeding that independently measured `line_y` through `text_area_top` places the
        //     baseline at `origin + baseline * scale`, at two different (origin, scale) pairs.
        let mut font_system = font_system();
        for font_family in [5.0, 6.0, 8.0] {
            let text = text_element(json!({
                "text": "Hamburg", "fontFamily": font_family, "fontSize": 20, "lineHeight": 1.25
            }));
            let lines = layout_lines(&text);
            let family = bundled_family(font_family);
            let shaped = shape_lines(&mut font_system, &lines, family, 20.0, 25.0, 100.0, None);

            let attrs = glyphon::Attrs::new().family(glyphon::Family::Name(family));
            let mut independent =
                glyphon::Buffer::new(&mut font_system, glyphon::Metrics::new(20.0, 25.0));
            independent.set_wrap(glyphon::Wrap::None);
            independent.set_size(Some(100.0), None);
            independent.set_text(&lines[0].text, &attrs, glyphon::Shaping::Advanced, None);
            independent.shape_until_scroll(&mut font_system, false);
            let independent_line_y = independent.layout_runs().next().expect("one line").line_y;

            // (a)
            assert!(
                (shaped[0].baseline_in_buffer - independent_line_y).abs() < 1e-4,
                "family {font_family}: shape_lines recorded {} but an independently shaped \
                 Buffer measures {independent_line_y}",
                shaped[0].baseline_in_buffer,
            );

            // (b)
            for (origin, scale) in [(0.0f32, 1.0f32), (100.0, 2.0)] {
                let top = text_area_top(origin, lines[0].baseline, independent_line_y, scale);
                let rendered_baseline = top + independent_line_y * scale;
                let expected = origin + lines[0].baseline as f32 * scale;
                assert!(
                    (rendered_baseline - expected).abs() < 1e-3,
                    "family {font_family}, origin {origin}, scale {scale}: {rendered_baseline} \
                     vs {expected}"
                );
            }
        }
    }
}
