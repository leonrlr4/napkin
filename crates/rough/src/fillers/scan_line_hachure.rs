//! `bin/fillers/scan-line-hachure.js`.

use crate::core::Point;
use crate::hachure_fill;
use crate::js::truthy;
use crate::math::math_random;
use crate::renderer::Ctx;

/// `bin/fillers/scan-line-hachure.js` `polygonHachureLines`.
pub(crate) fn polygon_hachure_lines(
    polygon_list: &mut [Vec<Point>],
    o: &mut Ctx,
) -> Vec<[Point; 2]> {
    let angle = o.o.hachure_angle + 90.0;
    let mut gap = o.o.hachure_gap;
    if gap < 0.0 {
        gap = o.o.stroke_width * 4.0;
    }
    gap = gap.max(0.1);
    let mut skip_offset = 1.0;
    if o.o.roughness >= 1.0 {
        // `(o.randomizer?.next() || Math.random()) > 0.7`: draws only if a randomizer
        // already exists on `o`, and falls back to `Math.random` when that draw is 0.
        let draw = o
            .existing_random()
            .filter(|v| truthy(*v))
            .unwrap_or_else(math_random);
        if draw > 0.7 {
            skip_offset = gap;
        }
    }
    hachure_fill::hachure_lines(
        polygon_list,
        gap,
        angle,
        if truthy(skip_offset) {
            skip_offset
        } else {
            1.0
        },
    )
}
