//! The style values a newly created element takes, ported from `getDefaultAppState`
//! (`packages/excalidraw/appState.ts`) and `STROKE_WIDTH`, `FREEDRAW_STROKE_WIDTH` and
//! `ROUNDNESS` (`packages/common/src/constants.ts`) at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`. `getDefaultAppState` picks `currentItemRoundness`
//! sharp in Excalidraw's own test environment and round otherwise; napkin always starts round,
//! matching the non-test default a person actually sees.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeWidth {
    Thin,
    #[default]
    Medium,
    Bold,
    ExtraBold,
}

impl StrokeWidth {
    /// `STROKE_WIDTH` (1, 2, 4, 8), or `FREEDRAW_STROKE_WIDTH` (0.5, 1, 2, 4) when `freedraw`.
    pub fn value(self, freedraw: bool) -> f64 {
        match (self, freedraw) {
            (StrokeWidth::Thin, false) => 1.0,
            (StrokeWidth::Thin, true) => 0.5,
            (StrokeWidth::Medium, false) => 2.0,
            (StrokeWidth::Medium, true) => 1.0,
            (StrokeWidth::Bold, false) => 4.0,
            (StrokeWidth::Bold, true) => 2.0,
            (StrokeWidth::ExtraBold, false) => 8.0,
            (StrokeWidth::ExtraBold, true) => 4.0,
        }
    }
}

/// `currentItemRoundness`: whether a new rectangle, diamond, ellipse or line gets a
/// `roundness` of `ADAPTIVE_RADIUS`/`PROPORTIONAL_RADIUS` or none (`getCurrentItemRoundness`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeStyle {
    Sharp,
    #[default]
    Round,
}

/// `currentItemArrowType`: whether a new arrow gets a `roundness` of `PROPORTIONAL_RADIUS` or
/// none. Excalidraw's third option, `elbow`, does not apply: napkin never creates elbow arrows
/// (spec §1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrowType {
    Sharp,
    #[default]
    Round,
}

/// The appState `currentItem*` values new elements take (`getDefaultAppState`).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStyle {
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: StrokeWidth,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub edges: EdgeStyle,
    pub arrow_type: ArrowType,
    pub start_arrowhead: Option<String>,
    pub end_arrowhead: Option<String>,
    pub stroke_variability: String,
}

impl Default for ItemStyle {
    fn default() -> Self {
        ItemStyle {
            stroke_color: "#1e1e1e".to_string(),
            background_color: "transparent".to_string(),
            fill_style: "solid".to_string(),
            stroke_width: StrokeWidth::default(),
            stroke_style: "solid".to_string(),
            roughness: 1.0,
            opacity: 100.0,
            edges: EdgeStyle::default(),
            arrow_type: ArrowType::default(),
            start_arrowhead: None,
            end_arrowhead: Some("arrow".to_string()),
            stroke_variability: "constant".to_string(),
        }
    }
}
