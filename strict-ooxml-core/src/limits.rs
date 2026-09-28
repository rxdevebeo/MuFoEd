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
    /// Default: 200.
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
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_zip_entries: 4096,
            max_compressed_input: 512 * 1024 * 1024,
            max_total_uncompressed: 512 * 1024 * 1024,
            max_single_uncompressed: 128 * 1024 * 1024,
            max_compression_ratio: 200,
            max_xml_depth: 256,
            max_xml_attributes_per_elem: 1024,
            max_text_len: 64 * 1024 * 1024,
            max_rel_depth: 32,
            max_parts: 4096,
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
        assert_eq!(limits.max_compression_ratio, 200);
        assert_eq!(limits.max_xml_depth, 256);
        assert_eq!(limits.max_xml_attributes_per_elem, 1024);
        assert_eq!(limits.max_text_len, 64 * 1024 * 1024);
        assert_eq!(limits.max_rel_depth, 32);
        assert_eq!(limits.max_parts, 4096);
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
