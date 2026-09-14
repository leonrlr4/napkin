//! `bin/geometry.js`.

use crate::core::Point;

/// `bin/geometry.js` `lineLength`.
pub(crate) fn line_length(line: &[Point; 2]) -> f64 {
    let p1 = line[0];
    let p2 = line[1];
    ((p1[0] - p2[0]).powf(2.0) + (p1[1] - p2[1]).powf(2.0)).sqrt()
}
