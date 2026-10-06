//! The top-centre toolbar (`Tools.tsx`): one button per tool, its icon drawn with the egui
//! painter so the app needs no icon font or image.

use eframe::egui;
use scene::editor::Tool;

use crate::theme::Theme;

/// Colors the toolbar draws with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolbarColors {
    /// The panel and an unselected, unhovered button.
    pub background: egui::Color32,
    /// Icon strokes and an unhovered button's outline.
    pub foreground: egui::Color32,
    /// The current tool's button fill.
    pub accent: egui::Color32,
}

impl ToolbarColors {
    pub fn from_theme(theme: &Theme) -> ToolbarColors {
        ToolbarColors {
            background: theme.background,
            foreground: theme.foreground,
            accent: theme.accent,
        }
    }
}

/// The toolbar's tools in order, with their shortcut hint (`Tools.tsx`).
pub const TOOLS: [(Tool, &str); 10] = [
    (Tool::Hand, "H"),
    (Tool::Selection, "V 1"),
    (Tool::Rectangle, "R 2"),
    (Tool::Diamond, "D 3"),
    (Tool::Ellipse, "O 4"),
    (Tool::Arrow, "A 5"),
    (Tool::Line, "L 6"),
    (Tool::Freedraw, "P 7"),
    (Tool::Text, "T 8"),
    (Tool::Eraser, "E 0"),
];

/// The tool's name, for the hover tooltip.
fn name(tool: Tool) -> &'static str {
    match tool {
        Tool::Hand => "Hand",
        Tool::Selection => "Selection",
        Tool::Rectangle => "Rectangle",
        Tool::Diamond => "Diamond",
        Tool::Ellipse => "Ellipse",
        Tool::Arrow => "Arrow",
        Tool::Line => "Line",
        Tool::Freedraw => "Draw",
        Tool::Text => "Text",
        Tool::Eraser => "Eraser",
    }
}

const BUTTON_SIZE: f32 = 32.0;
const ICON_MARGIN: f32 = 7.0;
const BUTTON_GAP: f32 = 2.0;
const CORNER_RADIUS: f32 = 4.0;

/// `tool`'s icon inside `rect`, stroked in `color`.
fn icon(painter: &egui::Painter, tool: Tool, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    match tool {
        Tool::Hand => {
            let palm = egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.center().y),
                rect.right_bottom(),
            );
            painter.rect_stroke(palm, 1.0, stroke, egui::StrokeKind::Inside);
            let step = rect.width() / 4.0;
            for i in 1..4 {
                let x = rect.min.x + step * i as f32;
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, palm.min.y)],
                    stroke,
                );
            }
        }
        Tool::Selection => {
            let points = vec![
                rect.left_top(),
                egui::pos2(rect.left_top().x, rect.bottom() - 2.0),
                egui::pos2(rect.center().x - 1.0, rect.bottom() - 5.0),
                egui::pos2(rect.right_top().x - 3.0, rect.bottom() - 3.0),
            ];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        Tool::Rectangle => {
            painter.rect_stroke(rect, 1.0, stroke, egui::StrokeKind::Inside);
        }
        Tool::Diamond => {
            let points = vec![
                egui::pos2(rect.center().x, rect.min.y),
                egui::pos2(rect.max.x, rect.center().y),
                egui::pos2(rect.center().x, rect.max.y),
                egui::pos2(rect.min.x, rect.center().y),
            ];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        Tool::Ellipse => {
            painter.circle_stroke(rect.center(), rect.width() / 2.0, stroke);
        }
        Tool::Arrow => {
            painter.arrow(
                rect.left_bottom(),
                rect.right_top() - rect.left_bottom(),
                stroke,
            );
        }
        Tool::Line => {
            painter.line_segment([rect.left_bottom(), rect.right_top()], stroke);
        }
        Tool::Freedraw => {
            let points = vec![
                rect.left_bottom(),
                egui::pos2(rect.center().x - 3.0, rect.center().y + 3.0),
                egui::pos2(rect.center().x + 1.0, rect.center().y - 2.0),
                rect.right_top(),
            ];
            painter.add(egui::Shape::line(points, stroke));
        }
        Tool::Text => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "T",
                egui::FontId::monospace(rect.height() * 0.85),
                color,
            );
        }
        Tool::Eraser => {
            let center = rect.center();
            let half = egui::vec2(rect.width() * 0.38, rect.height() * 0.22);
            let (sin, cos) = (-0.4_f32).sin_cos();
            let corners = [
                egui::vec2(-half.x, -half.y),
                egui::vec2(half.x, -half.y),
                egui::vec2(half.x, half.y),
                egui::vec2(-half.x, half.y),
            ];
            let points = corners
                .into_iter()
                .map(|v| center + egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos))
                .collect();
            painter.add(egui::Shape::closed_line(points, stroke));
        }
    }
}

/// Draws the toolbar at the top centre; returns the tool the user clicked.
pub fn show(ctx: &egui::Context, current: Tool, colors: ToolbarColors) -> Option<Tool> {
    egui::Area::new(egui::Id::new("napkin-toolbar"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
        .show(ctx, |ui| {
            let mut clicked = None;
            egui::Frame::new()
                .fill(colors.background)
                .corner_radius(CORNER_RADIUS + 2.0)
                .inner_margin(3.0)
                .stroke(egui::Stroke::new(
                    1.0,
                    colors.foreground.gamma_multiply(0.3),
                ))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                        for &(tool, hint) in &TOOLS {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(BUTTON_SIZE, BUTTON_SIZE),
                                egui::Sense::click(),
                            );
                            let selected = tool == current;
                            let icon_color = if selected {
                                painter_fill(ui, rect, colors.accent);
                                colors.background
                            } else {
                                if response.hovered() {
                                    painter_fill(
                                        ui,
                                        rect,
                                        egui::Color32::from_rgba_unmultiplied(
                                            colors.foreground.r(),
                                            colors.foreground.g(),
                                            colors.foreground.b(),
                                            30,
                                        ),
                                    );
                                }
                                colors.foreground
                            };
                            icon(ui.painter(), tool, rect.shrink(ICON_MARGIN), icon_color);
                            let response =
                                response.on_hover_text(format!("{} ({hint})", name(tool)));
                            if response.clicked() {
                                clicked = Some(tool);
                            }
                        }
                    });
                });
            clicked
        })
        .inner
}

fn painter_fill(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    ui.painter().rect_filled(rect, CORNER_RADIUS, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_in_order_with_their_shortcut_hints() {
        assert_eq!(
            TOOLS,
            [
                (Tool::Hand, "H"),
                (Tool::Selection, "V 1"),
                (Tool::Rectangle, "R 2"),
                (Tool::Diamond, "D 3"),
                (Tool::Ellipse, "O 4"),
                (Tool::Arrow, "A 5"),
                (Tool::Line, "L 6"),
                (Tool::Freedraw, "P 7"),
                (Tool::Text, "T 8"),
                (Tool::Eraser, "E 0"),
            ]
        );
    }
}
