//! DjVu reading: bounded IFF framing, directory resolution and native text zones.
//! Optional `djvu-image` decodes page images without invoking external programs.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // Delta fields are at most 24 bits; geometry is bounded by 16-bit page dimensions.
    clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::many_single_char_names,
    // Keep byte-level layout and validation adjacent.
    clippy::too_many_lines, clippy::items_after_statements,
    clippy::doc_markdown
)]
mod annotations;
mod container;
mod document;
#[cfg(feature = "djvu-image")]
mod image;
#[cfg(feature = "djvu-image")]
pub use image::PageImage;
mod text;
pub use annotations::{Annotation, AnnotationTarget};
pub use container::Chunk;
pub use document::{DjvuDocument, DjvuPage, PageInfo};
pub use text::{Rect, TextLayer, TextZone, ZoneKind};

/// Failure to read a DjVu document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DjvuError {
    /// Framing, encoding or a required field is invalid.
    #[error("invalid DjVu: {0}")]
    Invalid(String),
    /// A budget was exceeded before further work.
    #[error("DjVu limit exceeded: {0}")]
    Limit(&'static str),
    /// An indirect component could not be supplied.
    #[error("DjVu component could not be resolved: {0}")]
    Component(String),
    /// Image decoding was refused or failed.
    #[error("DjVu image unavailable: {0}")]
    Image(String),
}
/// Resource budgets. These apply independently of the source file's declarations.
#[derive(Debug, Clone, Copy)]
pub struct DjvuLimits {
    /// Maximum pages.
    pub max_pages: usize,
    /// Maximum chunk headers across all component files.
    pub max_chunks: usize,
    /// Maximum payload bytes per chunk.
    pub max_chunk_bytes: usize,
    /// Maximum bytes across the document and all resolved components.
    pub max_file_bytes: usize,
    /// Maximum decompressed BZZ block, including its sentinel.
    pub max_bzz_block: usize,
    /// Maximum text-layer bytes per page after decompression.
    pub max_text_bytes: usize,
    /// Maximum text zones per page.
    pub max_zones: usize,
    /// Maximum cumulative UTF-8 bytes copied into all zones of a page.
    pub max_zone_text_bytes: usize,
    /// Maximum nesting for text, containers and includes.
    pub max_zone_depth: usize,
    /// Maximum directory entries.
    pub max_component_files: usize,
    /// Maximum pixels in a page or image plane.
    pub max_ink_pixels: usize,
    /// Maximum JB2 records/symbols decoded per stream.
    pub max_sjbz_symbols: usize,
}
impl Default for DjvuLimits {
    fn default() -> Self {
        Self {
            max_pages: 2048,
            max_chunks: 2_000_000,
            max_chunk_bytes: 512 * 1024 * 1024,
            max_file_bytes: 2 * 1024 * 1024 * 1024,
            max_bzz_block: 4 * 1024 * 1024,
            max_text_bytes: 64 * 1024 * 1024,
            max_zones: 500_000,
            max_zone_text_bytes: 256 * 1024 * 1024,
            max_zone_depth: 16,
            max_component_files: 4096,
            max_ink_pixels: 16 * 1024 * 1024,
            max_sjbz_symbols: 4_194_304,
        }
    }
}
pub(crate) fn invalid(detail: impl Into<String>) -> DjvuError {
    DjvuError::Invalid(detail.into())
}
pub(crate) fn bounded(value: usize, max: usize, name: &'static str) -> Result<(), DjvuError> {
    if value > max {
        Err(DjvuError::Limit(name))
    } else {
        Ok(())
    }
}
pub(crate) fn decompress(
    bytes: &[u8],
    max: usize,
    limits: DjvuLimits,
) -> Result<Vec<u8>, DjvuError> {
    djvu_bzz::bzz_decode_with_limits(bytes, limits.max_bzz_block, max)
        .map_err(|e| invalid(format!("BZZ: {e}")))
}
