//! OMML formula support: model → layout → SVG, plus the MathML projection
//! (`STAGE-5C-TASK.md` §3.1.3–§3.1.5, §6).
//!
//! * [`layout`] places a formula on the text baseline (S5C.3, S5C.4).
//! * [`shapes`] draws the stretchy constructs as deterministic vectors (§9.1).
//! * [`mathml`] projects the model to MathML, the independent structural view
//!   of §9.6.
//!
//! The module is additive: without the `--no-math` opt-out the renderer draws
//! formulas exactly as Word/WPS do.

pub(crate) mod layout;
pub(crate) mod mathml;
pub(crate) mod shapes;

pub(crate) use layout::{layout_display, layout_inline};
pub use mathml::{math_expression_to_mathml, math_paragraph_to_mathml, MathMlError};
