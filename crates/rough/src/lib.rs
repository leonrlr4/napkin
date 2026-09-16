//! Line-by-line port of roughjs@4.6.4 (`bin/*.js` in the npm package) and the dependency
//! versions Excalidraw's yarn.lock resolves for it. Baselines in `tests/baseline/` come from
//! `tools/baseline/rough/generate.mjs`.
//!
//! # Input precondition
//!
//! Every geometry and option value passed to `RoughGenerator` must be a finite number.
//! rough.js itself does not handle NaN: a hachure-filled rectangle with a NaN width, for
//! example, exhausts the process's memory (confirmed by running roughjs directly under a
//! capped heap). This port does not reproduce that failure the same way, because Rust's
//! `f64::min`/`f64::max` return the finite operand when the other is NaN, while JS's
//! `Math.min`/`Math.max` propagate NaN; the NaN that would corrupt rough.js's scanline sort
//! gets laundered into a finite value somewhere upstream in the port instead. Callers must
//! not rely on either behavior: pass only finite numbers.
//!
//! # Inputs that never terminate
//!
//! These inputs do not return in rough.js, and the port mirrors the same loops, so they do
//! not return here either. Callers (Excalidraw's shape rules in M2) must not pass them:
//!
//! - `fillStyle: "zigzag-line"` with `zigzagOffset: 0`: `zigzagLines`' segment count is
//!   `Math.round(length / (2 * zigzagOffset))`, which is `Infinity` for any line of nonzero
//!   length, and `for (let i = 0; i < count; i++)` never reaches it.
//! - `fillStyle: "dashed"` with `dashOffset + dashGap == 0` (i.e. both `0`, since a negative
//!   value falls back to a default instead of being used literally): `dashedLine`'s segment
//!   count is `Math.floor(length / (offset + gap))`, `Infinity` for the same reason.
//! - `arc` with `start == stop`: `_arc`'s scan increment is
//!   `Math.min(ellipseInc / 2, (stop - start) / 2)`, which is exactly `0`, and
//!   `for (let angle = radOffset; angle <= stop; angle += increment)` then never advances.
//!   This one is not guaranteed to hang on every call: `radOffset` is `start` plus a random
//!   roughness-scaled offset, and the loop is skipped entirely when that offset happens to
//!   be positive. It always hangs when `roughness == 0` (the offset is then exactly `0`),
//!   and empirically hangs for most seeds otherwise; there is no way to pass `start == stop`
//!   and be sure of a return.

pub mod core;
mod fillers;
pub mod generator;
mod geometry;
pub mod hachure_fill;
pub mod js;
pub mod math;
pub mod path_data;
pub mod points_on_curve;
pub mod points_on_path;
mod renderer;

pub use crate::core::{Drawable, Op, OpSet, OpSetType, Options, Point, ResolvedOptions, Shape};
pub use crate::generator::RoughGenerator;
