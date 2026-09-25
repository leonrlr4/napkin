//! napkin's eframe application: CLI parsing, file loading, the omarchy theme, the camera, the
//! scene editor's input routing and overlay, and the egui app with its wgpu canvas
//! (tessellation, text and the render pipelines).

pub mod autosave;
pub mod bench;
pub mod camera;
pub mod cli;
pub mod edit_input;
pub mod fixture;
pub mod input;
pub mod napkin_app;
pub mod overlay;
pub mod pinch;
pub mod render;
pub mod stats;
pub mod storage;
pub mod theme;
pub mod writer;
