//! Turns the editor's [`scene::editor::Overlay`] (selection outlines, transform handles, line
//! points and the box-selection rectangle, all in scene coordinates) into egui shapes in screen
//! points.

use eframe::egui;
use scene::editor::Overlay;
use scene::geometry::Bounds;

use crate::camera::Camera;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayColors {
    pub accent: egui::Color32,
    pub selection: egui::Color32,
    pub handle_fill: egui::Color32,
}

impl OverlayColors {
    pub fn from_theme(theme: &Theme) -> OverlayColors {
        OverlayColors {
            accent: theme.accent,
            selection: theme.selection,
            handle_fill: theme.background,
        }
    }
}

/// `p`'s screen position for a canvas whose top-left corner is `origin`.
fn to_screen(camera: &Camera, origin: egui::Pos2, p: [f64; 2]) -> egui::Pos2 {
    let view = camera.scene_to_view(p);
    origin + egui::vec2(view[0] as f32, view[1] as f32)
}

fn to_rect(camera: &Camera, origin: egui::Pos2, bounds: Bounds) -> egui::Rect {
    let [x1, y1, x2, y2] = bounds;
    egui::Rect::from_two_pos(
        to_screen(camera, origin, [x1, y1]),
        to_screen(camera, origin, [x2, y2]),
    )
}

/// `color` with its alpha replaced by `alpha`.
fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// `overlay` in screen points for a canvas whose top-left corner is `origin`.
pub fn shapes(
    overlay: &Overlay,
    camera: &Camera,
    origin: egui::Pos2,
    colors: OverlayColors,
) -> Vec<egui::Shape> {
    let mut shapes = Vec::new();
    let stroke = egui::Stroke::new(1.0, colors.accent);

    for outline in &overlay.outlines {
        let points = outline
            .iter()
            .map(|&p| to_screen(camera, origin, p))
            .collect();
        shapes.push(egui::Shape::closed_line(points, stroke));
    }

    if let Some(bounds) = overlay.selection_box {
        let rect = to_rect(camera, origin, bounds);
        let path = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        shapes.extend(egui::Shape::dashed_line(&path, stroke, 2.0, 2.0));
    }

    for &handle in &overlay.handles {
        let rect = to_rect(camera, origin, handle);
        shapes.push(egui::Shape::Rect(egui::epaint::RectShape::new(
            rect,
            2.0,
            colors.handle_fill,
            stroke,
            egui::StrokeKind::Outside,
        )));
    }

    for &point in &overlay.points {
        let center = to_screen(camera, origin, point);
        shapes.push(egui::Shape::Circle(egui::epaint::CircleShape {
            center,
            radius: 5.0,
            fill: colors.handle_fill,
            stroke,
        }));
    }

    if let Some(bounds) = overlay.box_selection {
        let rect = to_rect(camera, origin, bounds);
        shapes.push(egui::Shape::Rect(egui::epaint::RectShape::new(
            rect,
            0.0,
            with_alpha(colors.selection, 64),
            stroke,
            egui::StrokeKind::Outside,
        )));
    }

    shapes
}

#[cfg(test)]
mod tests {
    use scene::editor::Overlay;

    use super::*;

    fn colors() -> OverlayColors {
        OverlayColors {
            accent: egui::Color32::BLUE,
            selection: egui::Color32::LIGHT_BLUE,
            handle_fill: egui::Color32::WHITE,
        }
    }

    #[test]
    fn scene_geometry_lands_in_screen_points() {
        let overlay = Overlay {
            handles: vec![[0.0, 0.0, 4.0, 4.0]],
            points: vec![[10.0, 5.0]],
            box_selection: Some([0.0, 0.0, 10.0, 10.0]),
            ..Overlay::default()
        };
        let camera = Camera {
            scroll_x: 1.0,
            scroll_y: 0.0,
            zoom: 2.0,
        };
        let shapes = shapes(&overlay, &camera, egui::pos2(10.0, 20.0), colors());
        let rects: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r.rect),
                _ => None,
            })
            .collect();
        assert!(
            rects.contains(&egui::Rect::from_min_max(
                egui::pos2(12.0, 20.0),
                egui::pos2(20.0, 28.0)
            )),
            "{rects:?}"
        );
        assert!(
            rects.contains(&egui::Rect::from_min_max(
                egui::pos2(12.0, 20.0),
                egui::pos2(32.0, 40.0)
            )),
            "{rects:?}"
        );
        let centers: Vec<egui::Pos2> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Circle(c) => Some(c.center),
                _ => None,
            })
            .collect();
        assert_eq!(centers, vec![egui::pos2(32.0, 30.0)]);
    }
}
