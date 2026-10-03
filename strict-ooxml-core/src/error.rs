//! Unified error model for the crate.
//!
//! Hosts [`StrictError`], its [`LimitKind`] discriminator, [`SourceLocation`]
//! and the crate-wide [`Result`] alias (see `TZ-STRICT-OOXML-RUST.md` §13 and
//! stage task S1.2).
//!
//! Invariants required of this module:
//!
//! - no `unwrap`/`expect`/`panic!` on library paths that handle input;
//! - every variant that can carry a location reports one (part + line/column).

use std::fmt;
use std::io;

use thiserror::Error;

use crate::opc::rels::RelId;
use crate::part::PartId;

/// Crate-wide result alias.
pub type Result<T> = core::result::Result<T, StrictError>;

/// Discriminates which [`ResourceLimits`](crate::limits::ResourceLimits) bound
/// was exceeded by a [`StrictError::LimitExceeded`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    /// `ResourceLimits::max_zip_entries`.
    ZipEntries,
    /// `ResourceLimits::max_compressed_input`.
    CompressedInput,
    /// `ResourceLimits::max_total_uncompressed`.
    TotalUncompressed,
    /// `ResourceLimits::max_single_uncompressed`.
    SingleUncompressed,
    /// `ResourceLimits::max_compression_ratio`.
    CompressionRatio,
    /// `ResourceLimits::max_xml_depth`.
    XmlDepth,
    /// `ResourceLimits::max_xml_attributes_per_elem`.
    XmlAttributesPerElement,
    /// `ResourceLimits::max_text_len`.
    TextLen,
    /// `ResourceLimits::max_rel_depth`.
    RelationshipDepth,
    /// `ResourceLimits::max_parts`.
    Parts,
    /// `ResourceLimits::max_block_nesting`.
    ///
    /// Not the same bound as [`XmlDepth`](Self::XmlDepth): this one counts block
    /// containers, of which an XML level holds at most one but a paragraph adds
    /// none, so a document can be well inside 256 XML levels and still overflow
    /// a 1 MiB stack by nesting tables.
    BlockNesting,
    /// `ResourceLimits::max_text_box_nesting`.
    ///
    /// Its own bound because a text box costs ten parser frames where a table
    /// costs one, so the number that fits the stack is a seventh of
    /// [`BlockNesting`](Self::BlockNesting)'s.
    TextBoxNesting,
    /// The per-formula node budget enforced by the OMML parser
    /// (`STAGE-5C-TASK.md` §5.1). Not a `ResourceLimits` field: the bound
    /// applies to one `m:oMath`, not to the whole package.
    MathNodes,
    /// The per-formula nesting budget enforced by the OMML parser
    /// (`STAGE-5C-TASK.md` §5.1).
    MathDepth,
}

impl LimitKind {
    /// Returns a stable, machine-readable name suitable for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZipEntries => "zip_entries",
            Self::CompressedInput => "compressed_input",
            Self::TotalUncompressed => "total_uncompressed",
            Self::SingleUncompressed => "single_uncompressed",
            Self::CompressionRatio => "compression_ratio",
            Self::XmlDepth => "xml_depth",
            Self::XmlAttributesPerElement => "xml_attributes_per_element",
            Self::TextLen => "text_len",
            Self::RelationshipDepth => "relationship_depth",
            Self::Parts => "parts",
            Self::BlockNesting => "block_nesting",
            Self::TextBoxNesting => "text_box_nesting",
            Self::MathNodes => "math_nodes",
            Self::MathDepth => "math_depth",
        }
    }
}

impl fmt::Display for LimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Location of a construct inside a package part.
///
/// `line`, `column` and `byte_offset` are 1-based for line/column and
/// 0-based for the byte offset, matching `TZ-STRICT-OOXML-RUST.md` §7.4.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// Part the location belongs to.
    pub part: PartId,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number.
    pub column: u32,
    /// Byte offset from the start of the part.
    pub byte_offset: u64,
}

impl SourceLocation {
    /// Creates a location from its parts.
    #[must_use]
    pub fn new(part: PartId, line: u32, column: u32, byte_offset: u64) -> Self {
        Self {
            part,
            line,
            column,
            byte_offset,
        }
    }

    /// Creates a location that points at no part (`<unknown>:1:1`).
    ///
    /// Used for synthesised nodes (for example a formula built by a caller
    /// rather than parsed) so model types stay `Default`-constructible.
    #[must_use]
    pub fn unknown() -> Self {
        Self::new(PartId::new(""), 1, 1, 0)
    }
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self::unknown()
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.part, self.line, self.column)
    }
}

/// The single error type returned by every fallible operation in the crate.
///
/// The enum is `#[non_exhaustive]`: new variants may be added without a
/// breaking change. Display messages are in English
/// (`TZ-STRICT-OOXML-RUST.md` §14).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StrictError {
    /// Underlying I/O failure while reading a package or part.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// The bytes are not a well-formed ZIP archive.
    #[error("invalid ZIP structure: {0}")]
    InvalidZip(String),
    /// The entry uses a compression method other than store or deflate.
    #[error("unsupported compression method: {0}")]
    UnsupportedCompression(u16),
    /// A configured resource limit was exceeded.
    #[error("resource limit exceeded ({kind}): limit = {limit}, actual = {actual}")]
    LimitExceeded {
        /// Which limit was exceeded.
        kind: LimitKind,
        /// Configured bound.
        limit: u64,
        /// Observed value.
        actual: u64,
    },
    /// The XML is malformed at the given location.
    #[error("invalid XML at {location}: {detail}")]
    InvalidXml {
        /// Where the problem was found.
        location: SourceLocation,
        /// Human-readable detail.
        detail: String,
    },
    /// An element or attribute uses a namespace prefix that is not in scope.
    #[error("unbound namespace prefix '{prefix}' at {location}")]
    UnboundPrefix {
        /// Where the prefix was used.
        location: SourceLocation,
        /// The offending prefix.
        prefix: String,
    },
    /// Strict and Transitional signals were found in the same package.
    #[error("mixed Strict/Transitional conformance: {detail}")]
    MixedConformance {
        /// Human-readable detail.
        detail: String,
    },
    /// Transitional content was found while `ConformancePolicy::StrictOnly`.
    #[error("Transitional content is not supported at {location}")]
    TransitionalNotSupported {
        /// Location of the first detected non-conformance.
        location: SourceLocation,
    },
    /// A normalization invariant was violated.
    #[error("normalization invariant violated at {location}: {detail}")]
    NormalizationInvariantViolation {
        /// Where the violation was detected.
        location: SourceLocation,
        /// Human-readable detail.
        detail: String,
    },
    /// Two attributes collapsed onto the same qualified name.
    #[error("attribute collision for '{qname}' at {location}")]
    AttributeCollision {
        /// Colliding qualified name.
        qname: String,
        /// Where the collision was detected.
        location: SourceLocation,
    },
    /// An enumerated attribute carried a value outside its Strict domain.
    #[error("invalid value '{value}' for '{qname}' at {location}")]
    InvalidEnumValue {
        /// Attribute qualified name.
        qname: String,
        /// The offending value.
        value: String,
        /// Where the value was found.
        location: SourceLocation,
    },
    /// A referenced package part does not exist.
    #[error("missing package part: {0}")]
    MissingPart(PartId),
    /// A part referenced by a relationship is absent from the package.
    ///
    /// Unlike [`MissingPart`](Self::MissingPart), this carries the location of
    /// the referencing construct so the error is actionable (STAGE-5 §5.1).
    #[error("missing referenced part {part} (referenced at {location})")]
    MissingReferencedPart {
        /// The referenced part that is not present in the package.
        part: PartId,
        /// Location of the referencing construct.
        location: SourceLocation,
    },
    /// A part name is not a safe, canonical OPC part path.
    #[error("invalid part name: {0}")]
    InvalidPartName(String),
    /// The package contains two parts with the same canonical name.
    #[error("duplicate package part: {0}")]
    DuplicatePart(PartId),
    /// A relationship id could not be resolved.
    #[error("unresolved relationship: {0}")]
    UnresolvedRelationship(RelId),
    /// Rendering failed (used from Stage 4 onward).
    #[error("render error: {0}")]
    Render(String),
    /// The requested operation or feature is not implemented yet.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A part could not be serialised: the writer left an element open, or the
    /// document nested deeper than the writer's budget.
    ///
    /// Not `InvalidXml`, which would blame the input: the input parsed, and it
    /// was the walk over the model that failed. Carrying the part matters,
    /// because the writer builds each part independently and the failure names
    /// exactly one of them (`STAGE-10-TASK.md` E34 — this replaces twenty
    /// `.expect("balanced")` sites that turned a reportable failure into a panic).
    #[error("could not serialise {part}: {detail}")]
    Write {
        /// The part being written when the writer gave up.
        part: PartId,
        /// What the writer reported.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{LimitKind, SourceLocation, StrictError};
    use crate::part::PartId;

    #[test]
    fn limit_kind_display_is_stable() {
        assert_eq!(LimitKind::ZipEntries.to_string(), "zip_entries");
        assert_eq!(LimitKind::XmlDepth.to_string(), "xml_depth");
        assert_eq!(LimitKind::Parts.to_string(), "parts");
    }

    #[test]
    fn io_errors_convert_and_keep_a_source() {
        let error: StrictError = io::Error::new(io::ErrorKind::UnexpectedEof, "truncated").into();
        assert!(matches!(error, StrictError::Io(_)));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn errors_display_their_location() {
        let location = SourceLocation::new(PartId::new("/word/document.xml"), 3, 7, 42);
        let error = StrictError::UnboundPrefix {
            location,
            prefix: "w".to_owned(),
        };
        let text = error.to_string();
        assert!(text.contains("/word/document.xml:3:7"), "{text}");
        assert!(text.contains('w'), "{text}");
    }
}
