//! `bin/fillers/dot-filler.js`.

use crate::core::{OpSet, OpSetType, Point};
use crate::geometry::line_length;
use crate::math::math_random;
use crate::renderer::{self, Ctx};

use super::scan_line_hachure::polygon_hachure_lines;

/// `bin/fillers/dot-filler.js` `DotFiller.fillPolygons`.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    // `Object.assign({}, o, { hachureAngle: 0 })`; `dotsOnLines` below runs on this copy,
    // not the original `o`.
    let mut o2 = o.clone();
    o2.o.hachure_angle = 0.0;
    let lines = polygon_hachure_lines(polygon_list, &mut o2);
    dots_on_lines(&lines, &mut o2)
}

/// `bin/fillers/dot-filler.js` `DotFiller.dotsOnLines`.
fn dots_on_lines(lines: &[[Point; 2]], o: &mut Ctx) -> OpSet {
    let mut ops = Vec::new();
    let mut gap = o.o.hachure_gap;
    if gap < 0.0 {
        gap = o.o.stroke_width * 4.0;
    }
    gap = gap.max(0.1);
    let mut fweight = o.o.fill_weight;
    if fweight < 0.0 {
        fweight = o.o.stroke_width / 2.0;
    }
    let ro = gap / 4.0;
    for line in lines {
        let length = line_length(line);
        let dl = length / gap;
        let count = dl.ceil() - 1.0;
        let offset = length - (count * gap);
        let x = ((line[0][0] + line[1][0]) / 2.0) - (gap / 4.0);
        let min_y = line[0][1].min(line[1][1]);
        let mut i = 0.0;
        while i < count {
            let y = min_y + offset + (i * gap);
            // `Math.random()` here is not reproducible; baselines for this filler compare
            // structure only, per m1-global-rules.md decision 2.
            let cx = (x - ro) + math_random() * 2.0 * ro;
            let cy = (y - ro) + math_random() * 2.0 * ro;
            let el = renderer::ellipse(cx, cy, fweight, fweight, o);
            ops.extend(el.ops);
            i += 1.0;
        }
    }
    OpSet {
        kind: OpSetType::FillSketch,
        ops,
    }
}
