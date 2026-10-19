//! Layered raster editing with optional embedded Plakat inference and a diagnostic CLI adapter.
//! Editing and project persistence work offline without an inference runtime.

pub mod assets;
pub mod civitai;
mod document;
pub mod generation;
pub mod model_tools;
pub mod ui;

pub use document::{Adjustment, Document, EditError, History, Layer, Raster, Tool};
