//! `bin/fillers/hatch-filler.js`.

use crate::core::{OpSet, Point};
use crate::renderer::Ctx;

use super::hachure;

/// `bin/fillers/hatch-filler.js` `HatchFiller.fillPolygons`. The second `_fillPolygons` call
/// reuses `polygon_list`, so it sees the floating-point drift `hachureLines` leaves behind
/// after rotating it out and back for the first call.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    let mut set = hachure::fill_polygons(polygon_list, o);
    // `Object.assign({}, o, { hachureAngle: o.hachureAngle + 90 })`.
    let mut o2 = o.clone();
    o2.o.hachure_angle += 90.0;
    let set2 = hachure::fill_polygons(polygon_list, &mut o2);
    set.ops.extend(set2.ops);
    set
}
