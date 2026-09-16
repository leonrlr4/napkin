//! `bin/fillers/zigzag-filler.js`.

use crate::core::{OpSet, OpSetType, Point};
use crate::geometry::line_length;
use crate::js::truthy;
use crate::renderer::Ctx;

use super::hachure::render_lines;
use super::scan_line_hachure::polygon_hachure_lines;

/// `bin/fillers/zigzag-filler.js` `ZigZagFiller.fillPolygons`.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    let mut gap = o.o.hachure_gap;
    if gap < 0.0 {
        gap = o.o.stroke_width * 4.0;
    }
    gap = gap.max(0.1);
    // `Object.assign({}, o, { hachureGap: gap })`; `renderLines` below is called with the
    // original `o`, not this copy.
    let mut o2 = o.clone();
    o2.o.hachure_gap = gap;
    let lines = polygon_hachure_lines(polygon_list, &mut o2);
    let zig_zag_angle = (std::f64::consts::PI / 180.0) * o.o.hachure_angle;
    let mut zigzag_lines: Vec<[Point; 2]> = Vec::new();
    let dgx = gap * 0.5 * zig_zag_angle.cos();
    let dgy = gap * 0.5 * zig_zag_angle.sin();
    for line in &lines {
        let [p1, p2] = *line;
        // `if (lineLength([p1, p2]))`: a number's truthiness, not a zero-length check.
        if truthy(line_length(line)) {
            zigzag_lines.push([[p1[0] - dgx, p1[1] + dgy], p2]);
            zigzag_lines.push([[p1[0] + dgx, p1[1] - dgy], p2]);
        }
    }
    let ops = render_lines(&zigzag_lines, o);
    OpSet {
        kind: OpSetType::FillSketch,
        ops,
    }
}
