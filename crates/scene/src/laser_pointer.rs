//! Port of `@excalidraw/laser-pointer` (`packages/laser-pointer/src/{state,math}.ts`),
//! restricted to the shape `createLaserPointer` builds: `simplify: 0`, `keepHead: false`
//! and `simplifyPhase` at its default (`"output"`) always.
//!
//! `simplify: 0` means the `douglasPeucker` branches in `stabilizeTail` and
//! `getStrokeOutline` (`simplifyPhase: "tail"`/`"output"`/`"input"`) never run; `simplify.ts`
//! is therefore not ported, and [`Options`] has no `simplify`/`simplify_phase` fields so
//! those branches cannot be reached by construction. `keepHead` is never set by
//! `createLaserPointer` either (always `false`), and by the same reasoning [`Options`] has
//! no `keep_head` field: `getStrokeOutline`'s two `if (this.options.keepHead) { ... }`
//! branches are unreachable and not ported.

/// `[x, y, r]`; `r` carries whatever `sizeMapping` reads (pressure, pinned to `1` by
/// `getConstantWidthFreedrawOutline`).
type Point = [f64; 3];

const CORNER_DETECTION_MAX_ANGLE_DEG: f64 = 75.0;
const MAX_TAIL_LENGTH: f64 = 50.0;

fn corner_detection_variance(speed: f64) -> f64 {
    if speed > 35.0 { 0.5 } else { 1.0 }
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn smul(p: Point, s: f64) -> Point {
    [p[0] * s, p[1] * s, p[2] * s]
}

fn norm(p: Point) -> Point {
    let len = (p[0] * p[0] + p[1] * p[1]).sqrt();
    [p[0] / len, p[1] / len, p[2]]
}

/// `f64::sin`/`f64::cos` (libm) can disagree with V8's `Math.sin`/`Math.cos` in the last
/// bit (node's V8 build here uses `third_party/glibc`'s large-table implementation, not the
/// portable fdlibm one `js::atan2` is ported from — see that function's doc comment and
/// `crates/rough/src/js.rs`'s module docs); not ported, since porting it means porting that
/// table-heavy glibc code. A divergence here is not always a harmless ~1-ULP nudge to a
/// rotated point: `norm_angle`'s `c_angle`/`t_angle` (below) feed `get_stroke_outline`'s
/// `theta <= t_angle` corner loops, and a last-bit difference there can add or drop an
/// iteration, the same class of bug the `atan2`/`hypot` port fixed.
fn rot(p: Point, rad: f64) -> Point {
    let (s, c) = (rad.sin(), rad.cos());
    [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]]
}

fn plerp(a: Point, b: Point, t: f64) -> Point {
    add(a, smul(sub(b, a), t))
}

fn angle(p: Point, p1: Point, p2: Point) -> f64 {
    rough::js::atan2(p2[1] - p[1], p2[0] - p[0]) - rough::js::atan2(p1[1] - p[1], p1[0] - p[0])
}

/// `sin`/`cos` are `f64`'s own (see [`rot`]'s doc comment: a divergence here can change a
/// loop bound, not just nudge a point); `js::atan2` is V8-exact.
fn norm_angle(a: f64) -> f64 {
    rough::js::atan2(a.sin(), a.cos())
}

fn mag(p: Point) -> f64 {
    (p[0] * p[0] + p[1] * p[1]).sqrt()
}

/// `dist`: plain Euclidean distance (`math.ts`'s own, distinct from perfect-freehand's
/// `vec.ts` version of the same name).
fn dist(a: Point, b: Point) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    (dx * dx + dy * dy).sqrt()
}

fn run_length(ps: &[Point]) -> f64 {
    if ps.len() < 2 {
        return 0.0;
    }
    let mut len = 0.0;
    for i in 1..ps.len() {
        len += dist(ps[i - 1], ps[i]);
    }
    len += dist(ps[ps.len() - 2], ps[ps.len() - 1]);
    len
}

/// `LaserPointerOptions`, minus `simplify`/`simplifyPhase` (see module docs), `keepHead`
/// (`createLaserPointer` never sets it, so it is always `false`; both of `getStrokeOutline`'s
/// `keepHead` branches are unreachable and, like `simplify`, not ported), and
/// `sizeMapping`'s unused `runningLength`/`currentIndex`/`totalLength` fields:
/// `createLaserPointer`'s `sizeMapping` (`(details) => Math.max(0.1, details.pressure)`) only
/// reads `pressure`.
pub(crate) struct Options {
    pub size: f64,
    pub streamline: f64,
    pub size_mapping: fn(f64) -> f64,
}

/// `LaserPointer`. `getStrokeOutline`'s `sizeOverride` parameter and `close()` are dropped:
/// `shape.ts` never passes the former and never calls the latter, always calling
/// `getStrokeOutline()` with no argument. `originalPoints` is kept (`original_points`
/// below): `add_point` reads its last entry to drop a repeated point before appending.
pub(crate) struct LaserPointer {
    options: Options,
    original_points: Vec<Point>,
    stable_points: Vec<Point>,
    tail_points: Vec<Point>,
    is_fresh: bool,
}

impl LaserPointer {
    pub(crate) fn new(options: Options) -> Self {
        LaserPointer {
            options,
            original_points: Vec::new(),
            stable_points: Vec::new(),
            tail_points: Vec::new(),
            is_fresh: true,
        }
    }

    fn last_point(&self) -> Point {
        *self.tail_points.last().unwrap_or_else(|| {
            self.stable_points
                .last()
                .expect("addPoint seeds stablePoints first")
        })
    }

    pub(crate) fn add_point(&mut self, point: Point) {
        if let Some(last) = self.original_points.last()
            && last[0] == point[0]
            && last[1] == point[1]
        {
            return;
        }

        self.original_points.push(point);

        if self.is_fresh {
            self.is_fresh = false;
            self.stable_points.push(point);
            return;
        }

        let point = if self.options.streamline > 0.0 {
            plerp(self.last_point(), point, 1.0 - self.options.streamline)
        } else {
            point
        };

        self.tail_points.push(point);

        if run_length(&self.tail_points) > MAX_TAIL_LENGTH {
            self.stabilize_tail();
        }
    }

    fn stabilize_tail(&mut self) {
        self.stable_points.append(&mut self.tail_points);
    }

    fn get_size(&self, pressure: f64) -> f64 {
        self.options.size * (self.options.size_mapping)(pressure)
    }

    /// `getStrokeOutline`.
    pub(crate) fn get_stroke_outline(&self) -> Vec<Point> {
        if self.is_fresh {
            return Vec::new();
        }

        let points: Vec<Point> = self
            .stable_points
            .iter()
            .chain(self.tail_points.iter())
            .copied()
            .collect();

        let len = points.len();
        if len == 0 {
            return Vec::new();
        }

        if len == 1 {
            let c = points[0];
            let size = self.get_size(c[2]);
            if size < 0.5 {
                return Vec::new();
            }
            let mut ps = Vec::new();
            let mut theta = 0.0;
            while theta <= std::f64::consts::PI * 2.0 {
                ps.push(add(c, smul(rot([1.0, 0.0, 0.0], theta), size)));
                theta += std::f64::consts::PI / 16.0;
            }
            ps.push(add(c, smul([1.0, 0.0, 0.0], self.get_size(c[2]))));
            return ps;
        }

        if len == 2 {
            let c = points[0];
            let n = points[1];
            let c_size = self.get_size(c[2]);
            let n_size = self.get_size(n[2]);
            if c_size < 0.5 || n_size < 0.5 {
                return Vec::new();
            }
            let mut ps = Vec::new();
            let p_angle = angle(c, [c[0], c[1] - 100.0, c[2]], n);

            let mut theta = p_angle;
            while theta <= std::f64::consts::PI + p_angle {
                ps.push(add(c, smul(rot([1.0, 0.0, 0.0], theta), c_size)));
                theta += std::f64::consts::PI / 16.0;
            }
            let mut theta = std::f64::consts::PI + p_angle;
            while theta <= std::f64::consts::PI * 2.0 + p_angle {
                ps.push(add(n, smul(rot([1.0, 0.0, 0.0], theta), n_size)));
                theta += std::f64::consts::PI / 16.0;
            }
            // `ps.push(ps[0])`: both loops above run zero times when `p_angle` is `NaN`
            // (any `theta <= ...` comparison against `NaN` is false), which happens for
            // some extreme inputs (`plerp`'s `sub` can overflow to infinity, and
            // `infinity * 0.0` is `NaN` — see `add_point`'s `streamline` transform). The
            // source's `ps.push(ps[0])` then pushes `undefined` into `ps` (JS `Array.prototype
            // .push` never rejects a value) and the caller's later destructuring of that
            // element throws; napkin returns the (here, empty) outline as-is instead of
            // panicking on the out-of-bounds index `ps[0]` would be.
            if let Some(&first) = ps.first() {
                ps.push(first);
            }
            return ps;
        }

        let mut forward_points: Vec<Point> = Vec::new();
        let mut backward_points: Vec<Point> = Vec::new();

        let mut prev_speed = 0.0;

        let mut visible_start_index = 0usize;
        // The source also threads a `runningLength` accumulator through this loop, for
        // `getSize`'s `details.runningLength`; dropped along with the rest of the generic
        // `sizeMapping` signature (see the `Options` docs) since `createLaserPointer`'s
        // `sizeMapping` never reads it.

        for i in 1..len - 1 {
            let p = points[i - 1];
            let c = points[i];
            let n = points[i + 1];

            let pressure = c[2];

            let d = dist(p, c);
            let speed = prev_speed + (d - prev_speed) * 0.2;

            let c_size = self.get_size(pressure);

            if c_size == 0.0 {
                visible_start_index = i + 1;
                continue;
            }

            let dir_pc = norm(sub(p, c));
            let dir_nc = norm(sub(n, c));
            let p1_dir_pc = rot(dir_pc, std::f64::consts::PI / 2.0);
            let p2_dir_pc = rot(dir_pc, -std::f64::consts::PI / 2.0);
            let p1_dir_nc = rot(dir_nc, std::f64::consts::PI / 2.0);
            let p2_dir_nc = rot(dir_nc, -std::f64::consts::PI / 2.0);

            let p1_pc = add(c, smul(p1_dir_pc, c_size));
            let p2_pc = add(c, smul(p2_dir_pc, c_size));
            let p1_nc = add(c, smul(p1_dir_nc, c_size));
            let p2_nc = add(c, smul(p2_dir_nc, c_size));

            let ft_dir = add(p1_dir_pc, p2_dir_nc);
            let bt_dir = add(p2_dir_pc, p1_dir_nc);

            let pa_pc = add(
                c,
                smul(
                    if mag(ft_dir) == 0.0 {
                        dir_pc
                    } else {
                        norm(ft_dir)
                    },
                    c_size,
                ),
            );
            let pa_nc = add(
                c,
                smul(
                    if mag(bt_dir) == 0.0 {
                        dir_nc
                    } else {
                        norm(bt_dir)
                    },
                    c_size,
                ),
            );

            let c_angle = norm_angle(angle(c, p, n));
            let d_angle = (CORNER_DETECTION_MAX_ANGLE_DEG / 180.0)
                * std::f64::consts::PI
                * corner_detection_variance(speed);

            if c_angle.abs() < d_angle {
                let t_angle = norm_angle(std::f64::consts::PI - c_angle).abs();

                if t_angle == 0.0 {
                    continue;
                }

                if c_angle < 0.0 {
                    backward_points.push(p2_pc);
                    backward_points.push(pa_nc);

                    let mut theta = 0.0;
                    while theta <= t_angle {
                        forward_points.push(add(c, rot(smul(p1_dir_pc, c_size), theta)));
                        theta += t_angle / 4.0;
                    }
                    let mut theta = t_angle;
                    while theta >= 0.0 {
                        backward_points.push(add(c, rot(smul(p1_dir_pc, c_size), theta)));
                        theta -= t_angle / 4.0;
                    }

                    backward_points.push(pa_nc);
                    backward_points.push(p1_nc);
                } else {
                    forward_points.push(p1_pc);
                    forward_points.push(pa_pc);

                    let mut theta = 0.0;
                    while theta <= t_angle {
                        backward_points.push(add(c, rot(smul(p1_dir_pc, -c_size), -theta)));
                        theta += t_angle / 4.0;
                    }
                    let mut theta = t_angle;
                    while theta >= 0.0 {
                        forward_points.push(add(c, rot(smul(p1_dir_pc, -c_size), -theta)));
                        theta -= t_angle / 4.0;
                    }
                    forward_points.push(pa_pc);
                    forward_points.push(p2_nc);
                }
            } else {
                forward_points.push(pa_pc);
                backward_points.push(pa_nc);
            }

            prev_speed = speed;
        }

        // `keepHead` is always `false` (see the `Options` doc comment): the source's
        // `if (this.options.keepHead) { ... }` branch here never runs.
        if visible_start_index >= len - 2 {
            return Vec::new();
        }

        let first = points[visible_start_index];
        let second = points[visible_start_index + 1];
        let penultimate = points[len - 2];
        let ultimate = points[len - 1];

        let dir_fs = norm(sub(second, first));
        let dir_pu = norm(sub(penultimate, ultimate));

        let pp_dir_fs = rot(dir_fs, -std::f64::consts::PI / 2.0);
        let pp_dir_pu = rot(dir_pu, std::f64::consts::PI / 2.0);

        let start_cap_size = self.get_size(first[2]);
        // `keepHead` is always `false`: `end_cap_size` is always `getSize(...)`, never
        // `this.options.size`.
        let end_cap_size = self.get_size(penultimate[2]);

        // Lowered threshold to 0.1, ensuring virtually all strokes get proper rounded caps
        // for visual consistency.
        let mut start_cap: Vec<Point> = Vec::new();
        if start_cap_size > 0.1 {
            let mut theta = 0.0;
            while theta <= std::f64::consts::PI {
                start_cap.insert(0, add(first, rot(smul(pp_dir_fs, start_cap_size), -theta)));
                theta += std::f64::consts::PI / 16.0;
            }
            start_cap.insert(0, add(first, smul(pp_dir_fs, -start_cap_size)));
        } else {
            start_cap.push(first);
        }

        let mut end_cap: Vec<Point> = Vec::new();
        let mut theta = 0.0;
        while theta <= std::f64::consts::PI * 3.0 {
            end_cap.push(add(ultimate, rot(smul(pp_dir_pu, -end_cap_size), -theta)));
            theta += std::f64::consts::PI / 16.0;
        }

        let mut stroke_outline: Vec<Point> = Vec::new();
        stroke_outline.extend(start_cap.iter().copied());
        stroke_outline.extend(forward_points);
        stroke_outline.extend(end_cap.into_iter().rev());
        stroke_outline.extend(backward_points.into_iter().rev());

        if let Some(&first_start_cap) = start_cap.first() {
            stroke_outline.push(first_start_cap);
        }

        stroke_outline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `addPoint`'s `streamline` transform (`plerp`) computes `sub(point, lastPoint)`
    /// (`B - A`), which overflows to infinity for two points this far apart; `infinity *
    /// 0.0` (the `1 - streamline` scale factor here) is `NaN`, which propagates into
    /// `p_angle`, and both of `getStrokeOutline`'s `len == 2` loops (guarded by
    /// `theta <= ... + p_angle`) then run zero times. The source's `ps.push(ps[0])` still
    /// succeeds on the resulting empty `ps` (pushing `undefined`); this must not panic on
    /// the equivalent `ps[0]` indexing.
    #[test]
    fn get_stroke_outline_does_not_panic_on_nan_p_angle() {
        let mut pointer = LaserPointer::new(Options {
            size: 2.8,
            streamline: 1.0,
            size_mapping: |pressure| pressure.max(0.1),
        });
        pointer.add_point([-1e308, 0.0, 1.0]);
        pointer.add_point([1e308, 0.0, 1.0]);
        assert_eq!(pointer.get_stroke_outline(), Vec::<Point>::new());
    }
}
