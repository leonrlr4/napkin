//! The property panel on the left (spec §7.2), drawing [`scene::editor::PanelState`] and
//! turning a picked value into a [`Property`] for `Editor::set_property`. Ported from
//! `packages/excalidraw/components/Actions.tsx`'s `SelectedShapeActions` (one row per
//! [`Section`]) and the individual `PanelComponent`s in
//! `packages/excalidraw/actions/actionProperties.tsx`, at commit
//! `afa3a653fc5d2b742adcbd5a6063187b056d2419`.
//!
//! The quick-pick and full-palette colors come from `packages/common/src/colors.ts`. The full
//! palette grid (`DEFAULT_ELEMENT_STROKE_COLOR_PALETTE`/`DEFAULT_ELEMENT_BACKGROUND_COLOR_PALETTE`)
//! is an object of 15 keys (`transparent`, `white`, `gray`, `black`, `bronze`, then the ten
//! `COMMON_ELEMENT_SHADES` hues), each either a single color or a 5-shade array;
//! `PickerColorList` renders one swatch per key, picking `value[activeShade]` for an array.
//! `Picker.tsx` seeds `activeShade` from `DEFAULT_ELEMENT_BACKGROUND_COLOR_INDEX` (1) for a
//! background palette and `DEFAULT_ELEMENT_STROKE_COLOR_INDEX` (4) for a stroke palette.

use eframe::egui;
use scene::color::{apply_dark_mode_filter, tinycolor};
use scene::editor::{ArrowType, EdgeStyle, PanelState, Property, Section, StrokeWidth};

use crate::theme::Theme;

/// Colors the panel draws with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelColors {
    pub background: egui::Color32,
    pub foreground: egui::Color32,
    pub accent: egui::Color32,
    pub muted: egui::Color32,
}

impl PanelColors {
    pub fn from_theme(theme: &Theme) -> PanelColors {
        PanelColors {
            background: theme.background,
            foreground: theme.foreground,
            accent: theme.accent,
            muted: theme.muted,
        }
    }
}

/// `DEFAULT_ELEMENT_STROKE_PICKS`: `COLOR_PALETTE.black`, then red/green/blue/yellow at
/// `DEFAULT_ELEMENT_STROKE_COLOR_INDEX` (4).
pub const STROKE_PICKS: [&str; 5] = ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"];

/// `DEFAULT_ELEMENT_BACKGROUND_PICKS`: `transparent`, then red/green/blue/yellow at
/// `DEFAULT_ELEMENT_BACKGROUND_COLOR_INDEX` (1).
pub const BACKGROUND_PICKS: [&str; 5] = ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"];

/// `DEFAULT_ELEMENT_STROKE_COLOR_PALETTE` flattened to one swatch per key at shade index 4, in
/// the object's insertion order (`transparent`, `white`, `gray`, `black`, `bronze`, then
/// `COMMON_ELEMENT_SHADES`: cyan, blue, violet, grape, pink, green, teal, yellow, orange, red).
const STROKE_PALETTE: [&str; 15] = [
    "transparent",
    "#ffffff",
    "#343a40",
    "#1e1e1e",
    "#846358",
    "#0c8599",
    "#1971c2",
    "#6741d9",
    "#9c36b5",
    "#c2255c",
    "#2f9e44",
    "#099268",
    "#f08c00",
    "#e8590c",
    "#e03131",
];

/// Same shape as [`STROKE_PALETTE`], at shade index 1.
const BACKGROUND_PALETTE: [&str; 15] = [
    "transparent",
    "#ffffff",
    "#e9ecef",
    "#1e1e1e",
    "#eaddd7",
    "#99e9f2",
    "#a5d8ff",
    "#d0bfff",
    "#eebefa",
    "#fcc2d7",
    "#b2f2bb",
    "#96f2d7",
    "#ffec99",
    "#ffd8a8",
    "#ffc9c9",
];

const FILL_STYLE_OPTIONS: [(&str, &str); 3] = [
    ("Hachure", "hachure"),
    ("Cross-hatch", "cross-hatch"),
    ("Solid", "solid"),
];

const STROKE_WIDTH_OPTIONS: [(&str, StrokeWidth); 3] = [
    ("Thin", StrokeWidth::Thin),
    ("Medium", StrokeWidth::Medium),
    ("Bold", StrokeWidth::Bold),
];

const STROKE_STYLE_OPTIONS: [(&str, &str); 3] = [
    ("Solid", "solid"),
    ("Dashed", "dashed"),
    ("Dotted", "dotted"),
];

const ROUGHNESS_OPTIONS: [(&str, f64); 3] =
    [("Architect", 0.0), ("Artist", 1.0), ("Cartoonist", 2.0)];

const EDGE_OPTIONS: [(&str, EdgeStyle); 2] =
    [("Sharp", EdgeStyle::Sharp), ("Round", EdgeStyle::Round)];

const ARROW_TYPE_OPTIONS: [(&str, ArrowType); 2] =
    [("Sharp", ArrowType::Sharp), ("Round", ArrowType::Round)];

/// `getArrowheadOptions`'s visible section only (`none`/`arrow`/`triangle`/`triangle_outline`);
/// the hidden section (circle, diamond, bar, cardinality markers) is left out of this panel.
const ARROWHEAD_OPTIONS: [(&str, Option<&str>); 4] = [
    ("None", None),
    ("Arrow", Some("arrow")),
    ("Triangle", Some("triangle")),
    ("Outline", Some("triangle_outline")),
];

/// `FONT_FAMILY.Excalifont`/`Nunito`/`"Comic Shanns"`, labeled as `DEFAULT_FONTS` does
/// (hand-drawn/normal/code).
const FONT_FAMILY_OPTIONS: [(&str, f64); 3] = [("Hand-drawn", 5.0), ("Normal", 6.0), ("Code", 8.0)];

/// `FONT_SIZES.sm`/`md`/`lg`/`xl`.
const FONT_SIZE_OPTIONS: [(&str, f64); 4] = [("S", 16.0), ("M", 20.0), ("L", 28.0), ("XL", 36.0)];

const PANEL_WIDTH: f32 = 200.0;
const SWATCH_SIZE: f32 = 20.0;

fn title(section: Section) -> &'static str {
    match section {
        Section::StrokeColor => "Stroke",
        Section::BackgroundColor => "Background",
        Section::FillStyle => "Fill",
        Section::StrokeWidth => "Stroke width",
        Section::StrokeStyle => "Stroke style",
        Section::Roughness => "Sloppiness",
        Section::Edges => "Edges",
        Section::ArrowType => "Arrow type",
        Section::Arrowheads => "Arrowheads",
        Section::FontFamily => "Font family",
        Section::FontSize => "Font size",
        Section::Opacity => "Opacity",
    }
}

/// `applyDarkModeFilter(color, theme === THEME.DARK)` turned into a paintable color (the file
/// keeps storing the light-mode value; only the swatch's displayed color changes).
fn to_egui_color(color: &str, dark: bool) -> egui::Color32 {
    let filtered = if dark {
        apply_dark_mode_filter(color)
    } else {
        color.to_string()
    };
    let tc = tinycolor(&filtered);
    egui::Color32::from_rgba_unmultiplied(
        tc.r.round().clamp(0.0, 255.0) as u8,
        tc.g.round().clamp(0.0, 255.0) as u8,
        tc.b.round().clamp(0.0, 255.0) as u8,
        (tc.a * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// One color swatch; returns whether it was clicked this frame. A transparent swatch is drawn
/// as an outlined box with a diagonal line (no pixels to sample a fill color from).
fn color_swatch(
    ui: &mut egui::Ui,
    color: &str,
    dark: bool,
    selected: bool,
    colors: PanelColors,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SWATCH_SIZE, SWATCH_SIZE), egui::Sense::click());
    let painter = ui.painter();
    if color == "transparent" {
        painter.rect_filled(rect, 3.0, colors.background);
        painter.line_segment(
            [rect.left_bottom(), rect.right_top()],
            egui::Stroke::new(1.0, colors.foreground.gamma_multiply(0.6)),
        );
    } else {
        painter.rect_filled(rect, 3.0, to_egui_color(color, dark));
    }
    let outline = if selected {
        egui::Stroke::new(2.0, colors.accent)
    } else {
        egui::Stroke::new(1.0, colors.foreground.gamma_multiply(0.3))
    };
    painter.rect_stroke(rect, 3.0, outline, egui::StrokeKind::Inside);
    response.on_hover_text(color).clicked()
}

/// A color section: the five quick picks, plus a "more" toggle that expands the full
/// 5-per-row palette grid below it. `id_source` keeps this section's expanded/collapsed state
/// (kept in egui's temporary memory, since it is pure UI state with no effect on the document)
/// distinct from the other color section's.
fn color_section(
    ui: &mut egui::Ui,
    id_source: &str,
    picks: &[&str; 5],
    palette: &[&str; 15],
    current: Option<&str>,
    colors: PanelColors,
    dark: bool,
) -> Option<String> {
    let mut picked = None;
    let expand_id = ui.id().with((id_source, "expanded"));
    let mut expanded = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(expand_id))
        .unwrap_or(false);
    ui.horizontal(|ui| {
        for &color in picks {
            if color_swatch(ui, color, dark, current == Some(color), colors) {
                picked = Some(color.to_string());
            }
        }
        if ui.small_button("...").clicked() {
            expanded = !expanded;
            ui.ctx().data_mut(|d| d.insert_temp(expand_id, expanded));
        }
    });
    if expanded {
        egui::Grid::new(ui.id().with((id_source, "grid")))
            .num_columns(5)
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                for (i, &color) in palette.iter().enumerate() {
                    if color_swatch(ui, color, dark, current == Some(color), colors) {
                        picked = Some(color.to_string());
                    }
                    if (i + 1) % 5 == 0 {
                        ui.end_row();
                    }
                }
            });
    }
    picked
}

/// A row of toggle buttons, one per `options` entry; returns the value of the one clicked this
/// frame, if any. `current` highlights the matching button, or none when it is `None` (mixed
/// selection).
fn buttons_row<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    options: &[(&str, T)],
    current: Option<T>,
) -> Option<T> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for &(label, value) in options {
            if ui
                .add(egui::Button::new(label).selected(current == Some(value)))
                .clicked()
            {
                picked = Some(value);
            }
        }
    });
    picked
}

fn arrowheads_row(ui: &mut egui::Ui, state: &PanelState, colors: PanelColors) -> Option<Property> {
    let mut picked = None;
    ui.label(egui::RichText::new("Start").small().color(colors.muted));
    let start_current = state.start_arrowhead.as_ref().map(|end| end.as_deref());
    if let Some(value) = buttons_row(ui, &ARROWHEAD_OPTIONS, start_current) {
        picked = Some(Property::StartArrowhead(value.map(str::to_string)));
    }
    ui.label(egui::RichText::new("End").small().color(colors.muted));
    let end_current = state.end_arrowhead.as_ref().map(|end| end.as_deref());
    if let Some(value) = buttons_row(ui, &ARROWHEAD_OPTIONS, end_current) {
        picked = Some(Property::EndArrowhead(value.map(str::to_string)));
    }
    picked
}

/// `Range`: a slider from 0 to 100 in steps of 10 (`actionChangeOpacity`'s `PanelComponent`).
/// With a mixed selection (`current` is `None`), the slider has nothing of its own to display
/// and starts at 100 (fully opaque) rather than reading back the current item style, which this
/// panel has no access to; the starting position does not affect what a drag then writes.
fn opacity_slider(ui: &mut egui::Ui, current: Option<f64>) -> Option<f64> {
    let mut value = current.unwrap_or(100.0);
    let response = ui.add(
        egui::Slider::new(&mut value, 0.0..=100.0)
            .step_by(10.0)
            .suffix("%"),
    );
    response.changed().then_some(value)
}

fn section_row(
    ui: &mut egui::Ui,
    section: Section,
    state: &PanelState,
    colors: PanelColors,
    dark: bool,
) -> Option<Property> {
    match section {
        Section::StrokeColor => color_section(
            ui,
            "stroke",
            &STROKE_PICKS,
            &STROKE_PALETTE,
            state.stroke_color.as_deref(),
            colors,
            dark,
        )
        .map(Property::StrokeColor),
        Section::BackgroundColor => color_section(
            ui,
            "background",
            &BACKGROUND_PICKS,
            &BACKGROUND_PALETTE,
            state.background_color.as_deref(),
            colors,
            dark,
        )
        .map(Property::BackgroundColor),
        Section::FillStyle => buttons_row(ui, &FILL_STYLE_OPTIONS, state.fill_style.as_deref())
            .map(|value| Property::FillStyle(value.to_string())),
        Section::StrokeWidth => {
            buttons_row(ui, &STROKE_WIDTH_OPTIONS, state.stroke_width).map(Property::StrokeWidth)
        }
        Section::StrokeStyle => {
            buttons_row(ui, &STROKE_STYLE_OPTIONS, state.stroke_style.as_deref())
                .map(|value| Property::StrokeStyle(value.to_string()))
        }
        Section::Roughness => {
            buttons_row(ui, &ROUGHNESS_OPTIONS, state.roughness).map(Property::Roughness)
        }
        Section::Edges => buttons_row(ui, &EDGE_OPTIONS, state.edges).map(Property::Edges),
        Section::ArrowType => {
            buttons_row(ui, &ARROW_TYPE_OPTIONS, state.arrow_type).map(Property::ArrowType)
        }
        Section::Arrowheads => arrowheads_row(ui, state, colors),
        Section::FontFamily => {
            buttons_row(ui, &FONT_FAMILY_OPTIONS, state.font_family).map(Property::FontFamily)
        }
        Section::FontSize => {
            buttons_row(ui, &FONT_SIZE_OPTIONS, state.font_size).map(Property::FontSize)
        }
        Section::Opacity => opacity_slider(ui, state.opacity).map(Property::Opacity),
    }
}

/// Draws the panel on the left when `state.sections` is non-empty; returns the property the
/// user picked.
pub fn show(
    ctx: &egui::Context,
    state: &PanelState,
    colors: PanelColors,
    dark: bool,
) -> Option<Property> {
    if state.sections.is_empty() {
        return None;
    }
    let mut picked = None;
    egui::Area::new(egui::Id::new("napkin-properties-panel"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 52.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(colors.background)
                .corner_radius(6.0)
                .inner_margin(8.0)
                .stroke(egui::Stroke::new(
                    1.0,
                    colors.foreground.gamma_multiply(0.3),
                ))
                .show(ui, |ui| {
                    ui.set_max_width(PANEL_WIDTH);
                    for &section in &state.sections {
                        ui.label(
                            egui::RichText::new(title(section))
                                .small()
                                .color(colors.muted),
                        );
                        if let Some(property) = section_row(ui, section, state, colors, dark) {
                            picked = Some(property);
                        }
                        ui.add_space(6.0);
                    }
                });
        });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_and_background_quick_picks_match_the_js_defaults() {
        assert_eq!(
            STROKE_PICKS,
            ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"]
        );
        assert_eq!(
            BACKGROUND_PICKS,
            ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"]
        );
    }

    /// `Object.entries(DEFAULT_ELEMENT_STROKE_COLOR_PALETTE).length` (and the background
    /// palette's) is 15: `transparent`, `white`, `gray`, `black`, `bronze`, plus the ten
    /// `COMMON_ELEMENT_SHADES` hues.
    #[test]
    fn palette_grids_have_the_same_swatch_count_as_js() {
        assert_eq!(STROKE_PALETTE.len(), 15);
        assert_eq!(BACKGROUND_PALETTE.len(), 15);
    }

    #[test]
    fn palettes_share_the_colorless_entries_and_differ_only_by_shade() {
        for (stroke, background) in STROKE_PALETTE.iter().zip(BACKGROUND_PALETTE.iter()) {
            let colorless = ["transparent", "#ffffff", "#1e1e1e"];
            if colorless.contains(stroke) || colorless.contains(background) {
                assert_eq!(stroke, background);
            }
        }
    }

    #[test]
    fn empty_sections_draw_nothing_and_pick_nothing() {
        let ctx = egui::Context::default();
        let state = PanelState::default();
        let colors = PanelColors {
            background: egui::Color32::BLACK,
            foreground: egui::Color32::WHITE,
            accent: egui::Color32::RED,
            muted: egui::Color32::GRAY,
        };
        let mut output = ctx.run_ui(Default::default(), |ui| {
            assert_eq!(show(ui.ctx(), &state, colors, false), None);
        });
        // `FullOutput` panics on drop if its `textures_delta` is not explicitly cleared.
        output.textures_delta.clear();
    }
}
