//! Layered raster editing with optional embedded Plakat inference and a diagnostic CLI adapter.
//! Editing and project persistence work offline without an inference runtime.

pub mod assets;
pub mod civitai;
mod document;
pub mod generation;
pub mod huggingface;
pub mod model_tools;
mod profiling;
pub mod ui;

pub use document::recovery;
pub use document::{
    Adjustment, BrushStroke, BrushStyle, CanvasMode, ColorCorrection, Curves, Document, EditError,
    Histogram, History, Layer, LayerBlendMode, LayerMask, LevelChannel, Levels, MaskTarget,
    PhotoGeometry, PixelClipboard, Raster, RetouchMode, RetouchStroke, SelectionOp,
    SelectionRefine, Shape, ToneAdjustment, ToneCurve, Tool,
};
