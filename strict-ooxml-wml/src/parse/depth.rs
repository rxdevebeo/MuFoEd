//! The XML recursion guard, in one place.
//!
//! The guard is a pair — enter, leave — and the second half was the defect
//! AUD-07 names: an `enter()` without a matching `leave()` on some path leaves the
//! counter above where it started, and nothing fails where the leak is. The *next*
//! sibling does: a `settings.xml` with 300 `m:mathPr` in a row was refused as
//! `LimitExceeded { XmlDepth, 256, 257 }`, a statement about nesting that the
//! document never made.
//!
//! So the counter lives here and `enter`/`leave` are visible only to
//! `parse::mod`. A parser file can reach the recursion through
//! [`PartParser::nested`](super::PartParser::nested) and cannot touch the counter
//! itself, so there is no spelling of a direct call that compiles. The previous
//! shape made the correct thing a convention and the defect a mistake anyone
//! could repeat — and `math.rs` repeated it 21 times, because it is a *child*
//! module of `parse` and `PartParser`'s private methods were visible to it.
//!
//! `pub(super)` rather than `pub(crate)`: the narrowest reach that still lets the
//! one legitimate caller pair the two halves. The pairing itself still happens in
//! exactly one place.

use strict_ooxml_core::error::{LimitKind, Result, StrictError};

/// How deep the parser currently is, and how deep it may go.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Depth {
    depth: u32,
    max: u32,
}

impl Depth {
    /// A guard that allows `max` levels.
    pub(super) const fn new(max: u32) -> Self {
        Self { depth: 0, max }
    }

    /// Enters one level, refusing past the bound.
    ///
    /// Panics are not an option here and an early return from a caller would be
    /// exactly the leak this module exists to prevent, so the caller is
    /// [`PartParser::nested`](super::PartParser::nested) and nothing else.
    #[doc(hidden)]
    pub(super) fn enter(&mut self) -> Result<()> {
        // Checked *before* the counter moves, so a refused entry leaves it where
        // it was: `nested` propagates the error and never reaches its `leave`,
        // and a parser that is abandoned on an error should not be holding an
        // extra level on the way out.
        let depth = self.depth.saturating_add(1);
        if depth > self.max {
            return Err(StrictError::LimitExceeded {
                kind: LimitKind::XmlDepth,
                limit: u64::from(self.max),
                actual: u64::from(depth),
            });
        }
        self.depth = depth;
        Ok(())
    }

    /// Leaves one level. See [`enter`](Self::enter) for the visibility.
    #[doc(hidden)]
    pub(super) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// The current depth; for the `#[cfg(test)]` property test.
    #[cfg(test)]
    pub(super) const fn current(self) -> u32 {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use strict_ooxml_core::error::{LimitKind, StrictError};

    use super::Depth;

    #[test]
    fn the_counter_is_back_at_zero_after_a_body_that_failed() {
        // The defect in the smallest form that can state it: an error on the way
        // out of the body must leave the counter where it was.
        let mut depth = Depth::new(4);
        for _ in 0..100 {
            let before = depth.current();
            depth.enter().expect("inside the budget");
            // The body fails, which is the path a leaked `leave` used to miss.
            let result: Result<(), StrictError> = Err(StrictError::InvalidPartName("x".to_owned()));
            depth.leave();
            assert!(result.is_err());
            assert_eq!(depth.current(), before, "the guard leaked");
        }
    }

    #[test]
    fn the_bound_is_refused_rather_than_climbed() {
        let mut depth = Depth::new(2);
        assert!(depth.enter().is_ok());
        assert!(depth.enter().is_ok());
        let error = depth.enter().expect_err("the third level");
        assert!(matches!(
            error,
            StrictError::LimitExceeded {
                kind: LimitKind::XmlDepth,
                ..
            }
        ));
        // And the refusal itself did not move the counter, so a caller that
        // propagates the error without reaching its `leave` leaves no level open.
        assert_eq!(depth.current(), 2);
        depth.leave();
        depth.leave();
        assert_eq!(depth.current(), 0);
    }
}
