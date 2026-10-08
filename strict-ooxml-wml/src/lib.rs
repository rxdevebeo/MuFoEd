//! WordprocessingML Strict document model and parser.
//!
//! `strict-ooxml-wml` turns the parts of a Strict package (`document.xml`,
//! `styles.xml`, `numbering.xml`, `settings.xml` and the relationships that
//! link them) into a typed, immutable [`model::Document`]. It depends only on
//! [`strict_ooxml_core`] (ADR-0004).
//!
//! The parsing pipeline has two phases (STAGE-2-TASK §4.4):
//!
//! 1. **parse** — [`parse::parse_document`] walks the parts with the streaming
//!    [`strict_ooxml_core::xml::XmlReader`] and builds raw typed structures;
//! 2. **resolve** — [`resolve::resolve`] wires styles, numbering and
//!    relationship targets into the final immutable document.
//!
//! The entry point is [`parse_document`]; see [`ParseOptions`].
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_core::opc::{OpenOptions, Package};
//! use strict_ooxml_wml::{parse_document, ParseOptions};
//!
//! let package = Package::open_path("document.docx", &OpenOptions::default())?;
//! let document = parse_document(&package, &ParseOptions::default())?;
//! println!("blocks: {}", document.body.blocks.len());
//! # Ok::<(), strict_ooxml_core::error::StrictError>(())
//! ```
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
// The DOM is intentionally made of wide, flag-heavy property structs and
// non-boxed enum variants so that consumers index a flat model; size trade-offs
// are documented in ADR-0004.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::large_enum_variant,
    clippy::type_complexity
)]

pub mod model;
pub mod nesting;
pub mod parse;
pub mod resolve;

pub use model::Document;
pub use parse::{parse_document, ParseOptions};

/// Default XML namespace of the WordprocessingML Strict main schema.
pub const WML_STRICT_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

/// Strict namespace of the DrawingML main schema.
pub const DRAWINGML_STRICT_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";

/// Strict namespace of the DrawingML word-processing drawing schema.
pub const WORDPROCESSING_DRAWING_STRICT_NS: &str =
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";

/// Strict namespace of the DrawingML picture schema.
pub const PICTURE_STRICT_NS: &str = "http://purl.oclc.org/ooxml/drawingml/picture";

/// Strict namespace of the DrawingML word-processing shape schema.
pub const WORD_PROCESSING_SHAPE_STRICT_NS: &str =
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingShape";

/// Strict namespace of the DrawingML word-processing group schema.
pub const WORD_PROCESSING_GROUP_STRICT_NS: &str =
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingGroup";

/// Strict namespace of the DrawingML chart schema.
pub const CHART_STRICT_NS: &str = "http://purl.oclc.org/ooxml/drawingml/chart";

/// Strict namespace of the DrawingML diagram (SmartArt) schema.
pub const DIAGRAM_STRICT_NS: &str = "http://purl.oclc.org/ooxml/drawingml/diagram";

/// Strict namespace of the DrawingML locked canvas schema.
pub const LOCKED_CANVAS_STRICT_NS: &str = "http://purl.oclc.org/ooxml/drawingml/lockedCanvas";

/// Transitional namespace of the DrawingML locked canvas schema.
pub const LOCKED_CANVAS_TRANSITIONAL_NS: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas";

/// Microsoft 2010 wordprocessingShape namespace (compatibility for real-world
/// "Strict" producers, recorded as `Partial`).
pub const MS_WORD_PROCESSING_SHAPE_NS: &str =
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";

/// Microsoft 2010 wordprocessingGroup namespace.
pub const MS_WORD_PROCESSING_GROUP_NS: &str =
    "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";

/// Microsoft 2010 wordprocessingDrawing namespace (`wp14:sizeRelH` / `sizeRelV`).
pub const MS_WORD_PROCESSING_DRAWING_NS: &str =
    "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing";

/// Microsoft 2006 WordprocessingML namespace (`wne:txbxContent`).
pub const MS_WORD_2006_WML_NS: &str = "http://schemas.microsoft.com/office/word/2006/wordml";

/// Strict namespace of the OPC relationships schema.
pub const RELS_STRICT_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// Strict namespace of the Office Math Markup Language schema
/// (`STAGE-5C-TASK.md` §5.1). OMML is a native part of ISO/IEC 29500-1, so the
/// Strict namespace is the only one accepted for formulas.
pub const MATH_STRICT_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

/// The OMML namespace under the name used by the Stage-5C order (`M_NS`).
pub const M_NS: &str = MATH_STRICT_NS;

/// Namespaces the WML parser treats as understood for `mc:Choice/@Requires`
/// (AUD-50, same selection rule as `McePolicy::ProcessChoice`).
///
/// Prefixes resolve through in-scope `xmlns` declarations; membership is by URI.
pub const SUPPORTED_MCE_NAMESPACES: &[&str] = &[
    MS_WORD_PROCESSING_SHAPE_NS,
    MS_WORD_PROCESSING_GROUP_NS,
    MS_WORD_PROCESSING_DRAWING_NS,
    MATH_STRICT_NS,
];
