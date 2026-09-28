//! Conformance detection.
//!
//! Combines namespace, relationship-type and content-type signals into a single
//! [`Conformance`] verdict; disagreeing signals yield [`Conformance::Mixed`]
//! rather than a silent choice (`TZ-STRICT-OOXML-RUST.md` §4.1, §9.4; stage
//! task S1.12).

use crate::error::Result;
use crate::ns::registry::{classify_namespace, classify_relationship};
use crate::ns::Conformance;
use crate::opc::content_types::ContentTypeIndex;

/// Signals gathered from a package, used to detect conformance.
#[derive(Debug, Default)]
pub struct ConformanceSignals<'a> {
    /// Namespace URIs of the key parts (`document.xml`, `styles.xml`, ...).
    pub namespaces: Vec<&'a str>,
    /// Raw relationship-type URIs found in the package.
    pub relationship_types: Vec<&'a str>,
    /// Content-type index of the package.
    pub content_types: Option<&'a ContentTypeIndex>,
}

/// Detects conformance from a set of signals.
///
/// # Errors
///
/// Currently infallible; the `Result` is kept for forward compatibility with
/// stricter validation.
pub fn detect_conformance(signals: &ConformanceSignals<'_>) -> Result<Conformance> {
    let mut see_strict = false;
    let mut see_transitional = false;

    for ns in &signals.namespaces {
        match classify_namespace(ns) {
            Some(Conformance::Strict) => see_strict = true,
            Some(Conformance::Transitional) => see_transitional = true,
            _ => {}
        }
    }
    for rel in &signals.relationship_types {
        match classify_relationship(rel) {
            Some(Conformance::Strict) => see_strict = true,
            Some(Conformance::Transitional) => see_transitional = true,
            _ => {}
        }
    }
    if let Some(index) = signals.content_types {
        match classify_content_types(index) {
            Some(Conformance::Strict) => see_strict = true,
            Some(Conformance::Transitional) => see_transitional = true,
            _ => {}
        }
    }

    let verdict = match (see_strict, see_transitional) {
        (true, true) => Conformance::Mixed,
        (true, false) => Conformance::Strict,
        (false, true) => Conformance::Transitional,
        (false, false) => Conformance::Unknown,
    };
    Ok(verdict)
}

/// Classifies a content-type index by scanning declared type strings.
fn classify_content_types(index: &ContentTypeIndex) -> Option<Conformance> {
    let mut strict = false;
    let mut transitional = false;
    for content_type in index.iter_content_types() {
        if content_type.contains("purl.oclc.org/ooxml") {
            strict = true;
        } else if content_type.contains("schemas.openxmlformats.org") {
            transitional = true;
        }
    }
    match (strict, transitional) {
        (true, false) => Some(Conformance::Strict),
        (false, true) => Some(Conformance::Transitional),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_conformance, ConformanceSignals};
    use crate::ns::Conformance;

    #[test]
    fn detects_strict_and_transitional() {
        let strict = ConformanceSignals {
            namespaces: vec!["http://purl.oclc.org/ooxml/wordprocessingml/main"],
            relationship_types: vec![
                "http://purl.oclc.org/ooxml/officeDocument/relationships/styles",
            ],
            content_types: None,
        };
        assert_eq!(detect_conformance(&strict).unwrap(), Conformance::Strict);

        let transitional = ConformanceSignals {
            namespaces: vec!["http://schemas.openxmlformats.org/wordprocessingml/2006/main"],
            relationship_types: vec![
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            ],
            content_types: None,
        };
        assert_eq!(
            detect_conformance(&transitional).unwrap(),
            Conformance::Transitional
        );
    }

    #[test]
    fn detects_mixed() {
        let signals = ConformanceSignals {
            namespaces: vec![
                "http://purl.oclc.org/ooxml/wordprocessingml/main",
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            ],
            relationship_types: vec![],
            content_types: None,
        };
        assert_eq!(detect_conformance(&signals).unwrap(), Conformance::Mixed);
    }

    #[test]
    fn detects_unknown_without_signals() {
        let signals = ConformanceSignals::default();
        assert_eq!(detect_conformance(&signals).unwrap(), Conformance::Unknown);
    }
}
