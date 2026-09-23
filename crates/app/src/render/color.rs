//! CSS color parsing for the renderer, on top of `scene`'s tinycolor port.

use scene::color::{apply_dark_mode_filter, tinycolor};

/// Straight-alpha RGBA with gamma-encoded channels, which is what browser canvases blend.
pub type Rgba = [f32; 4];

/// `tinycolor(value)`, `r`/`g`/`b` scaled from `0..255` to `0..1` and clamped, `a` unchanged.
/// `None` if `value` does not parse.
pub fn css_color(value: &str) -> Option<Rgba> {
    let color = tinycolor(value);
    if !color.ok {
        return None;
    }
    Some([
        (color.r / 255.0).clamp(0.0, 1.0) as f32,
        (color.g / 255.0).clamp(0.0, 1.0) as f32,
        (color.b / 255.0).clamp(0.0, 1.0) as f32,
        color.a as f32,
    ])
}

/// `css_color` after the dark-mode filter when `dark`; canvas keeps its default black for
/// a string it cannot parse.
pub fn render_color(value: &str, dark: bool) -> Rgba {
    let filtered;
    let value = if dark {
        filtered = apply_dark_mode_filter(value);
        filtered.as_str()
    } else {
        value
    };
    css_color(value).unwrap_or([0.0, 0.0, 0.0, 1.0])
}

#[cfg(test)]
mod tests {
    use scene::color::apply_dark_mode_filter;

    use super::*;

    #[test]
    fn parses_css_colors() {
        assert_eq!(css_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            css_color("rgba(255, 0, 0, 0.5)"),
            Some([1.0, 0.0, 0.0, 0.5])
        );
        assert_eq!(css_color("transparent").map(|c| c[3]), Some(0.0));
        assert_eq!(css_color("not a color"), None);
    }

    #[test]
    fn render_color_applies_dark_mode_and_defaults_to_black() {
        assert_eq!(render_color("not a color", false), [0.0, 0.0, 0.0, 1.0]);
        let expected = css_color(&apply_dark_mode_filter("#1e1e1e")).expect("filter output parses");
        assert_eq!(render_color("#1e1e1e", true), expected);
    }
}
