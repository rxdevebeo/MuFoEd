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

mod clearance;
pub(crate) mod layout;
pub(crate) mod mathml;
pub(crate) mod shapes;

pub(crate) use layout::{layout_display, layout_inline_wrapped};
pub use mathml::{math_expression_to_mathml, math_paragraph_to_mathml, MathMlError};
/// Keep cached field results visible and hide their instructions without
/// changing the source model that the DOCX writer serializes.
pub(crate) fn visible_nodes(
    nodes: &[strict_ooxml_wml::model::math::MathNode],
) -> impl Iterator<Item = &strict_ooxml_wml::model::math::MathNode> {
    use strict_ooxml_wml::model::{math::MathNode, values::FieldCharType};
    let mut results = Vec::new();
    nodes.iter().filter(move |node| {
        if let MathNode::Run(run) = node {
            if let Some(kind) = run.field_char {
                match kind {
                    FieldCharType::Begin => results.push(false),
                    FieldCharType::Separate => {
                        if let Some(result) = results.last_mut() {
                            *result = true;
                        }
                    }
                    FieldCharType::End => {
                        results.pop();
                    }
                }
                return false;
            }
        }
        results.iter().all(|result| *result)
    })
}
