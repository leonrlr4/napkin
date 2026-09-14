//! Line-by-line port of roughjs@4.6.4 (`bin/*.js` in the npm package) and the dependency
//! versions Excalidraw's yarn.lock resolves for it. Baselines in `tests/baseline/` come from
//! `tools/baseline/rough/generate.mjs`.

pub mod js;
pub mod math;
pub mod path_data;
