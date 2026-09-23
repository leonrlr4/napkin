//! The omarchy theme file (spec §7.6): `mode` plus five `#rrggbb` colors, translated into
//! an [`egui::Visuals`].

use eframe::egui;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: egui::Color32,
    pub foreground: egui::Color32,
    pub accent: egui::Color32,
    pub selection: egui::Color32,
    pub muted: egui::Color32,
}

impl Theme {
    /// The dark palette used when the omarchy theme file is missing or invalid (spec §7.6).
    pub fn builtin() -> Theme {
        Theme {
            dark: true,
            background: egui::Color32::from_rgb(0x1a, 0x1b, 0x26),
            foreground: egui::Color32::from_rgb(0xc0, 0xca, 0xf5),
            accent: egui::Color32::from_rgb(0x7a, 0xa2, 0xf7),
            selection: egui::Color32::from_rgb(0x33, 0x46, 0x7c),
            muted: egui::Color32::from_rgb(0x56, 0x5f, 0x89),
        }
    }

    /// `mode` ("dark" or "light") plus the five colors as "#rrggbb" strings.
    pub fn parse(text: &str) -> Result<Theme, String> {
        let table: toml::Table = text
            .parse()
            .map_err(|error| format!("invalid TOML: {error}"))?;
        let mode = table
            .get("mode")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "missing key: mode".to_string())?;
        let dark = match mode {
            "dark" => true,
            "light" => false,
            other => {
                return Err(format!(
                    "mode: invalid value {other:?} (expected \"dark\" or \"light\")"
                ));
            }
        };
        Ok(Theme {
            dark,
            background: color(&table, "background")?,
            foreground: color(&table, "foreground")?,
            accent: color(&table, "accent")?,
            selection: color(&table, "selection")?,
            muted: color(&table, "muted")?,
        })
    }

    pub fn load(path: &std::path::Path) -> Result<Theme, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Theme::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn visuals(&self) -> egui::Visuals {
        let mut visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = self.background;
        visuals.window_fill = self.background;
        visuals.extreme_bg_color = self.background;
        visuals.override_text_color = Some(self.foreground);
        visuals.selection.bg_fill = self.selection;
        visuals.hyperlink_color = self.accent;
        visuals.widgets.noninteractive.fg_stroke.color = self.muted;
        visuals
    }
}

/// Reads `table[key]` as a `#rrggbb` string. The error names `key` either way, so a bad
/// theme file points straight at the offending line.
fn color(table: &toml::Table, key: &str) -> Result<egui::Color32, String> {
    let value = table
        .get(key)
        .ok_or_else(|| format!("missing key: {key}"))?;
    let text = value
        .as_str()
        .ok_or_else(|| format!("{key}: not a string"))?;
    parse_hex_color(text).ok_or_else(|| format!("{key}: invalid color {text:?} (expected #rrggbb)"))
}

fn parse_hex_color(text: &str) -> Option<egui::Color32> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(egui::Color32::from_rgb(r, g, b))
}

/// `$HOME/.local/state/omarchy/current/theme/colors.toml`.
pub fn omarchy_theme_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join(".local/state/omarchy/current/theme/colors.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OZARK: &str = r##"
mode = "dark"
accent    = "#ef9268"
selection = "#3a3560"
muted     = "#9990b0"
background = "#171525"
foreground = "#e2dee8"
red = "#e8828a"
"##;

    #[test]
    fn parses_an_omarchy_theme_ignoring_extra_keys() {
        let theme = Theme::parse(OZARK).expect("parses");
        assert!(theme.dark);
        assert_eq!(theme.background, egui::Color32::from_rgb(0x17, 0x15, 0x25));
        assert_eq!(theme.accent, egui::Color32::from_rgb(0xef, 0x92, 0x68));
    }

    #[test]
    fn light_mode_and_errors() {
        let light = OZARK.replace("\"dark\"", "\"light\"");
        assert!(!Theme::parse(&light).expect("parses").dark);
        let missing = OZARK.replace("accent", "accentx");
        assert!(Theme::parse(&missing).unwrap_err().contains("accent"));
        let bad = OZARK.replace("#ef9268", "orange-ish");
        assert!(Theme::parse(&bad).is_err());
        assert!(Theme::parse("mode = \"sepia\"").is_err());
    }
}
