//! Strict ↔ Transitional registry tables.
//!
//! Data-only description of the namespaces from `TZ-STRICT-OOXML-RUST.md`
//! §9.3, each entry carrying its [`ScopeSupport`] and `verified` flag (stage
//! task S1.11).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::ns::Conformance;

/// How well the scope of an entry is supported by the toolkit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScopeSupport {
    /// Fully in scope.
    Full,
    /// Partially in scope.
    Partial,
    /// Out of scope.
    None,
    /// In scope as metadata only.
    Metadata,
    /// Optional.
    Optional,
}

/// A single namespace-registry entry.
#[derive(Clone, Copy, Debug)]
pub struct NamespaceEntry {
    /// Stable key, for example `wordprocessingml.main`.
    pub key: &'static str,
    /// Strict URI, if the namespace exists in Strict.
    pub strict: Option<&'static str>,
    /// Transitional URI, if the namespace exists in Transitional.
    pub transitional: Option<&'static str>,
    /// Support status of this namespace within the toolkit.
    pub in_scope: ScopeSupport,
    /// Whether the entry has been confirmed against official schemas.
    pub verified: bool,
}

const NS: [NamespaceEntry; 17] = [
    NamespaceEntry {
        key: "wordprocessingml.main",
        strict: Some("http://purl.oclc.org/ooxml/wordprocessingml/main"),
        transitional: Some("http://schemas.openxmlformats.org/wordprocessingml/2006/main"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "officeDocument.relationships",
        strict: Some("http://purl.oclc.org/ooxml/officeDocument/relationships"),
        transitional: Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.main",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/main"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/main"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.wordprocessingDrawing",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing"),
        transitional: Some(
            "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        ),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.picture",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/picture"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/picture"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.chart",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/chart"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/chart"),
        in_scope: ScopeSupport::Partial,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.diagram",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/diagram"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/diagram"),
        in_scope: ScopeSupport::Partial,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.spreadsheetDrawing",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"),
        in_scope: ScopeSupport::None,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.chartDrawing",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/chartDrawing"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/chartDrawing"),
        in_scope: ScopeSupport::None,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.lockedCanvas",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/lockedCanvas"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas"),
        in_scope: ScopeSupport::None,
        verified: true,
    },
    NamespaceEntry {
        key: "drawingml.compatibility",
        strict: Some("http://purl.oclc.org/ooxml/drawingml/compatibility"),
        transitional: Some("http://schemas.openxmlformats.org/drawingml/2006/compatibility"),
        in_scope: ScopeSupport::None,
        verified: true,
    },
    NamespaceEntry {
        key: "officeDocument.extendedProperties",
        strict: Some("http://purl.oclc.org/ooxml/officeDocument/extendedProperties"),
        transitional: Some(
            "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        ),
        in_scope: ScopeSupport::Metadata,
        verified: true,
    },
    NamespaceEntry {
        key: "officeDocument.customProperties",
        strict: Some("http://purl.oclc.org/ooxml/officeDocument/customProperties"),
        transitional: Some(
            "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
        ),
        in_scope: ScopeSupport::Optional,
        verified: true,
    },
    NamespaceEntry {
        key: "package.metadata.coreProperties",
        strict: Some("http://purl.oclc.org/ooxml/package/metadata/coreProperties"),
        transitional: Some(
            "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
        ),
        in_scope: ScopeSupport::Metadata,
        verified: true,
    },
    NamespaceEntry {
        key: "package.relationships",
        strict: Some("http://purl.oclc.org/ooxml/package/relationships"),
        transitional: Some("http://schemas.openxmlformats.org/package/2006/relationships"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
    NamespaceEntry {
        key: "officeDocument.math",
        strict: Some("http://purl.oclc.org/ooxml/officeDocument/math"),
        transitional: Some("http://schemas.openxmlformats.org/officeDocument/2006/math"),
        in_scope: ScopeSupport::Optional,
        verified: false,
    },
    NamespaceEntry {
        key: "markupCompatibility",
        strict: Some("http://schemas.openxmlformats.org/markup-compatibility/2006"),
        transitional: Some("http://schemas.openxmlformats.org/markup-compatibility/2006"),
        in_scope: ScopeSupport::Full,
        verified: true,
    },
];

/// Registry of namespace mappings with URI lookup.
pub struct NamespaceRegistry {
    entries: &'static [NamespaceEntry],
    by_uri: HashMap<&'static str, &'static NamespaceEntry>,
}

impl NamespaceRegistry {
    /// Returns the process-wide registry.
    pub fn global() -> &'static NamespaceRegistry {
        static REGISTRY: OnceLock<NamespaceRegistry> = OnceLock::new();
        REGISTRY.get_or_init(NamespaceRegistry::build)
    }

    fn build() -> Self {
        let mut by_uri = HashMap::new();
        for entry in &NS {
            if let Some(uri) = entry.strict {
                by_uri.insert(uri, entry);
            }
            if let Some(uri) = entry.transitional {
                by_uri.insert(uri, entry);
            }
        }
        Self {
            entries: &NS,
            by_uri,
        }
    }

    /// Looks up an entry by either its Strict or Transitional URI.
    #[must_use]
    pub fn lookup(&self, uri: &str) -> Option<&'static NamespaceEntry> {
        self.by_uri.get(uri).copied()
    }

    /// Returns all registry entries.
    #[must_use]
    pub fn entries(&self) -> &'static [NamespaceEntry] {
        self.entries
    }
}

impl Default for NamespaceRegistry {
    fn default() -> Self {
        Self::build()
    }
}

/// Classifies a namespace URI as Strict or Transitional, if known.
#[must_use]
pub fn classify_namespace(uri: &str) -> Option<Conformance> {
    let entry = NamespaceRegistry::global().lookup(uri)?;
    if entry.strict == Some(uri) {
        Some(Conformance::Strict)
    } else if entry.transitional == Some(uri) {
        Some(Conformance::Transitional)
    } else {
        None
    }
}

/// Returns the Strict form of a namespace URI, when the registry knows it.
///
/// This is the mapping the writer needs for the places where a namespace URI is
/// carried as a *value* rather than as a declaration — `a:graphicData/@uri` is
/// the one that matters in practice. Such a value is invisible to the
/// conformance check, which only looks at declarations and relationship types,
/// and invisible to the normalizer, which rewrites declarations. A picture
/// written with the Transitional URI in `@uri` therefore passed every gate in
/// the project and was still not Strict.
#[must_use]
pub fn strict_form(uri: &str) -> Option<&'static str> {
    NamespaceRegistry::global()
        .lookup(uri)
        .and_then(|entry| entry.strict)
}

/// Classifies a relationship-type URI by its office-relationships base.
#[must_use]
pub fn classify_relationship(uri: &str) -> Option<Conformance> {
    const STRICT_BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
    const TRANSITIONAL_BASE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
    if uri.starts_with(STRICT_BASE) {
        Some(Conformance::Strict)
    } else if uri.starts_with(TRANSITIONAL_BASE) {
        Some(Conformance::Transitional)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_namespace, classify_relationship, NamespaceRegistry};
    use crate::ns::Conformance;

    #[test]
    fn registry_lookup_finds_both_families() {
        let registry = NamespaceRegistry::global();
        assert!(registry
            .lookup("http://purl.oclc.org/ooxml/wordprocessingml/main")
            .is_some());
        assert!(registry
            .lookup("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
            .is_some());
        assert!(registry.lookup("urn:not-registered").is_none());
    }

    #[test]
    fn classifies_namespaces() {
        assert_eq!(
            classify_namespace("http://purl.oclc.org/ooxml/wordprocessingml/main"),
            Some(Conformance::Strict)
        );
        assert_eq!(
            classify_namespace("http://schemas.openxmlformats.org/wordprocessingml/2006/main"),
            Some(Conformance::Transitional)
        );
        assert_eq!(classify_namespace("urn:x"), None);
    }

    #[test]
    fn classifies_relationship_types() {
        assert_eq!(
            classify_relationship(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles"
            ),
            Some(Conformance::Transitional)
        );
        assert_eq!(
            classify_relationship("http://purl.oclc.org/ooxml/officeDocument/relationships/styles"),
            Some(Conformance::Strict)
        );
    }
}
