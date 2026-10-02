//! Resource limits for hostile and damaged input.
//!
//! Hosts [`ResourceLimits`] with the safe defaults from
//! `TZ-STRICT-OOXML-RUST.md` §12.1. Every field is public so callers can
//! override individual limits through `OpenOptions` (functional-update syntax),
//! for example:
//!
//! ```
//! use strict_ooxml_core::limits::ResourceLimits;
//!
//! let limits = ResourceLimits {
//!     max_xml_depth: 32,
//!     ..ResourceLimits::default()
//! };
//! assert_eq!(limits.max_xml_depth, 32);
//! ```

/// Tunable resource budget applied while reading a package.
///
/// The defaults are safe for untrusted input; a limit is checked *before* the
/// corresponding allocation or work is performed. Exceeding any limit yields
/// `StrictError::LimitExceeded` and never a panic.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum number of entries in the ZIP central directory. Default: 4096.
    pub max_zip_entries: usize,
    /// Maximum size of the compressed input archive, in bytes. Bounds the
    /// allocation performed when a package is read into memory. Default: 512 MiB.
    pub max_compressed_input: u64,
    /// Maximum total uncompressed size across all entries, in bytes.
    /// Default: 512 MiB.
    pub max_total_uncompressed: u64,
    /// Maximum uncompressed size of a single part, in bytes. Default: 128 MiB.
    pub max_single_uncompressed: u64,
    /// Maximum allowed uncompressed-to-compressed ratio for one entry.
    /// Default: 1000.
    ///
    /// This is a **secondary** heuristic behind the absolute size limits
    /// ([`max_single_uncompressed`](Self::max_single_uncompressed),
    /// [`max_total_uncompressed`](Self::max_total_uncompressed)): deflate can
    /// legitimately exceed 200:1 on highly repetitive XML (the theoretical
    /// maximum is ≈1032:1), so a low threshold produces false rejections on
    /// real documents (REWORK-CORE-1 C-1). The default is close to that
    /// theoretical ceiling, so it only catches true bombs that stay below the
    /// absolute limits.
    pub max_compression_ratio: u32,
    /// Maximum XML element nesting depth. Default: 256.
    pub max_xml_depth: u32,
    /// Maximum number of attributes on a single XML element. Default: 1024.
    pub max_xml_attributes_per_elem: u32,
    /// Maximum length of a text node within one part, in bytes.
    /// Default: 64 MiB.
    pub max_text_len: usize,
    /// Maximum relationship resolution depth between parts. Default: 32.
    pub max_rel_depth: u32,
    /// Maximum number of accessible package parts. Default: 4096.
    pub max_parts: usize,
    /// Maximum nesting of block containers. Default: 12.
    ///
    /// A *block container* is an element whose children are themselves blocks:
    /// `w:tbl`, a block-level `w:sdt`, `w:customXml`, `w:txbxContent`, a
    /// `w:footnote`/`w:endnote`/`w:comment` body, and `w:hdr`/`w:ftr`.
    ///
    /// This is not [`max_xml_depth`](Self::max_xml_depth) and it exists because
    /// 256 is far past what the stack can take: a table cell is roughly seven
    /// XML elements deep, so 256 XML levels is about 36 nested tables - which
    /// overflowed a 1 MiB stack, and 1 MiB is the main thread on Windows. Word
    /// itself draws a nested-table hierarchy as a flat one past a couple of
    /// levels, so no real document is refused by 12.
    ///
    /// Exceeding it is `LimitKind::BlockNesting` and refuses the whole document:
    /// a document nested that deeply is hostile input, not a document with a
    /// difficult corner.
    pub max_block_nesting: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_zip_entries: 4096,
            max_compressed_input: 512 * 1024 * 1024,
            max_total_uncompressed: 512 * 1024 * 1024,
            max_single_uncompressed: 128 * 1024 * 1024,
            max_compression_ratio: 1000,
            max_xml_depth: 256,
            max_xml_attributes_per_elem: 1024,
            max_text_len: 64 * 1024 * 1024,
            max_rel_depth: 32,
            max_parts: 4096,
            max_block_nesting: 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResourceLimits;

    #[test]
    fn defaults_match_the_specification() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_zip_entries, 4096);
        assert_eq!(limits.max_compressed_input, 512 * 1024 * 1024);
        assert_eq!(limits.max_total_uncompressed, 512 * 1024 * 1024);
        assert_eq!(limits.max_single_uncompressed, 128 * 1024 * 1024);
        assert_eq!(limits.max_compression_ratio, 1000);
        assert_eq!(limits.max_xml_depth, 256);
        assert_eq!(limits.max_xml_attributes_per_elem, 1024);
        assert_eq!(limits.max_text_len, 64 * 1024 * 1024);
        assert_eq!(limits.max_rel_depth, 32);
        assert_eq!(limits.max_parts, 4096);
        assert_eq!(limits.max_block_nesting, 12);
    }

    #[test]
    fn individual_limits_are_overridable() {
        let limits = ResourceLimits {
            max_xml_depth: 8,
            ..ResourceLimits::default()
        };
        assert_eq!(limits.max_xml_depth, 8);
        assert_eq!(limits.max_text_len, 64 * 1024 * 1024);
    }
}
