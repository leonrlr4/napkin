//! `bin/fillers/filler.js`.

use crate::core::{OpSet, Point};
use crate::renderer::Ctx;

mod hachure;
mod hatch;
pub(crate) mod scan_line_hachure;
mod zigzag;

/// `bin/fillers/filler.js` `getFiller` combined with the filler's `fillPolygons`. Fillers
/// carry no state (`this.helper` aside), so the JS module-level cache keyed by `fillStyle`
/// name is not ported: it never changes what a call returns, only whether a filler instance
/// is reused.
///
/// `"dots"`, `"dashed"` and `"zigzag-line"` are ported in Task 11.
pub(crate) fn fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet {
    match o.o.fill_style.as_str() {
        "zigzag" => zigzag::fill_polygons(polygon_list, o),
        "cross-hatch" => hatch::fill_polygons(polygon_list, o),
        "dots" => todo!(),
        "dashed" => todo!(),
        "zigzag-line" => todo!(),
        _ => hachure::fill_polygons(polygon_list, o),
    }
}
