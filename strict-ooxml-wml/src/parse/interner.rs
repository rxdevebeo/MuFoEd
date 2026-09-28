//! Name and value interning (perf P9, ADR-0004).
//!
//! The interner deduplicates strings and hands out shared `Arc<str>` handles so
//! the DOM stores one allocation per distinct name or value.

use std::collections::HashSet;
use std::sync::Arc;

/// A deduplicating pool of strings.
#[derive(Clone, Debug, Default)]
pub struct Interner {
    pool: HashSet<Arc<str>>,
}

impl Interner {
    /// Creates an empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a shared handle for `value`, inserting it on first use.
    pub fn intern(&mut self, value: &str) -> Arc<str> {
        if let Some(existing) = self.pool.get(value) {
            return Arc::clone(existing);
        }
        let interned: Arc<str> = Arc::from(value);
        self.pool.insert(Arc::clone(&interned));
        interned
    }

    /// Returns the number of distinct interned strings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// Returns `true` if nothing has been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Interner;

    #[test]
    fn interns_and_shares_handles() {
        let mut interner = Interner::new();
        let a = interner.intern("w:p");
        let b = interner.intern("w:p");
        let c = interner.intern("w:r");
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert!(!std::sync::Arc::ptr_eq(&a, &c));
        assert_eq!(interner.len(), 2);
    }
}
