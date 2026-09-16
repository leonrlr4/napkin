//! `bin/fillers/filler.js`.

use crate::core::{OpSet, Point};
use crate::renderer::Ctx;

mod dashed;
mod dot;
mod hachure;
mod hatch;
pub(crate) mod scan_line_hachure;
mod zigzag;
mod zigzag_line;

/// `bin/fillers/filler.js` `getFiller` combined with the filler's `fillPolygons`. Fillers
/// carry no state (`this.helper` aside), so the JS module-level cache keyed by `fillStyle`
/// name is not ported: it never changes what a call returns, only whether a filler instance
/// is reused.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    match o.o.fill_style.as_str() {
        "zigzag" => zigzag::fill_polygons(polygon_list, o),
        "cross-hatch" => hatch::fill_polygons(polygon_list, o),
        "dots" => dot::fill_polygons(polygon_list, o),
        "dashed" => dashed::fill_polygons(polygon_list, o),
        "zigzag-line" => zigzag_line::fill_polygons(polygon_list, o),
        _ => hachure::fill_polygons(polygon_list, o),
    }
}
