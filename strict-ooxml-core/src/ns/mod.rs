//! Namespace registry and conformance detection.
//!
//! Maps Strict ↔ Transitional namespace URIs and relationship types, and decides
//! whether a package is `Strict`, `Transitional`, `Mixed` or `Unknown`
//! (`TZ-STRICT-OOXML-RUST.md` §4.1, §9.3, §9.4).

pub mod detect;
pub mod registry;

/// Conformance of a package with respect to the Strict standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Conformance {
    /// All key namespaces belong to the Strict (`purl.oclc.org/ooxml/...`) family.
    Strict,
    /// Key namespaces belong to the Transitional
    /// (`schemas.openxmlformats.org/...`) family.
    Transitional,
    /// Both families were observed in the same package.
    Mixed,
    /// No conclusive signal was found.
    Unknown,
}
