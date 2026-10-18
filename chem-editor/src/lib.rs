//! A visual chemistry editor drawn with egui: 2D structures, molecular
//! formulas and reaction equations.
//!
//! The crate is a self-contained widget. [`editor::ChemEditor`] owns a
//! [`model::ChemDocument`]; the host calls [`ChemEditor::show`] from any panel
//! and persists the document as JSON, SVG or DOCX. The core does not depend on
//! the OOXML library; the optional `docx` feature adds an adapter that writes
//! through `StrictLib`'s public model and shared writer.

#![warn(missing_docs)]

pub mod element;
pub mod geometry;
pub mod locale;
pub mod mhchem;
pub mod model;
pub mod render;

pub mod editor;

mod formula;
mod icons;
mod svg;
mod widget;

#[cfg(feature = "docx")]
mod docx;

#[cfg(feature = "docx")]
pub use docx::{DocxExport, DocxImport, DRAWING_GROUP};

#[cfg(feature = "raster")]
mod png;

pub use editor::{AlignEdge, Axis, ChemEditor, Mode, Target, Tool};
pub use locale::Locale;
pub use model::{ChemDocument, ChemObject};
pub use render::{Align, Prim, Theme};
