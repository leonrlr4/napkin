//! Excalidraw's file format and element rules at commit afa3a653fc5d2b742adcbd5a6063187b056d2419.
//! No egui, no rendering: shape output is data (spec §4.2).

pub mod collision;
pub mod color;
pub mod edit;
pub mod element;
pub mod env;
pub mod file;
pub mod fractional_index;
pub mod geometry;
pub mod history;
pub mod json;
mod laser_pointer;
pub mod new_element;
mod perfect_freehand;
pub mod sample;
pub mod selection;
pub mod shape;
pub mod transform;

pub use crate::element::{Element, Placement};
pub use crate::file::SceneFile;
