//! `bin/fillers/hachure-filler.js`.

use crate::core::{Op, OpSet, OpSetType, Point};
use crate::renderer::{self, Ctx};

use super::scan_line_hachure::polygon_hachure_lines;

/// `bin/fillers/hachure-filler.js` `HachureFiller.fillPolygons`, which is `_fillPolygons`
/// unchanged; `HatchFiller` overrides `fillPolygons` but calls `_fillPolygons` directly, so
/// the port only needs the one function.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    let lines = polygon_hachure_lines(polygon_list, o);
    let ops = render_lines(&lines, o);
    OpSet {
        kind: OpSetType::FillSketch,
        ops,
    }
}

/// `bin/fillers/hachure-filler.js` `HachureFiller.renderLines`. Shared by `HatchFiller` and
/// `ZigZagFiller`, which inherit it unchanged.
pub(crate) fn render_lines(lines: &[[Point; 2]], o: &mut Ctx) -> Vec<Op> {
    let mut ops = Vec::new();
    for line in lines {
        ops.extend(renderer::double_line_fill_ops(
            line[0][0], line[0][1], line[1][0], line[1][1], o,
        ));
    }
    ops
}
