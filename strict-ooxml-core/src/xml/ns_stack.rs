//! Namespace scope stack.
//!
//! Tracks `xmlns`/`xmlns:prefix` declarations as elements open and close and
//! resolves prefixes in scope (stage task S1.10). An unknown prefix is reported
//! as [`StrictError::UnboundPrefix`](crate::error::StrictError) by the caller,
//! never a panic.

use std::sync::Arc;

use crate::xml::qname::NsUri;

/// The `xml` prefix is bound to a fixed URI and cannot be redeclared.
pub(crate) const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// A single prefix binding.
#[derive(Clone, Debug)]
struct Binding {
    /// `None` is the default (unprefixed) declaration.
    prefix: Option<String>,
    uri: Arc<str>,
}

/// A stack of namespace bindings with explicit scope marks.
#[derive(Clone, Debug, Default)]
pub(crate) struct NsStack {
    bindings: Vec<Binding>,
    marks: Vec<usize>,
}

impl NsStack {
    /// Creates an empty stack.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Opens a new scope. Returns without changing the stack if unbalanced
    /// `pop_scope` calls are avoided by the caller.
    pub(crate) fn push_scope(&mut self) {
        self.marks.push(self.bindings.len());
    }

    /// Closes the innermost scope, discarding its bindings.
    pub(crate) fn pop_scope(&mut self) {
        if let Some(mark) = self.marks.pop() {
            self.bindings.truncate(mark);
        }
    }

    /// Declares a binding in the current scope.
    pub(crate) fn declare(&mut self, prefix: Option<String>, uri: &str) {
        self.bindings.push(Binding {
            prefix,
            uri: Arc::from(uri),
        });
    }

    /// Resolves a prefix (`None` = default namespace) to a URI in scope.
    ///
    /// The `xml` prefix is resolved to its fixed URI. An empty default
    /// declaration (`xmlns=""`) yields `None` (no default namespace).
    pub(crate) fn resolve(&self, prefix: Option<&str>) -> Option<NsUri> {
        if prefix == Some("xml") {
            return Some(NsUri::new(XML_NS));
        }
        for binding in self.bindings.iter().rev() {
            if binding.prefix.as_deref() == prefix {
                if binding.uri.is_empty() {
                    return None;
                }
                return Some(NsUri::new(binding.uri.clone()));
            }
        }
        None
    }

    /// Returns `true` if the prefix was explicitly declared in some open scope.
    ///
    /// Used to distinguish "no default namespace" from "unknown prefix" for
    /// prefixed names.
    pub(crate) fn is_declared(&self, prefix: &str) -> bool {
        prefix == "xml"
            || self
                .bindings
                .iter()
                .any(|b| b.prefix.as_deref() == Some(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::NsStack;

    #[test]
    fn resolves_and_scopes() {
        let mut stack = NsStack::new();
        stack.push_scope();
        stack.declare(None, "urn:default");
        stack.declare(Some("w".to_owned()), "urn:w");
        assert_eq!(stack.resolve(None).unwrap(), "urn:default");
        assert_eq!(stack.resolve(Some("w")).unwrap(), "urn:w");
        assert!(stack.resolve(Some("z")).is_none());
        assert!(stack.is_declared("w"));
        assert!(!stack.is_declared("z"));

        stack.push_scope();
        stack.declare(None, "urn:inner");
        assert_eq!(stack.resolve(None).unwrap(), "urn:inner");
        stack.pop_scope();
        assert_eq!(stack.resolve(None).unwrap(), "urn:default");
        stack.pop_scope();
        assert!(stack.resolve(None).is_none());
    }

    #[test]
    fn xml_prefix_is_reserved() {
        let stack = NsStack::new();
        assert_eq!(stack.resolve(Some("xml")).unwrap(), super::XML_NS);
    }

    #[test]
    fn empty_default_declaration_unsets_namespace() {
        let mut stack = NsStack::new();
        stack.push_scope();
        stack.declare(None, "urn:x");
        stack.declare(None, "");
        assert!(stack.resolve(None).is_none());
    }
}
