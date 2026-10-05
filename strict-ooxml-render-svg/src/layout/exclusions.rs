//! Page-level wrap exclusions carried across paragraphs (`FIX_PLAN` §3.3, F15).

use crate::layout::floating::WrapSide;

/// A wrap rectangle in absolute page coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageExclusion {
    /// Left edge, already expanded by `distL`.
    pub left: f64,
    /// Right edge, already expanded by `distR`.
    pub right: f64,
    /// Top edge on the page, already expanded by `distT`.
    pub top: f64,
    /// Bottom edge on the page, already expanded by `distB`.
    pub bottom: f64,
    /// Side policy.
    pub side: WrapSide,
}

impl PageExclusion {
    /// Converts page coordinates into paragraph-local ones for a host at `host_y`.
    #[must_use]
    pub(crate) fn to_paragraph_local(self, host_y: f64) -> crate::layout::floating::WrapExclusion {
        crate::layout::floating::WrapExclusion {
            left: self.left,
            right: self.right,
            top: self.top - host_y,
            bottom: self.bottom - host_y,
            side: self.side,
        }
    }
}
