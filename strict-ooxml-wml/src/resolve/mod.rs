//! Resolve phase: wire styles, numbering and relationships into the document
//! (STAGE-2 §4.4).

pub mod numbering;
pub mod rels;
pub mod styles;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::Package;

use crate::model::Document;
use crate::parse::ParseOptions;

/// Resolves cross-references in a freshly parsed document.
///
/// Fills style `basedOn` chains and numbering validation, and records any
/// integrity problems in the support model.
///
/// # Errors
///
/// Currently infallible; the `Result` is kept for forward compatibility.
pub fn resolve(document: &mut Document, package: &Package, _options: &ParseOptions) -> Result<()> {
    styles::resolve_styles(document);
    numbering::resolve_numbering(document);
    rels::validate_references(document, package);
    Ok(())
}
