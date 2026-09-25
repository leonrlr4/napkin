//! Resize handles and pointer-driven resizing (`packages/element/src/transformHandles.ts`,
//! `packages/element/src/resizeTest.ts`, the resize half of
//! `packages/element/src/resizeElements.ts`, `packages/common/src/points.ts`'s
//! `rescalePoints`, and `packages/element/src/textElement.ts`'s `computeBoundTextPosition`
//! and its helpers, all at commit `afa3a653fc5d2b742adcbd5a6063187b056d2419`).
//!
//! napkin only ever draws and hit-tests the four corner squares: it never renders the
//! `n`/`s`/`e`/`w` edge squares Excalidraw shows above a size threshold, and rather than
//! porting the slash/backslash corner omission for a two-point line or arrow, it shows no
//! corner handles at all for that case. Resizing from an edge still works everywhere else
//! through the line-proximity test in [`handle_at`], which mirrors `resizeTest`'s fallback
//! independently of what squares are drawn. Sticky notes, elbow-arrow fixed-point mirroring,
//! image `scale`, and bound-text rewrapping or its text-measured minimum size are all out of
//! scope here (napkin has no sticky note or image element, elbow arrows never gain handles at
//! all, and rewrapping needs the text measurement M5 adds); a resized container's bound text
//! is only repositioned, keeping its own width, height and font size.

use crate::collision::{DEFAULT_TRANSFORM_HANDLE_SPACING, SIDE_RESIZING_THRESHOLD};
use crate::element::{Element, TextElement};
use crate::env::Env;
use crate::file::SceneFile;
use crate::geometry::{
    Bounds, GeometryCache, element_absolute_coords, element_bounds, rotate_point,
};
use crate::new_element::bump_version;
use crate::selection::{Selection, selected_bounds};

/// `BOUND_TEXT_PADDING` (`packages/common/src/constants.ts`).
const BOUND_TEXT_PADDING: f64 = 5.0;
/// `MIN_FONT_SIZE`.
const MIN_FONT_SIZE: f64 = 1.0;
/// `transformHandleSizes.mouse`: napkin has no pen/touch pointer type.
const HANDLE_SIZE: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandleKind {
    N,
    S,
    E,
    W,
    Nw,
    Ne,
    Sw,
    Se,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResizeOptions {
    /// Shift (`shouldMaintainAspectRatio`).
    pub keep_aspect_ratio: bool,
    /// Alt (`shouldResizeFromCenter`).
    pub from_center: bool,
}

/// `getTransformHandlesFromCoords` at angle 0 for a mouse pointer on desktop: the four corner
/// squares in `nw, ne, sw, se` order, minus `omit`, as `[min_x, min_y, max_x, max_y]` rather
/// than JS's `[x, y, width, height]`.
pub fn corner_handles(
    bounds: Bounds,
    zoom: f64,
    margin: f64,
    omit: &[HandleKind],
) -> Vec<(HandleKind, Bounds)> {
    use HandleKind::*;

    let [x1, y1, x2, y2] = bounds;
    let handle = HANDLE_SIZE / zoom;
    let dashed_line_margin = margin / zoom;
    let centering_offset = (HANDLE_SIZE - DEFAULT_TRANSFORM_HANDLE_SPACING * 2.0) / (2.0 * zoom);
    let left = x1 - dashed_line_margin - handle + centering_offset;
    let top = y1 - dashed_line_margin - handle + centering_offset;
    let right = x2 + dashed_line_margin - centering_offset;
    let bottom = y2 + dashed_line_margin - centering_offset;

    [
        (Nw, left, top),
        (Ne, right, top),
        (Sw, left, bottom),
        (Se, right, bottom),
    ]
    .into_iter()
    .filter(|(kind, _, _)| !omit.contains(kind))
    .map(|(kind, x, y)| (kind, [x, y, x + handle, y + handle]))
    .collect()
}

/// Whether `element` alone, as the sole member of a selection, is a two-point line or arrow:
/// napkin shows no handles at all for that case (see the module doc comment) rather than
/// porting `OMIT_SIDES_FOR_LINE_SLASH`/`BACKSLASH`.
fn is_two_point_linear(element: &Element) -> bool {
    matches!(element, Element::Line(l) | Element::Arrow(l) if l.points.len() <= 2)
}

/// The bounds and margin [`corner_handles`] should use for `selection`'s corner squares, or
/// `None` when napkin shows no handles at all: an empty selection, a single two-point line or
/// arrow, or a selection containing a rotated, locked, elbow-arrow or [`Element::Raw`]
/// element (`hasBoundingBox` plus `getTransformHandles`'s own locked/elbow-arrow checks).
fn resizable_bounds(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
) -> Option<(Bounds, f64)> {
    let positions = selection.positions(file);
    if positions.is_empty() {
        return None;
    }
    for &index in &positions {
        let element = &file.elements[index];
        if matches!(element, Element::Raw(_)) {
            return None;
        }
        if element.is_locked() {
            return None;
        }
        if let Element::Arrow(l) = element
            && l.elbowed == Some(true)
        {
            return None;
        }
        if element.placement()?.angle != 0.0 {
            return None;
        }
    }

    if positions.len() == 1 {
        let element = &file.elements[positions[0]];
        if is_two_point_linear(element) {
            return None;
        }
        let margin = if matches!(element, Element::Line(_) | Element::Arrow(_)) {
            DEFAULT_TRANSFORM_HANDLE_SPACING + 8.0
        } else {
            DEFAULT_TRANSFORM_HANDLE_SPACING
        };
        Some((element_absolute_coords(element)?.0, margin))
    } else {
        Some((selected_bounds(geometry, file, selection)?, 4.0))
    }
}

/// The corner handles the selection shows: none for an empty selection, a single two-point
/// line or arrow, or a selection containing a rotated, locked, elbow-arrow or `Raw` element.
pub fn selection_handles(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
    zoom: f64,
) -> Vec<(HandleKind, Bounds)> {
    match resizable_bounds(geometry, file, selection) {
        Some((bounds, margin)) => corner_handles(bounds, zoom, margin, &[]),
        None => Vec::new(),
    }
}

/// Squared-off distance from `point` to the segment `seg` (`distanceToLineSegment`, shared by
/// `resizeTest`'s and `getTransformHandleTypeFromCoords`'s `pointOnLineSegment` edge test).
fn distance_to_segment(point: [f64; 2], seg: [[f64; 2]; 2]) -> f64 {
    let [x1, y1] = seg[0];
    let [x2, y2] = seg[1];
    let a = point[0] - x1;
    let b = point[1] - y1;
    let c = x2 - x1;
    let d = y2 - y1;
    let len_sq = c * c + d * d;
    let t = if len_sq != 0.0 {
        ((a * c + b * d) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [x1 + t * c, y1 + t * d];
    rough::js::hypot(point[0] - q[0], point[1] - q[1])
}

/// `resizeTest` / `getTransformHandleTypeFromCoords`: corner squares first, then the edges of
/// the padded bounds (never `E`/`W` for a single text element, never edges for a single line
/// or arrow with two points).
pub fn handle_at(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
    point: [f64; 2],
    zoom: f64,
) -> Option<HandleKind> {
    let (corner_bounds, margin) = resizable_bounds(geometry, file, selection)?;
    for (kind, bounds) in corner_handles(corner_bounds, zoom, margin, &[]) {
        if point[0] >= bounds[0]
            && point[0] <= bounds[2]
            && point[1] >= bounds[1]
            && point[1] <= bounds[3]
        {
            return Some(kind);
        }
    }

    let positions = selection.positions(file);
    let edge_bounds = if positions.len() == 1 {
        element_absolute_coords(&file.elements[positions[0]])?.0
    } else {
        selected_bounds(geometry, file, selection)?
    };
    let exclude_ew =
        positions.len() == 1 && matches!(file.elements[positions[0]], Element::Text(_));
    let spacing = SIDE_RESIZING_THRESHOLD / zoom;
    let [x1, y1, x2, y2] = edge_bounds;
    let (px1, py1, px2, py2) = (x1 - spacing, y1 - spacing, x2 + spacing, y2 + spacing);
    let sides = [
        (HandleKind::N, [[px1, py1], [px2, py1]]),
        (HandleKind::E, [[px2, py1], [px2, py2]]),
        (HandleKind::S, [[px2, py2], [px1, py2]]),
        (HandleKind::W, [[px1, py2], [px1, py1]]),
    ];
    for (kind, segment) in sides {
        if exclude_ew && matches!(kind, HandleKind::E | HandleKind::W) {
            continue;
        }
        if distance_to_segment(point, segment) < spacing {
            return Some(kind);
        }
    }
    None
}

/// `getResizeOffsetXY`: `point` minus the edge or corner `handle` drags.
pub fn resize_offset(
    geometry: &mut GeometryCache,
    file: &SceneFile,
    selection: &Selection,
    handle: HandleKind,
    point: [f64; 2],
) -> [f64; 2] {
    use HandleKind::*;

    let positions = selection.positions(file);
    let (bounds, angle) = if positions.len() == 1 {
        let element = &file.elements[positions[0]];
        let bounds = element_absolute_coords(element).map_or([0.0; 4], |(b, _)| b);
        (bounds, element.placement().map_or(0.0, |p| p.angle))
    } else {
        (
            selected_bounds(geometry, file, selection).unwrap_or([0.0; 4]),
            0.0,
        )
    };
    let [x1, y1, x2, y2] = bounds;
    let center = [(x1 + x2) / 2.0, (y1 + y2) / 2.0];
    let [x, y] = rotate_point(point, center, -angle);
    let offset = match handle {
        N => [x - (x1 + x2) / 2.0, y - y1],
        S => [x - (x1 + x2) / 2.0, y - y2],
        W => [x - x1, y - (y1 + y2) / 2.0],
        E => [x - x2, y - (y1 + y2) / 2.0],
        Nw => [x - x1, y - y1],
        Ne => [x - x2, y - y1],
        Sw => [x - x1, y - y2],
        Se => [x - x2, y - y2],
    };
    rotate_point(offset, [0.0, 0.0], angle)
}

/// JS `Math.sign`, except `0`/`-0`/`NaN` all map to `0.0` (the only callers, the aspect-ratio
/// branches of [`next_single_width_height`] and `resize_elements`, only ever multiply the
/// result by another factor).
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `normalizeRadians`.
fn normalize_radians(angle: f64) -> f64 {
    const TAU: f64 = std::f64::consts::TAU;
    if angle < 0.0 {
        (angle % TAU) + TAU
    } else {
        angle % TAU
    }
}

/// Whether `handle`'s direction contains `side` (`'n'`, `'s'`, `'e'` or `'w'`), i.e. JS's
/// `transformHandleType.includes(side)`.
fn handle_has(handle: HandleKind, side: char) -> bool {
    use HandleKind::*;
    matches!(
        (handle, side),
        (N, 'n')
            | (S, 's')
            | (E, 'e')
            | (W, 'w')
            | (Nw, 'n')
            | (Nw, 'w')
            | (Ne, 'n')
            | (Ne, 'e')
            | (Sw, 's')
            | (Sw, 'w')
            | (Se, 's')
            | (Se, 'e')
    )
}

/// `getNextSingleWidthAndHeightFromPointer`, specialized to napkin's always-fresh-from-`start`
/// resize (see [`resize_element`]'s doc comment): since the latest and original element are
/// the same snapshot, `boundsCurrentWidth`/`boundsCurrentHeight` always equal
/// `atStartBoundsWidth`/`atStartBoundsHeight`, so the scale defaults to 1 rather than needing
/// a second `getResizedElementAbsoluteCoords` call.
fn next_single_width_height(
    orig_bounds: Bounds,
    orig_width: f64,
    orig_height: f64,
    angle: f64,
    handle: HandleKind,
    pointer: [f64; 2],
    options: ResizeOptions,
) -> (f64, f64) {
    use HandleKind::*;

    let [x1, y1, x2, y2] = orig_bounds;
    let center = [(x1 + x2) / 2.0, (y1 + y2) / 2.0];
    let rotated_pointer = rotate_point(pointer, center, -angle);
    let bounds_width = x2 - x1;
    let bounds_height = y2 - y1;

    let mut scale_x = 1.0;
    let mut scale_y = 1.0;
    if matches!(handle, E | Ne | Se) {
        scale_x = (rotated_pointer[0] - x1) / bounds_width;
    }
    if matches!(handle, S | Sw | Se) {
        scale_y = (rotated_pointer[1] - y1) / bounds_height;
    }
    if matches!(handle, W | Nw | Sw) {
        scale_x = (x2 - rotated_pointer[0]) / bounds_width;
    }
    if matches!(handle, N | Nw | Ne) {
        scale_y = (y2 - rotated_pointer[1]) / bounds_height;
    }

    let mut next_width = orig_width * scale_x;
    let mut next_height = orig_height * scale_y;

    if options.from_center {
        next_width = 2.0 * next_width - orig_width;
        next_height = 2.0 * next_height - orig_height;
    }

    if options.keep_aspect_ratio {
        let width_ratio = next_width.abs() / orig_width;
        let height_ratio = next_height.abs() / orig_height;
        if matches!(handle, N | S | E | W) {
            next_height *= width_ratio;
            next_width *= height_ratio;
        } else {
            let ratio = width_ratio.max(height_ratio);
            next_width = orig_width * ratio * js_sign(next_width);
            next_height = orig_height * ratio * js_sign(next_height);
        }
    }

    (next_width, next_height)
}

/// Where `getResizedOrigin` anchors the unchanged corner/edge/center while the opposite one
/// moves (`ResizeAnchor`/`getResizeAnchor`).
#[derive(Clone, Copy)]
enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
    EastSide,
    WestSide,
    NorthSide,
    SouthSide,
}

/// `getResizeAnchor`.
fn resize_anchor(handle: HandleKind, keep_aspect_ratio: bool, from_center: bool) -> Anchor {
    use HandleKind::*;
    if from_center {
        return Anchor::Center;
    }
    if keep_aspect_ratio {
        return match handle {
            N => Anchor::SouthSide,
            E => Anchor::WestSide,
            S => Anchor::NorthSide,
            W => Anchor::EastSide,
            Ne => Anchor::BottomLeft,
            Nw => Anchor::BottomRight,
            Se => Anchor::TopLeft,
            Sw => Anchor::TopRight,
        };
    }
    match handle {
        E | Se | S => Anchor::TopLeft,
        N | Nw | W => Anchor::BottomRight,
        Ne => Anchor::BottomLeft,
        Sw => Anchor::TopRight,
    }
}

/// `getResizedOrigin`. The trigonometric terms are kept exactly as JS writes them even though
/// every caller in this task only ever passes `angle: 0.0` (see [`resize_element`]'s doc
/// comment): at that angle `cos(angle) == 1.0` and `sin(angle) == 0.0` collapse each branch to
/// the same value a angle-free version would compute.
fn get_resized_origin(
    prev_origin: [f64; 2],
    prev_size: [f64; 2],
    new_size: [f64; 2],
    angle: f64,
    handle: HandleKind,
    keep_aspect_ratio: bool,
    from_center: bool,
) -> [f64; 2] {
    let anchor = resize_anchor(handle, keep_aspect_ratio, from_center);
    let [x, y] = prev_origin;
    let [prev_width, prev_height] = prev_size;
    let [new_width, new_height] = new_size;
    let (cos, sin) = (angle.cos(), angle.sin());
    let dw = prev_width - new_width;
    let dh = prev_height - new_height;

    match anchor {
        Anchor::TopLeft => [
            x + dw / 2.0 + (-dw / 2.0) * cos + (dh / 2.0) * sin,
            y + dh / 2.0 + (-dw / 2.0) * sin + (-dh / 2.0) * cos,
        ],
        Anchor::TopRight => [
            x + (dw / 2.0) * (cos + 1.0) + (dh / 2.0) * sin,
            y + dh / 2.0 + (dw / 2.0) * sin + (-dh / 2.0) * cos,
        ],
        Anchor::BottomLeft => [
            x + (dw / 2.0) * (1.0 - cos) + (-dh / 2.0) * sin,
            y + (dh / 2.0) * (cos + 1.0) + (-dw / 2.0) * sin,
        ],
        Anchor::BottomRight => [
            x + (dw / 2.0) * (cos + 1.0) + (-dh / 2.0) * sin,
            y + (dh / 2.0) * (cos + 1.0) + (dw / 2.0) * sin,
        ],
        Anchor::Center => [
            x - (new_width - prev_width) / 2.0,
            y - (new_height - prev_height) / 2.0,
        ],
        Anchor::EastSide => [
            x + (dw / 2.0) * (cos + 1.0),
            y + (dw / 2.0) * sin + dh / 2.0,
        ],
        Anchor::WestSide => [
            x + (dw / 2.0) * (1.0 - cos),
            y + (-dw / 2.0) * sin + dh / 2.0,
        ],
        Anchor::NorthSide => [
            x + dw / 2.0 + (dh / 2.0) * sin,
            y + (-dh / 2.0) * (cos - 1.0),
        ],
        Anchor::SouthSide => [
            x + dw / 2.0 + (-dh / 2.0) * sin,
            y + (dh / 2.0) * (cos + 1.0),
        ],
    }
}

/// `rescalePoints` for one dimension (`0` for x, `1` for y).
fn rescale_points(
    dimension: usize,
    new_size: f64,
    points: &[[f64; 2]],
    normalize: bool,
) -> Vec<[f64; 2]> {
    let coords = || points.iter().map(|p| p[dimension]);
    let max_c = coords().fold(f64::NEG_INFINITY, f64::max);
    let min_c = coords().fold(f64::INFINITY, f64::min);
    let size = max_c - min_c;
    let scale = if size == 0.0 { 1.0 } else { new_size / size };

    let mut next_min = f64::INFINITY;
    let mut scaled: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            let mut next = *p;
            next[dimension] = p[dimension] * scale;
            next_min = next_min.min(next[dimension]);
            next
        })
        .collect();

    if !normalize || scaled.len() == 2 {
        return scaled;
    }
    let translation = min_c - next_min;
    for p in &mut scaled {
        p[dimension] += translation;
    }
    scaled
}

/// `rescalePointsInElement`'s `points` computation for a line, arrow or freedraw element.
fn rescale_points_pair(
    points: &[[f64; 2]],
    width: f64,
    height: f64,
    normalize: bool,
) -> Vec<[f64; 2]> {
    let scaled_y = rescale_points(1, height, points, normalize);
    rescale_points(0, width, &scaled_y, normalize)
}

/// `measureFontSizeFromWidth`, always in the unbound-container case: every caller in this
/// module only reaches a text element that is not bound to a container (`resize_element`'s
/// target is never a bound text, since [`crate::selection::is_selectable`] excludes them from
/// selection in the first place, and `resize_elements` skips a bound text target before
/// calling this).
fn font_size_for_width(text: &TextElement, target_width: f64) -> Option<f64> {
    let next_font_size = text.font_size * (target_width / text.base.width);
    (next_font_size >= MIN_FONT_SIZE).then_some(next_font_size)
}

/// `resizeSingleElement` / `resizeSingleTextElement` for the element at `position`, from its
/// state in `start`, plus the position of its bound text. Returns whether anything changed.
///
/// `start` is the scene as it stood when the pointer went down; `file` is the scene as it
/// stands now. Every call recomputes the target element from `start`, not from `file`'s
/// current (possibly already-resized) copy: napkin has no "latest vs. original element"
/// distinction to track between pointer-move events, so the geometry this function derives
/// from `start` (`getResizedElementAbsoluteCoords`'s "at start" bounds) already equals what a
/// tracked "latest" element's bounds would be at the start of this call.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors transformElements' pointer-resize contract: geometry cache, before/after scenes, target, handle, pointer and modifier keys are all independently needed"
)]
pub fn resize_element(
    _geometry: &mut GeometryCache,
    file: &mut SceneFile,
    start: &SceneFile,
    position: usize,
    handle: HandleKind,
    pointer: [f64; 2],
    options: ResizeOptions,
    env: &mut impl Env,
) -> bool {
    let orig = &start.elements[position];
    // A `Raw` element's restricted mutation surface has no width/height/points/fontSize, so
    // it never resizes (spec §5.2).
    if matches!(orig, Element::Raw(_)) {
        return false;
    }
    let Some(placement) = orig.placement() else {
        return false;
    };
    let Some((orig_bounds, _)) = element_absolute_coords(orig) else {
        return false;
    };
    let (next_width, next_height) = next_single_width_height(
        orig_bounds,
        placement.width,
        placement.height,
        placement.angle,
        handle,
        pointer,
        options,
    );
    if next_width == 0.0 || next_height == 0.0 {
        return false;
    }

    if let Element::Text(orig_text) = orig {
        if matches!(handle, HandleKind::E | HandleKind::W) {
            return false;
        }
        let metrics_width = orig_text.base.width * (next_height / orig_text.base.height);
        let Some(font_size) = font_size_for_width(orig_text, metrics_width) else {
            return false;
        };
        let prev_origin = [orig_text.base.x, orig_text.base.y];
        let new_origin = get_resized_origin(
            prev_origin,
            [orig_text.base.width, orig_text.base.height],
            [metrics_width, next_height],
            placement.angle,
            handle,
            false,
            options.from_center,
        );

        let mut next = orig.clone();
        let Element::Text(t) = &mut next else {
            unreachable!("matched Element::Text above")
        };
        t.font_size = font_size;
        t.base.width = metrics_width;
        t.base.height = next_height;
        t.base.x = new_origin[0];
        t.base.y = new_origin[1];

        if next == *orig {
            return false;
        }
        file.elements[position] = next;
        bump_version(&mut file.elements[position], env);
        return true;
    }

    let is_line_or_arrow = matches!(orig, Element::Line(_) | Element::Arrow(_));
    let mut rescaled_points = match orig {
        Element::Line(l) | Element::Arrow(l) => Some(rescale_points_pair(
            &l.points,
            next_width,
            next_height,
            true,
        )),
        Element::Freedraw(f) => Some(rescale_points_pair(
            &f.points,
            next_width,
            next_height,
            true,
        )),
        _ => None,
    };

    let mut prev_origin = [placement.x, placement.y];
    if is_line_or_arrow && let Some(bounds) = element_bounds(orig) {
        prev_origin = [bounds[0], bounds[1]];
    }

    let mut new_origin = get_resized_origin(
        prev_origin,
        [placement.width, placement.height],
        [next_width, next_height],
        placement.angle,
        handle,
        options.keep_aspect_ratio,
        options.from_center,
    );

    if is_line_or_arrow && let Some(points) = &mut rescaled_points {
        let offset_x = placement.x - prev_origin[0];
        let offset_y = placement.y - prev_origin[1];
        new_origin[0] += offset_x;
        new_origin[1] += offset_y;
        let (shift_x, shift_y) = (points[0][0], points[0][1]);
        new_origin[0] += shift_x;
        new_origin[1] += shift_y;
        for p in points.iter_mut() {
            p[0] -= shift_x;
            p[1] -= shift_y;
        }
    }

    if next_width < 0.0 {
        new_origin[0] += next_width;
    }
    if next_height < 0.0 {
        new_origin[1] += next_height;
    }

    let mut next = orig.clone();
    match &mut next {
        Element::Rectangle(g) | Element::Diamond(g) | Element::Ellipse(g) => {
            g.base.x = new_origin[0];
            g.base.y = new_origin[1];
            g.base.width = next_width.abs();
            g.base.height = next_height.abs();
        }
        Element::Line(l) | Element::Arrow(l) => {
            l.base.x = new_origin[0];
            l.base.y = new_origin[1];
            l.base.width = next_width.abs();
            l.base.height = next_height.abs();
            if let Some(points) = rescaled_points {
                l.points = points;
            }
        }
        Element::Freedraw(f) => {
            f.base.x = new_origin[0];
            f.base.y = new_origin[1];
            f.base.width = next_width.abs();
            f.base.height = next_height.abs();
            if let Some(points) = rescaled_points {
                f.points = points;
            }
        }
        Element::Text(_) | Element::Raw(_) => unreachable!("handled above"),
    }

    let mut changed = false;
    if next != *orig {
        file.elements[position] = next;
        bump_version(&mut file.elements[position], env);
        changed = true;
    }

    if matches!(
        orig,
        Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
    ) && reposition_bound_text(file, position, env)
    {
        changed = true;
    }

    changed
}

/// `anchorsMap`, shared by `getNextMultipleWidthAndHeightFromPointer` and
/// `resizeMultipleElements`: the point opposite `handle` that stays put while it drags.
fn multi_anchor(
    handle: HandleKind,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    width: f64,
    height: f64,
) -> [f64; 2] {
    use HandleKind::*;
    match handle {
        Ne => [min_x, max_y],
        Se => [min_x, min_y],
        Sw => [max_x, min_y],
        Nw => [max_x, max_y],
        E => [min_x, min_y + height / 2.0],
        W => [max_x, min_y + height / 2.0],
        N => [min_x + width / 2.0, max_y],
        S => [min_x + width / 2.0, min_y],
    }
}

/// `flipConditionsMap`: whether the pointer has crossed the anchor in x and/or y.
fn multi_flip(handle: HandleKind, pointer: [f64; 2], anchor: [f64; 2]) -> (bool, bool) {
    use HandleKind::*;
    let [px, py] = pointer;
    let [ax, ay] = anchor;
    match handle {
        Ne => (px < ax, py > ay),
        Se => (px < ax, py < ay),
        Sw => (px > ax, py < ay),
        Nw => (px > ax, py > ay),
        E => (px < ax, false),
        W => (px > ax, false),
        N => (false, py > ay),
        S => (false, py < ay),
    }
}

/// `resizeMultipleElements` for `targets` (ascending positions), from their state in `start`.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors transformElements' pointer-resize contract: geometry cache, before/after scenes, targets, handle, pointer and modifier keys are all independently needed"
)]
pub fn resize_elements(
    geometry: &mut GeometryCache,
    file: &mut SceneFile,
    start: &SceneFile,
    targets: &[usize],
    handle: HandleKind,
    pointer: [f64; 2],
    options: ResizeOptions,
    env: &mut impl Env,
) -> bool {
    if targets.is_empty() {
        return false;
    }
    if let [only] = targets {
        return resize_element(geometry, file, start, *only, handle, pointer, options, env);
    }

    let Some([min_x, min_y, max_x, max_y]) =
        geometry.common_bounds(targets.iter().map(|&i| &start.elements[i]))
    else {
        return false;
    };
    let width = max_x - min_x;
    let height = max_y - min_y;

    let default_anchor = multi_anchor(handle, min_x, min_y, max_x, max_y, width, height);
    let anchor = if options.from_center {
        [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0]
    } else {
        default_anchor
    };
    let resize_from_center_scale = if options.from_center { 2.0 } else { 1.0 };

    let mut next_width = if handle_has(handle, 'e') || handle_has(handle, 'w') {
        (pointer[0] - anchor[0]).abs() * resize_from_center_scale
    } else {
        width
    };
    let mut next_height = if handle_has(handle, 'n') || handle_has(handle, 's') {
        (pointer[1] - anchor[1]).abs() * resize_from_center_scale
    } else {
        height
    };

    if options.keep_aspect_ratio {
        let scale = ((pointer[0] - anchor[0]).abs() / width)
            .max((pointer[1] - anchor[1]).abs() / height)
            * resize_from_center_scale;
        next_width = width * scale * js_sign(pointer[0] - anchor[0]);
        next_height = height * scale * js_sign(pointer[1] - anchor[1]);
    }

    let (flip_x, flip_y) = multi_flip(handle, pointer, anchor);

    if next_width == 0.0 || next_height == 0.0 {
        return false;
    }

    let mut scale_x = if handle_has(handle, 'e') || handle_has(handle, 'w') {
        next_width.abs() / width
    } else {
        1.0
    };
    let mut scale_y = if handle_has(handle, 'n') || handle_has(handle, 's') {
        next_height.abs() / height
    } else {
        1.0
    };
    let scale = if matches!(
        handle,
        HandleKind::N | HandleKind::S | HandleKind::E | HandleKind::W
    ) {
        if handle_has(handle, 'e') || handle_has(handle, 'w') {
            scale_x
        } else {
            scale_y
        }
    } else {
        (next_width.abs() / width).max(next_height.abs() / height)
    };

    let keep_aspect_ratio = options.keep_aspect_ratio
        || targets
            .iter()
            .any(|&i| matches!(start.elements[i], Element::Text(_)))
        || targets
            .iter()
            .any(|&i| !start.elements[i].group_ids().is_empty());
    if keep_aspect_ratio {
        scale_x = scale;
        scale_y = scale;
    }

    let flip_factor_x = if flip_x { -1.0 } else { 1.0 };
    let flip_factor_y = if flip_y { -1.0 } else { 1.0 };

    // Compute every target's next state before applying any of them: a bound text whose
    // scaled font size would drop below MIN_FONT_SIZE aborts the whole operation, matching
    // `resizeMultipleElements` building its full `elementsAndUpdates` list before its
    // separate mutation pass.
    let mut next_elements: Vec<(usize, Element)> = Vec::with_capacity(targets.len());
    for &position in targets {
        let orig = &start.elements[position];
        // Bound text is resized along with its container, not as its own target; `Raw`'s
        // restricted mutation surface has no width/height/points/fontSize (spec §5.2).
        if orig.container_id().is_some() || matches!(orig, Element::Raw(_)) {
            continue;
        }
        let Some(placement) = orig.placement() else {
            continue;
        };

        let new_width = placement.width * scale_x;
        let new_height = placement.height * scale_y;
        let new_angle = normalize_radians(placement.angle * flip_factor_x * flip_factor_y);

        let is_linear_or_freedraw = matches!(
            orig,
            Element::Line(_) | Element::Arrow(_) | Element::Freedraw(_)
        );
        let offset_x = placement.x - anchor[0];
        let offset_y = placement.y - anchor[1];
        let shift_x = if flip_x && !is_linear_or_freedraw {
            new_width
        } else {
            0.0
        };
        let shift_y = if flip_y && !is_linear_or_freedraw {
            new_height
        } else {
            0.0
        };
        let new_x = anchor[0] + flip_factor_x * (offset_x * scale_x + shift_x);
        let new_y = anchor[1] + flip_factor_y * (offset_y * scale_y + shift_y);

        let rescaled_points = match orig {
            Element::Line(l) | Element::Arrow(l) => Some(rescale_points_pair(
                &l.points,
                new_width * flip_factor_x,
                new_height * flip_factor_y,
                false,
            )),
            Element::Freedraw(f) => Some(rescale_points_pair(
                &f.points,
                new_width * flip_factor_x,
                new_height * flip_factor_y,
                false,
            )),
            _ => None,
        };

        let text_font_size = if let Element::Text(orig_text) = orig {
            match font_size_for_width(orig_text, new_width) {
                Some(size) => Some(size),
                None => return false,
            }
        } else {
            None
        };

        let mut next = orig.clone();
        match &mut next {
            Element::Rectangle(g) | Element::Diamond(g) | Element::Ellipse(g) => {
                g.base.x = new_x;
                g.base.y = new_y;
                g.base.width = new_width;
                g.base.height = new_height;
                g.base.angle = new_angle;
            }
            Element::Line(l) | Element::Arrow(l) => {
                l.base.x = new_x;
                l.base.y = new_y;
                l.base.width = new_width;
                l.base.height = new_height;
                l.base.angle = new_angle;
                if let Some(points) = rescaled_points {
                    l.points = points;
                }
            }
            Element::Freedraw(f) => {
                f.base.x = new_x;
                f.base.y = new_y;
                f.base.width = new_width;
                f.base.height = new_height;
                f.base.angle = new_angle;
                if let Some(points) = rescaled_points {
                    f.points = points;
                }
            }
            Element::Text(t) => {
                t.base.x = new_x;
                t.base.y = new_y;
                t.base.width = new_width;
                t.base.height = new_height;
                t.base.angle = new_angle;
                t.font_size = text_font_size.expect("computed above for a text orig element");
            }
            Element::Raw(_) => unreachable!("Raw targets are skipped above"),
        }
        next_elements.push((position, next));
    }

    let mut changed = false;
    for (position, next) in next_elements {
        if next != start.elements[position] {
            file.elements[position] = next;
            bump_version(&mut file.elements[position], env);
            changed = true;
        } else {
            file.elements[position] = next;
        }
    }

    for &position in targets {
        if matches!(
            file.elements[position],
            Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
        ) && reposition_bound_text(file, position, env)
        {
            changed = true;
        }
    }

    changed
}

/// `computeBoundTextPosition` for a rectangle, diamond or ellipse container; `None` otherwise
/// (an arrow container's label follows `LinearElementEditor.getBoundTextElementPosition`
/// instead, out of scope here).
pub fn bound_text_position(container: &Element, text: &TextElement) -> Option<[f64; 2]> {
    if !matches!(
        container,
        Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_)
    ) {
        return None;
    }
    let placement = container.placement()?;

    // `getContainerCoords`.
    let (offset_x, offset_y) = match container {
        Element::Diamond(_) => (placement.width / 4.0, placement.height / 4.0),
        Element::Ellipse(_) => {
            let k = 1.0 - std::f64::consts::FRAC_1_SQRT_2;
            (placement.width / 2.0 * k, placement.height / 2.0 * k)
        }
        _ => (0.0, 0.0),
    };
    let container_x = placement.x + BOUND_TEXT_PADDING + offset_x;
    let container_y = placement.y + BOUND_TEXT_PADDING + offset_y;

    // `getBoundTextMaxWidth`/`getBoundTextMaxHeight`.
    let max_width = match container {
        Element::Diamond(_) => {
            rough::js::math_round(placement.width / 2.0) - BOUND_TEXT_PADDING * 2.0
        }
        Element::Ellipse(_) => {
            rough::js::math_round(placement.width / 2.0 * std::f64::consts::SQRT_2)
                - BOUND_TEXT_PADDING * 2.0
        }
        _ => placement.width - BOUND_TEXT_PADDING * 2.0,
    };
    let max_height = match container {
        Element::Diamond(_) => {
            rough::js::math_round(placement.height / 2.0) - BOUND_TEXT_PADDING * 2.0
        }
        Element::Ellipse(_) => {
            rough::js::math_round(placement.height / 2.0 * std::f64::consts::SQRT_2)
                - BOUND_TEXT_PADDING * 2.0
        }
        _ => placement.height - BOUND_TEXT_PADDING * 2.0,
    };

    let y = match text.vertical_align.as_str() {
        "top" => container_y,
        "bottom" => container_y + (max_height - text.base.height),
        _ => container_y + (max_height / 2.0 - text.base.height / 2.0),
    };
    let x = match text.text_align.as_str() {
        "left" => container_x,
        "right" => container_x + (max_width - text.base.width),
        _ => container_x + (max_width / 2.0 - text.base.width / 2.0),
    };

    if placement.angle != 0.0 {
        let content_center = [
            container_x + max_width / 2.0,
            container_y + max_height / 2.0,
        ];
        let text_center = [x + text.base.width / 2.0, y + text.base.height / 2.0];
        let [rx, ry] = rotate_point(text_center, content_center, placement.angle);
        return Some([rx - text.base.width / 2.0, ry - text.base.height / 2.0]);
    }
    Some([x, y])
}

/// Repositions `container_position`'s bound text (if it has one, and it is not deleted) with
/// [`bound_text_position`], rounding through `Math.round`. Returns whether the text moved.
fn reposition_bound_text(
    file: &mut SceneFile,
    container_position: usize,
    env: &mut impl Env,
) -> bool {
    let container = file.elements[container_position].clone();
    let Some((text_id, _)) = container
        .bound_elements()
        .into_iter()
        .find(|&(_, kind)| kind == "text")
    else {
        return false;
    };
    let text_id = text_id.to_string();
    let Some(text_position) = file
        .elements
        .iter()
        .position(|e| !e.is_deleted() && e.id() == Some(text_id.as_str()))
    else {
        return false;
    };
    let Element::Text(text) = &file.elements[text_position] else {
        return false;
    };
    let Some([x, y]) = bound_text_position(&container, text) else {
        return false;
    };
    let (new_x, new_y) = (rough::js::math_round(x), rough::js::math_round(y));

    let Element::Text(t) = &mut file.elements[text_position] else {
        unreachable!("checked above")
    };
    if t.base.x == new_x && t.base.y == new_y {
        return false;
    }
    t.base.x = new_x;
    t.base.y = new_y;
    bump_version(&mut file.elements[text_position], env);
    true
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::sample;

    struct TestEnv;

    impl Env for TestEnv {
        fn fill_random(&mut self, bytes: &mut [u8]) {
            bytes.fill(7);
        }

        fn now_ms(&mut self) -> f64 {
            42.0
        }
    }

    const PLAIN: ResizeOptions = ResizeOptions {
        keep_aspect_ratio: false,
        from_center: false,
    };

    fn resize(
        values: Vec<Value>,
        targets: &[usize],
        handle: HandleKind,
        pointer: [f64; 2],
        options: ResizeOptions,
    ) -> SceneFile {
        let start = sample::file(values);
        let mut file = start.clone();
        let mut geometry = GeometryCache::default();
        if let [position] = targets {
            resize_element(
                &mut geometry,
                &mut file,
                &start,
                *position,
                handle,
                pointer,
                options,
                &mut TestEnv,
            );
        } else {
            resize_elements(
                &mut geometry,
                &mut file,
                &start,
                targets,
                handle,
                pointer,
                options,
                &mut TestEnv,
            );
        }
        file
    }

    fn rect_of(file: &SceneFile, position: usize) -> [f64; 4] {
        let p = file.elements[position].placement().expect("placement");
        [p.x, p.y, p.width, p.height]
    }

    fn assert_rect(actual: [f64; 4], expected: [f64; 4]) {
        for i in 0..4 {
            assert!(
                (actual[i] - expected[i]).abs() < 1e-9,
                "{actual:?} != {expected:?}"
            );
        }
    }

    fn rect(id: &str, r: [f64; 4]) -> Value {
        sample::generic("rectangle", id, r)
    }

    #[test]
    fn corner_handles_follow_transform_handle_geometry() {
        use HandleKind::*;
        assert_eq!(
            corner_handles([0.0, 0.0, 100.0, 100.0], 1.0, 2.0, &[]),
            vec![
                (Nw, [-8.0, -8.0, 0.0, 0.0]),
                (Ne, [100.0, -8.0, 108.0, 0.0]),
                (Sw, [-8.0, 100.0, 0.0, 108.0]),
                (Se, [100.0, 100.0, 108.0, 108.0]),
            ]
        );
        let zoomed = corner_handles([0.0, 0.0, 100.0, 100.0], 2.0, 2.0, &[]);
        assert_eq!(
            (zoomed[0], zoomed[3]),
            (
                (Nw, [-4.0, -4.0, 0.0, 0.0]),
                (Se, [100.0, 100.0, 104.0, 104.0])
            )
        );
        assert_eq!(
            corner_handles([0.0, 0.0, 100.0, 100.0], 1.0, 10.0, &[])[0],
            (Nw, [-16.0, -16.0, -8.0, -8.0])
        );
        let slash: Vec<HandleKind> = corner_handles([0.0, 0.0, 1.0, 1.0], 1.0, 2.0, &[Nw, Se])
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(slash, vec![Ne, Sw]);
    }

    #[test]
    fn which_selections_have_handles_and_where_they_hit() {
        use HandleKind::*;
        let file = sample::file(vec![
            rect("r", [0.0, 0.0, 100.0, 100.0]),
            sample::with(
                rect("rotated", [0.0, 0.0, 10.0, 10.0]),
                json!({"angle": 0.3}),
            ),
            sample::linear("arrow", "a", [0.0, 0.0], &[[0.0, 0.0], [50.0, 50.0]]),
            json!({"id": "i", "type": "image", "x": 0, "y": 0, "width": 10, "height": 10, "angle": 0, "version": 1, "versionNonce": 1}),
            sample::text("t", [300.0, 0.0, 100.0, 25.0], "hi", None),
            rect("s", [200.0, 0.0, 50.0, 50.0]),
        ]);
        let mut g = GeometryCache::default();
        let sel = |ids: &[&str]| Selection::from_ids(ids.iter().copied());

        assert_eq!(selection_handles(&mut g, &file, &sel(&["r"]), 1.0).len(), 4);
        for ids in [
            &["rotated"][..],
            &["a"][..],
            &["i"][..],
            &["r", "i"][..],
            &[][..],
        ] {
            assert!(
                selection_handles(&mut g, &file, &sel(ids), 1.0).is_empty(),
                "{ids:?}"
            );
            assert_eq!(
                handle_at(&mut g, &file, &sel(ids), [104.0, 104.0], 1.0),
                None,
                "{ids:?}"
            );
        }

        let r = sel(&["r"]);
        assert_eq!(handle_at(&mut g, &file, &r, [104.0, 104.0], 1.0), Some(Se));
        assert_eq!(handle_at(&mut g, &file, &r, [102.0, -2.0], 1.0), Some(Ne));
        assert_eq!(handle_at(&mut g, &file, &r, [50.0, -3.0], 1.0), Some(N));
        assert_eq!(handle_at(&mut g, &file, &r, [-4.0, 50.0], 1.0), Some(W));
        assert_eq!(handle_at(&mut g, &file, &r, [50.0, 1.0], 1.0), None);

        let t = sel(&["t"]);
        assert_eq!(handle_at(&mut g, &file, &t, [404.0, 12.0], 1.0), None);
        assert_eq!(handle_at(&mut g, &file, &t, [350.0, 29.0], 1.0), Some(S));

        // Multiple elements: common bounds [0, 0, 250, 100] with the default margin of 4.
        let multi = selection_handles(&mut g, &file, &sel(&["r", "s"]), 1.0);
        assert_eq!(multi[0], (Nw, [-10.0, -10.0, -2.0, -2.0]));
        assert_eq!(
            resize_offset(&mut g, &file, &r, Se, [104.0, 103.0]),
            [4.0, 3.0]
        );
    }

    #[test]
    fn single_generic_resizes_from_the_opposite_corner() {
        use HandleKind::*;
        let r = || vec![rect("r", [0.0, 0.0, 100.0, 50.0])];
        assert_rect(
            rect_of(&resize(r(), &[0], Se, [150.0, 100.0], PLAIN), 0),
            [0.0, 0.0, 150.0, 100.0],
        );
        let aspect = ResizeOptions {
            keep_aspect_ratio: true,
            from_center: false,
        };
        assert_rect(
            rect_of(&resize(r(), &[0], Se, [150.0, 100.0], aspect), 0),
            [0.0, 0.0, 200.0, 100.0],
        );
        let center = ResizeOptions {
            keep_aspect_ratio: false,
            from_center: true,
        };
        assert_rect(
            rect_of(&resize(r(), &[0], Se, [150.0, 100.0], center), 0),
            [-50.0, -50.0, 200.0, 150.0],
        );
        assert_rect(
            rect_of(&resize(r(), &[0], Se, [-50.0, 100.0], PLAIN), 0),
            [-50.0, 0.0, 50.0, 100.0],
        );
        assert_rect(
            rect_of(&resize(r(), &[0], Nw, [20.0, 10.0], PLAIN), 0),
            [20.0, 10.0, 80.0, 40.0],
        );
        assert_rect(
            rect_of(&resize(r(), &[0], E, [130.0, 999.0], PLAIN), 0),
            [0.0, 0.0, 130.0, 50.0],
        );

        let resized = resize(r(), &[0], Se, [150.0, 100.0], PLAIN);
        assert_eq!(resized.elements[0].version(), 4.0);
        let untouched = resize(r(), &[0], Se, [100.0, 50.0], PLAIN);
        assert_eq!(untouched.elements[0].version(), 3.0);
    }

    #[test]
    fn lines_scale_points_and_text_scales_font_size() {
        use HandleKind::*;
        let line = sample::with(
            sample::linear(
                "line",
                "l",
                [0.0, 0.0],
                &[[0.0, 0.0], [50.0, 50.0], [100.0, 0.0]],
            ),
            json!({"roughness": 0, "roundness": null}),
        );
        let file = resize(vec![line], &[0], Se, [200.0, 100.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 200.0, 100.0]);
        assert_eq!(
            file.elements[0].to_value()["points"],
            json!([[0.0, 0.0], [100.0, 100.0], [200.0, 0.0]])
        );

        let text = || vec![sample::text("t", [0.0, 0.0, 100.0, 25.0], "hi", None)];
        for pointer in [[200.0, 50.0], [300.0, 50.0]] {
            let file = resize(text(), &[0], Se, pointer, PLAIN);
            assert_rect(rect_of(&file, 0), [0.0, 0.0, 200.0, 50.0]);
            assert_eq!(file.elements[0].to_value()["fontSize"], json!(40.0));
        }
        // Below MIN_FONT_SIZE nothing changes.
        let tiny = resize(text(), &[0], Se, [2.0, 1.0], PLAIN);
        assert_eq!(tiny, sample::file(text()));
    }

    #[test]
    fn multiple_elements_scale_about_the_common_bounds() {
        use HandleKind::*;
        let two = || {
            vec![
                rect("a", [0.0, 0.0, 50.0, 50.0]),
                rect("b", [50.0, 50.0, 50.0, 50.0]),
            ]
        };
        let file = resize(two(), &[0, 1], Se, [200.0, 200.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 100.0]);
        assert_rect(rect_of(&file, 1), [100.0, 100.0, 100.0, 100.0]);
        let file = resize(two(), &[0, 1], Se, [200.0, 100.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 50.0]);
        assert_rect(rect_of(&file, 1), [100.0, 50.0, 100.0, 50.0]);

        // A text element in the selection forces a uniform scale.
        let mixed = vec![
            rect("a", [0.0, 0.0, 50.0, 50.0]),
            sample::text("t", [50.0, 50.0, 100.0, 25.0], "hi", None),
        ];
        let file = resize(mixed, &[0, 1], Se, [300.0, 75.0], PLAIN);
        assert_rect(rect_of(&file, 0), [0.0, 0.0, 100.0, 100.0]);
        assert_rect(rect_of(&file, 1), [100.0, 100.0, 200.0, 50.0]);
        assert_eq!(file.elements[1].to_value()["fontSize"], json!(40.0));
    }

    #[test]
    fn bound_text_is_repositioned_without_rewrapping() {
        let centered = |container: &str| {
            sample::with(
                sample::text("t", [30.0, 40.0, 40.0, 20.0], "hi", Some(container)),
                json!({"textAlign": "center", "verticalAlign": "middle"}),
            )
        };
        let container = sample::with(
            rect("r", [0.0, 0.0, 100.0, 100.0]),
            json!({"boundElements": [{"id": "t", "type": "text"}]}),
        );
        let file = resize(
            vec![container, centered("r")],
            &[0],
            HandleKind::Se,
            [200.0, 100.0],
            PLAIN,
        );
        assert_rect(rect_of(&file, 1), [80.0, 40.0, 40.0, 20.0]);

        let Element::Text(text) = Element::from_value(centered("c")) else {
            panic!("text")
        };
        let diamond =
            Element::from_value(sample::generic("diamond", "c", [0.0, 0.0, 200.0, 100.0]));
        assert_eq!(bound_text_position(&diamond, &text), Some([80.0, 40.0]));
        let ellipse =
            Element::from_value(sample::generic("ellipse", "c", [0.0, 0.0, 200.0, 100.0]));
        let [x, y] = bound_text_position(&ellipse, &text).expect("ellipse container");
        assert!(
            (x - 79.789_321_881_345_24).abs() < 1e-9 && (y - 40.144_660_940_672_62).abs() < 1e-9,
            "{x} {y}"
        );
        let line = Element::from_value(sample::linear(
            "line",
            "c",
            [0.0, 0.0],
            &[[0.0, 0.0], [1.0, 1.0]],
        ));
        assert_eq!(bound_text_position(&line, &text), None);
    }
}
