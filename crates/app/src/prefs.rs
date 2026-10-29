//! The tool, padlock and new-element style that outlive the window, the part of Excalidraw's
//! `APP_STATE_STORAGE_CONF` marked `browser: true` that napkin has: `activeTool` (with its
//! `locked` flag) and the `currentItem*` values. Excalidraw keeps them in `localStorage`, not
//! in the `.excalidraw` file, so they follow the person across canvases; napkin keeps them in
//! one JSON file for the same reason. Which parts of the style panel are open rides along in
//! the same file.
//!
//! Reading is lenient: a missing file, unparseable JSON or a field of the wrong type falls back
//! to [`Prefs::default`] for that field alone, so a hand edit or an older napkin's file never
//! stops napkin from starting.

use std::path::Path;

use scene::editor::{ArrowType, EdgeStyle, Editor, ItemStyle, StrokeWidth, Tool};
use scene::env::Env;
use serde_json::{Map, Value, json};

use crate::properties_panel::PanelLayout;
use crate::storage;

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub tool: Tool,
    pub tool_locked: bool,
    pub style: ItemStyle,
    pub panel: PanelLayout,
}

impl Default for Prefs {
    /// The padlock starts locked, unlike Excalidraw: drawing several arrows or lines in a row
    /// without reselecting the tool each time is what napkin is used for.
    fn default() -> Self {
        Prefs {
            tool: Tool::Selection,
            tool_locked: true,
            style: ItemStyle::default(),
            panel: PanelLayout::default(),
        }
    }
}

const TOOLS: [(Tool, &str); 10] = [
    (Tool::Selection, "selection"),
    (Tool::Hand, "hand"),
    (Tool::Rectangle, "rectangle"),
    (Tool::Diamond, "diamond"),
    (Tool::Ellipse, "ellipse"),
    (Tool::Arrow, "arrow"),
    (Tool::Line, "line"),
    (Tool::Freedraw, "freedraw"),
    (Tool::Text, "text"),
    (Tool::Eraser, "eraser"),
];

const STROKE_WIDTHS: [(StrokeWidth, &str); 4] = [
    (StrokeWidth::Thin, "thin"),
    (StrokeWidth::Medium, "medium"),
    (StrokeWidth::Bold, "bold"),
    (StrokeWidth::ExtraBold, "extraBold"),
];

fn tool_name(tool: Tool) -> &'static str {
    TOOLS
        .iter()
        .find(|(t, _)| *t == tool)
        .map(|(_, name)| *name)
        .expect("TOOLS lists every tool")
}

fn sharp_or_round(sharp: bool) -> &'static str {
    if sharp { "sharp" } else { "round" }
}

impl Prefs {
    /// What `editor` and the panel would start with if napkin restarted now.
    pub fn of<E: Env>(editor: &Editor<E>, panel: PanelLayout) -> Prefs {
        Prefs {
            tool: editor.tool(),
            tool_locked: editor.tool_locked(),
            style: editor.style().clone(),
            panel,
        }
    }

    pub fn apply_to<E: Env>(&self, editor: &mut Editor<E>) {
        editor.set_tool(self.tool);
        editor.set_tool_locked(self.tool_locked);
        editor.set_style(self.style.clone());
    }

    pub fn to_json(&self) -> Value {
        let s = &self.style;
        let stroke_width = STROKE_WIDTHS
            .iter()
            .find(|(w, _)| *w == s.stroke_width)
            .map(|(_, name)| *name)
            .expect("STROKE_WIDTHS lists every width");
        json!({
            "activeTool": tool_name(self.tool),
            "toolLocked": self.tool_locked,
            "panelCollapsed": self.panel.collapsed,
            "strokePaletteOpen": self.panel.stroke_palette,
            "backgroundPaletteOpen": self.panel.background_palette,
            "currentItemStrokeColor": s.stroke_color,
            "currentItemBackgroundColor": s.background_color,
            "currentItemFillStyle": s.fill_style,
            "currentItemStrokeWidth": stroke_width,
            "currentItemStrokeStyle": s.stroke_style,
            "currentItemRoughness": s.roughness,
            "currentItemOpacity": s.opacity,
            "currentItemRoundness": sharp_or_round(s.edges == EdgeStyle::Sharp),
            "currentItemArrowType": sharp_or_round(s.arrow_type == ArrowType::Sharp),
            "currentItemStartArrowhead": s.start_arrowhead,
            "currentItemEndArrowhead": s.end_arrowhead,
            "currentItemStrokeVariability": s.stroke_variability,
            "currentItemFontFamily": s.font_family,
            "currentItemFontSize": s.font_size,
        })
    }

    pub fn from_json(value: &Value) -> Prefs {
        let mut prefs = Prefs::default();
        let Some(map) = value.as_object() else {
            return prefs;
        };
        let string = |key: &str| map.get(key).and_then(Value::as_str);
        let number = |key: &str| {
            map.get(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };

        if let Some(tool) = string("activeTool")
            .and_then(|name| TOOLS.iter().find(|(_, n)| *n == name))
            .map(|(tool, _)| *tool)
        {
            prefs.tool = tool;
        }
        let flag = |field: &mut bool, key: &str| {
            if let Some(value) = map.get(key).and_then(Value::as_bool) {
                *field = value;
            }
        };
        flag(&mut prefs.tool_locked, "toolLocked");
        flag(&mut prefs.panel.collapsed, "panelCollapsed");
        flag(&mut prefs.panel.stroke_palette, "strokePaletteOpen");
        flag(&mut prefs.panel.background_palette, "backgroundPaletteOpen");

        let s = &mut prefs.style;
        set_string(&mut s.stroke_color, string("currentItemStrokeColor"));
        set_string(
            &mut s.background_color,
            string("currentItemBackgroundColor"),
        );
        set_string(&mut s.fill_style, string("currentItemFillStyle"));
        set_string(&mut s.stroke_style, string("currentItemStrokeStyle"));
        set_string(
            &mut s.stroke_variability,
            string("currentItemStrokeVariability"),
        );
        if let Some(width) = string("currentItemStrokeWidth")
            .and_then(|name| STROKE_WIDTHS.iter().find(|(_, n)| *n == name))
            .map(|(width, _)| *width)
        {
            s.stroke_width = width;
        }
        if let Some(v) = number("currentItemRoughness") {
            s.roughness = v;
        }
        if let Some(v) = number("currentItemOpacity").filter(|v| (0.0..=100.0).contains(v)) {
            s.opacity = v;
        }
        if let Some(v) = number("currentItemFontFamily") {
            s.font_family = v;
        }
        if let Some(v) = number("currentItemFontSize").filter(|v| *v > 0.0) {
            s.font_size = v;
        }
        match string("currentItemRoundness") {
            Some("sharp") => s.edges = EdgeStyle::Sharp,
            Some("round") => s.edges = EdgeStyle::Round,
            _ => {}
        }
        match string("currentItemArrowType") {
            Some("sharp") => s.arrow_type = ArrowType::Sharp,
            Some("round") => s.arrow_type = ArrowType::Round,
            _ => {}
        }
        arrowhead(&mut s.start_arrowhead, map, "currentItemStartArrowhead");
        arrowhead(&mut s.end_arrowhead, map, "currentItemEndArrowhead");
        prefs
    }

    /// [`Prefs::default`] when `path` is missing or unreadable.
    pub fn load(path: &Path) -> Prefs {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .map_or_else(Prefs::default, |value| Prefs::from_json(&value))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&self.to_json()).expect("prefs always serialize");
        storage::write_atomic(path, &text).map(|_| ())
    }
}

fn set_string(field: &mut String, value: Option<&str>) {
    if let Some(value) = value {
        *field = value.to_owned();
    }
}

/// A string is that arrowhead, `null` is none; anything else (or a missing key) keeps the
/// default.
fn arrowhead(field: &mut Option<String>, map: &Map<String, Value>, key: &str) {
    match map.get(key) {
        Some(Value::String(name)) => *field = Some(name.clone()),
        Some(Value::Null) => *field = None,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let prefs = Prefs {
            tool: Tool::Arrow,
            tool_locked: false,
            style: ItemStyle {
                stroke_color: "#e03131".into(),
                background_color: "#a5d8ff".into(),
                fill_style: "hachure".into(),
                stroke_width: StrokeWidth::Bold,
                stroke_style: "dashed".into(),
                roughness: 2.0,
                opacity: 60.0,
                edges: EdgeStyle::Sharp,
                arrow_type: ArrowType::Sharp,
                start_arrowhead: Some("dot".into()),
                end_arrowhead: None,
                stroke_variability: "constant".into(),
                font_family: 2.0,
                font_size: 28.0,
            },
            panel: PanelLayout {
                collapsed: true,
                stroke_palette: false,
                background_palette: true,
            },
        };
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
    }

    #[test]
    fn bad_fields_fall_back_one_at_a_time() {
        let prefs = Prefs::from_json(&json!({
            "activeTool": "lasso",
            "toolLocked": "yes",
            "currentItemStrokeColor": "#2f9e44",
            "currentItemOpacity": 400,
            "currentItemEndArrowhead": 3,
        }));
        let default = Prefs::default();
        assert_eq!(prefs.tool, default.tool);
        assert_eq!(prefs.tool_locked, default.tool_locked);
        assert_eq!(prefs.style.stroke_color, "#2f9e44");
        assert_eq!(prefs.style.opacity, default.style.opacity);
        assert_eq!(prefs.style.end_arrowhead, default.style.end_arrowhead);
        assert_eq!(Prefs::from_json(&json!([1, 2])), default);
    }

    #[test]
    fn a_missing_or_corrupt_file_loads_the_default() {
        let dir = std::env::temp_dir().join(format!("napkin-prefs-{}", std::process::id()));
        let path = dir.join("prefs.json");
        assert_eq!(Prefs::load(&path), Prefs::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default());
        let saved = Prefs {
            tool: Tool::Line,
            ..Prefs::default()
        };
        saved.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), saved);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
