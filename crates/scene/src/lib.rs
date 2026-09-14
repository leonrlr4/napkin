//! Excalidraw's file format and element rules at commit afa3a653fc5d2b742adcbd5a6063187b056d2419.
//! No egui, no rendering: shape output is data (spec §4.2).

pub mod color;
pub mod element;
pub mod env;
pub mod file;
pub mod fractional_index;
pub mod json;
pub mod new_element;

pub use crate::element::Element;
pub use crate::file::SceneFile;
