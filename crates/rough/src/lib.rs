//! Line-by-line port of roughjs@4.6.4 (`bin/*.js` in the npm package) and the dependency
//! versions Excalidraw's yarn.lock resolves for it. Baselines in `tests/baseline/` come from
//! `tools/baseline/rough/generate.mjs`.

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
