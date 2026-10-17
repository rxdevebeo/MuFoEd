//! A visual editor for Presentation `MathML`, drawn with egui.
//!
//! `MuFoEd`'s window is an egui application (`eframe`). This crate is the same
//! toolkit, so the editor is a widget: the host calls [`MathEditor::show`]
//! inside whatever panel it already has. The formula comes and goes as
//! `MathML` text. `MuFoEd` already turns that text into its document model, so
//! this crate does not depend on the OOXML library.

#![warn(missing_docs)]

mod edit;
mod icons;
mod layout;
mod mathml;
mod model;
mod text;
mod widget;

pub use mathml::MathmlError;
pub use text::Text;
pub use widget::{MathEditor, Style};
