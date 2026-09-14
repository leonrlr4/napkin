//! `bin/fillers/dashed-filler.js`.

use crate::core::{Op, OpSet, OpSetType, Point};
use crate::geometry::line_length;
use crate::renderer::{self, Ctx};

use super::scan_line_hachure::polygon_hachure_lines;

/// `bin/fillers/dashed-filler.js` `DashedFiller.fillPolygons`.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    let lines = polygon_hachure_lines(polygon_list, o);
    OpSet {
        kind: OpSetType::FillSketch,
        ops: dashed_line(&lines, o),
    }
}

/// `bin/fillers/dashed-filler.js` `DashedFiller.dashedLine`.
fn dashed_line(lines: &[[Point; 2]], o: &mut Ctx) -> Vec<Op> {
    // `dashOffset`/`dashGap` fall back to `hachureGap`, which itself falls back to
    // `strokeWidth * 4`, when negative.
    let offset = if o.o.dash_offset < 0.0 {
        if o.o.hachure_gap < 0.0 {
            o.o.stroke_width * 4.0
        } else {
            o.o.hachure_gap
        }
    } else {
        o.o.dash_offset
    };
    let gap = if o.o.dash_gap < 0.0 {
        if o.o.hachure_gap < 0.0 {
            o.o.stroke_width * 4.0
        } else {
            o.o.hachure_gap
        }
    } else {
        o.o.dash_gap
    };
    let mut ops = Vec::new();
    for line in lines {
        let length = line_length(line);
        let count = (length / (offset + gap)).floor();
        let start_offset = (length + gap - (count * (offset + gap))) / 2.0;
        let (mut p1, mut p2) = (line[0], line[1]);
        if p1[0] > p2[0] {
            p1 = line[1];
            p2 = line[0];
        }
        // `Math.atan(dy / dx)`, not `atan2`.
        let alpha = ((p2[1] - p1[1]) / (p2[0] - p1[0])).atan();
        let mut i = 0.0;
        while i < count {
            let lstart = i * (offset + gap);
            let lend = lstart + offset;
            let start = [
                p1[0] + (lstart * alpha.cos()) + (start_offset * alpha.cos()),
                p1[1] + lstart * alpha.sin() + (start_offset * alpha.sin()),
            ];
            let end = [
                p1[0] + (lend * alpha.cos()) + (start_offset * alpha.cos()),
                p1[1] + (lend * alpha.sin()) + (start_offset * alpha.sin()),
            ];
            ops.extend(renderer::double_line_fill_ops(
                start[0], start[1], end[0], end[1], o,
            ));
            i += 1.0;
        }
    }
    ops
}
