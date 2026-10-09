//! Placing prepared images on the canvas: `newImagePlaceholder` and `getImageNaturalDimensions`
//! (`packages/excalidraw/components/App.tsx`) and `positionElementsOnGrid`
//! (`packages/element/src/positionElementsOnGrid.ts`) at the pinned commit.
//! [`Editor::insert_images`](crate::editor::Editor::insert_images) puts them together.

use crate::file::FileData;

/// One prepared image the app hands to the editor: everything `initializeImage` derives from
/// the file before the element exists.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImage {
    pub file: FileData,
    pub natural_size: [f64; 2],
}

/// `newImagePlaceholder` + `getImageNaturalDimensions`: the element's `[x, y, width, height]`
/// for an image of `natural_size` inserted centered on `at`, given the canvas's height in
/// screen points and the zoom.
pub fn natural_placement(
    at: [f64; 2],
    natural_size: [f64; 2],
    canvas_height: f64,
    zoom: f64,
) -> [f64; 4] {
    let placeholder_size = 100.0 / zoom;
    let placeholder = [
        at[0] - placeholder_size / 2.0,
        at[1] - placeholder_size / 2.0,
        placeholder_size,
        placeholder_size,
    ];

    let min_height = (canvas_height - 120.0).max(160.0);
    let max_height = min_height.min((canvas_height * 0.5).floor() / zoom);
    let height = natural_size[1].min(max_height);
    let width = height * (natural_size[0] / natural_size[1]);

    [
        placeholder[0] + placeholder[2] / 2.0 - width / 2.0,
        placeholder[1] + placeholder[3] / 2.0 - height / 2.0,
        width,
        height,
    ]
}

/// `positionElementsOnGrid` for single-element units: a roughly square grid of
/// `ceil(sqrt(n))` columns, `padding` between cells, each row centered horizontally and the
/// whole grid vertically on `center`. Each placement is `[x, y, width, height]`; only the
/// position changes.
pub fn position_on_grid(placements: &[[f64; 4]], center: [f64; 2], padding: f64) -> Vec<[f64; 4]> {
    if placements.is_empty() {
        return Vec::new();
    }
    let columns = ((placements.len() as f64).sqrt().ceil() as usize).max(1);
    let rows: Vec<&[[f64; 4]]> = placements.chunks(columns).collect();

    let row_width = |row: &[[f64; 4]]| -> f64 {
        row.iter().map(|p| p[2]).sum::<f64>() + (row.len() - 1) as f64 * padding
    };
    let row_height = |row: &[[f64; 4]]| -> f64 { row.iter().fold(0.0, |max, p| p[3].max(max)) };

    let content_height: f64 = rows.iter().map(|row| row_height(row)).sum();
    let total_height = content_height + (rows.len() - 1) as f64 * padding;

    let mut result = Vec::with_capacity(placements.len());
    let mut y = center[1] - total_height / 2.0;
    for row in rows {
        let mut x = center[0] - row_width(row) / 2.0;
        for p in row {
            result.push([x, y, p[2], p[3]]);
            x += p[2] + padding;
        }
        y += row_height(row) + padding;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lays_out_four_units_in_two_rows_centered_on_the_point() {
        let cells = [[0.0, 0.0, 10.0, 20.0]; 4];
        let placed = position_on_grid(&cells, [0.0, 0.0], 5.0);
        assert_eq!(
            placed,
            [
                [-12.5, -22.5, 10.0, 20.0],
                [2.5, -22.5, 10.0, 20.0],
                [-12.5, 2.5, 10.0, 20.0],
                [2.5, 2.5, 10.0, 20.0],
            ]
        );
    }

    #[test]
    fn grid_of_three_puts_the_short_last_row_on_the_center_line() {
        let cells = [[0.0, 0.0, 10.0, 10.0]; 3];
        let placed = position_on_grid(&cells, [0.0, 0.0], 10.0);
        assert_eq!(placed[2], [-5.0, 5.0, 10.0, 10.0]);
        assert!(position_on_grid(&[], [0.0, 0.0], 10.0).is_empty());
    }
}
