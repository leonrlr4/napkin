//! `bin/fillers/zigzag-line-filler.js`.

use crate::core::{Op, OpSet, OpSetType, Point};
use crate::geometry::line_length;
use crate::js;
use crate::js::math_round;
use crate::renderer::{self, Ctx};

use super::scan_line_hachure::polygon_hachure_lines;

/// `bin/fillers/zigzag-line-filler.js` `ZigZagLineFiller.fillPolygons`.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    let gap = if o.o.hachure_gap < 0.0 {
        o.o.stroke_width * 4.0
    } else {
        o.o.hachure_gap
    };
    let zo = if o.o.zigzag_offset < 0.0 {
        gap
    } else {
        o.o.zigzag_offset
    };
    // `Object.assign({}, o, { hachureGap: gap + zo })`; `zigzagLines` below runs on this
    // copy, not the original `o`.
    let mut o2 = o.clone();
    o2.o.hachure_gap = gap + zo;
    let lines = polygon_hachure_lines(polygon_list, &mut o2);
    OpSet {
        kind: OpSetType::FillSketch,
        ops: zigzag_lines(&lines, zo, &mut o2),
    }
}

/// `bin/fillers/zigzag-line-filler.js` `ZigZagLineFiller.zigzagLines`.
fn zigzag_lines(lines: &[[Point; 2]], zo: f64, o: &mut Ctx) -> Vec<Op> {
    let mut ops = Vec::new();
    for line in lines {
        let length = line_length(line);
        let count = math_round(length / (2.0 * zo));
        let (mut p1, mut p2) = (line[0], line[1]);
        if p1[0] > p2[0] {
            p1 = line[1];
            p2 = line[0];
        }
        // `Math.atan(dy / dx)`, not `atan2`.
        let alpha = js::atan((p2[1] - p1[1]) / (p2[0] - p1[0]));
        let mut i = 0.0;
        while i < count {
            let lstart = i * 2.0 * zo;
            let lend = (i + 1.0) * 2.0 * zo;
            let dz = (2.0 * zo.powf(2.0)).sqrt();
            let start = [p1[0] + (lstart * alpha.cos()), p1[1] + lstart * alpha.sin()];
            let end = [p1[0] + (lend * alpha.cos()), p1[1] + (lend * alpha.sin())];
            let middle = [
                start[0] + dz * (alpha + std::f64::consts::PI / 4.0).cos(),
                start[1] + dz * (alpha + std::f64::consts::PI / 4.0).sin(),
            ];
            ops.extend(renderer::double_line_fill_ops(
                start[0], start[1], middle[0], middle[1], o,
            ));
            ops.extend(renderer::double_line_fill_ops(
                middle[0], middle[1], end[0], end[1], o,
            ));
            i += 1.0;
        }
    }
    ops
}
