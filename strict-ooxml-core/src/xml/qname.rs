//! Qualified names and namespace URIs.
//!
//! Owns the resolved-URI newtype and the qualified-name type returned alongside
//! XML events (`TZ-STRICT-OOXML-RUST.md` §9.2; stage task S1.10). See ADR-0003
//! for why these types own their strings.

use std::fmt;
use std::sync::Arc;

/// A resolved namespace URI.
///
/// Interned as an [`Arc`] so that cloning a resolved name is cheap and repeated
/// URIs share one allocation.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NsUri(Arc<str>);

impl NsUri {
    /// Creates a namespace URI.
    pub fn new(uri: impl Into<Arc<str>>) -> Self {
        Self(uri.into())
    }

    /// Returns the URI as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NsUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NsUri({:?})", &*self.0)
    }
}

impl fmt::Display for NsUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for NsUri {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for NsUri {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

/// A qualified XML name: optional namespace, optional prefix and local part.
///
/// For an unprefixed element the namespace is the in-scope default namespace
/// (possibly absent); for an unprefixed attribute the namespace is always
/// absent, per the XML Namespaces specification.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QName {
    /// Resolved namespace URI, if any.
    pub ns: Option<NsUri>,
    /// Namespace prefix as written, if the name was prefixed.
    pub prefix: Option<String>,
    /// Local part of the name.
    pub local: String,
}

impl QName {
    /// Creates a qualified name.
    pub fn new(ns: Option<NsUri>, prefix: Option<String>, local: String) -> Self {
        Self { ns, prefix, local }
    }

    /// Returns the local part.
    #[must_use]
    pub fn local(&self) -> &str {
        &self.local
    }

    /// Returns the resolved namespace URI, if any.
    #[must_use]
    pub fn namespace(&self) -> Option<&NsUri> {
        self.ns.as_ref()
    }
}

impl fmt::Display for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(prefix) = &self.prefix {
            write!(f, "{prefix}:{}", self.local)
        } else {
            f.write_str(&self.local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NsUri, QName};

    #[test]
    fn prefixed_and_plain_names() {
        let prefixed = QName::new(
            Some(NsUri::new("urn:w")),
            Some("w".to_owned()),
            "document".to_owned(),
        );
        assert_eq!(prefixed.to_string(), "w:document");
        assert_eq!(prefixed.local(), "document");
        assert_eq!(prefixed.namespace().unwrap(), "urn:w");

        let plain = QName::new(None, None, "body".to_owned());
        assert_eq!(plain.to_string(), "body");
        assert!(plain.namespace().is_none());
    }

    #[test]
    fn namespace_uri_compares_to_str() {
        assert_eq!(NsUri::new("urn:a"), "urn:a");
        assert_ne!(NsUri::new("urn:a"), "urn:b");
    }
}
