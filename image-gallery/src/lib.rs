//! A folder image gallery drawn with egui, with batch processing.
//!
//! The gallery browses one chosen folder: it shows previews of every picture
//! it can read, a large viewer, and the file's properties. Commands act on the
//! selection, one file or many: format conversion, quarter turns and flips
//! (lossless for JPEG through the EXIF orientation), resizing, cropping,
//! canvas changes, colour adjustments, metadata removal, renaming by a
//! template, copying and moving.
//!
//! The crate is split into a UI-free core and the widget:
//!
//! - [`decode`] reads every supported format into an [`image::DynamicImage`]
//!   together with its ICC profile and EXIF chunk;
//! - [`ops`] holds the operations of a recipe, [`encode`] the writers;
//! - [`batch`] runs a [`batch::Recipe`] over many files on worker threads,
//!   with progress and cancellation;
//! - [`catalog`] lists a folder, [`naming`] expands file name templates;
//! - [`Gallery`] is the embeddable widget: the host calls
//!   [`Gallery::show`] (or the ribbon and the content separately) and answers
//!   the [`GalleryEvent`]s it returns, such as a request for a folder dialog.
//!
//! The core is pure Rust: no native image libraries are linked.

pub mod batch;
pub mod catalog;
pub mod decode;
pub mod encode;
mod error;
pub mod exif;
pub mod formats;
pub mod gallery;
pub mod locale;
mod lossless;
pub mod naming;
pub mod ops;
mod psd;
mod raw;
#[cfg(feature = "svg")]
mod svg;
mod thumbs;
mod tiff;

pub use error::Error;
pub use formats::Format;
pub use gallery::{Command, Gallery, GalleryEvent, Settings};
pub use locale::Locale;
