//! Page boundaries for a converted PDF.
//!
//! The writer emits only `Document::sections` last element, as the body
//! `w:sectPr`. Every earlier page has to leave its properties on the paragraph
//! that ends that page (`w:pPr/w:sectPr`). The last page stays solely in
//! `Document::sections`, so the same break is not written twice.

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::props::{ParagraphProperties, Section, SectionProperties};
use strict_ooxml_wml::model::values::{LineSpacingRule, Rsids, SectionType, Spacing, Twips};

/// Sections closed so far, plus the page currently being filled.
pub(crate) struct SectionAccumulator {
    /// Properties of the section whose blocks are still being appended.
    open: Option<SectionProperties>,
    /// Sections already closed, in document order.
    sections: Vec<Section>,
}

impl SectionAccumulator {
    /// No pages yet.
    pub(crate) fn new() -> Self {
        Self {
            open: None,
            sections: Vec::new(),
        }
    }

    /// Opens `properties` for the page whose blocks are about to be appended.
    ///
    /// `always_break` is the visual mode: each PDF page is its own section, so
    /// two pages of the same size still paginate as two pages. Semantic mode
    /// breaks only when the page size or the margins change. Identical
    /// neighbours stay one section, and a later size change still gets a boundary.
    pub(crate) fn begin_page(
        &mut self,
        blocks: &mut Vec<Block>,
        properties: SectionProperties,
        always_break: bool,
    ) {
        let Some(open) = self.open.take() else {
            self.open = Some(properties);
            return;
        };
        if !always_break && same_geometry(&open, &properties) {
            self.open = Some(open);
            return;
        }
        self.open = Some(properties);
        close_section(blocks, open, &mut self.sections);
    }

    /// Closes the last page as the body section, without a paragraph break.
    pub(crate) fn finish(mut self) -> Vec<Section> {
        if let Some(properties) = self.open.take() {
            self.sections.push(Section {
                properties,
                location: SourceLocation::unknown(),
            });
        }
        self.sections
    }
}

/// Page size and margins are the geometry a later page must not overwrite.
fn same_geometry(left: &SectionProperties, right: &SectionProperties) -> bool {
    left.page_size == right.page_size && left.page_margins == right.page_margins
}

/// Ends `properties` on the last paragraph of the blocks gathered so far.
fn close_section(
    blocks: &mut Vec<Block>,
    mut properties: SectionProperties,
    sections: &mut Vec<Section>,
) {
    properties.section_type = Some(SectionType::NextPage);
    attach_break(blocks, properties.clone());
    sections.push(Section {
        properties,
        location: SourceLocation::unknown(),
    });
}

/// Puts `properties` on the paragraph that ends the section.
///
/// A table cannot carry `w:sectPr`. When the page does not end on a paragraph,
/// or that paragraph already ends an earlier section, an empty paragraph after
/// it holds the break. That paragraph is inkless; layout must not turn it into
/// an extra page.
fn attach_break(blocks: &mut Vec<Block>, properties: SectionProperties) {
    if let Some(Block::Paragraph(paragraph)) = blocks.last_mut() {
        if paragraph.props.section.is_none() {
            paragraph.props.section = Some(properties);
            return;
        }
    }
    blocks.push(Block::Paragraph(marker_paragraph(properties)));
}

/// An empty paragraph whose only content is the section break.
fn marker_paragraph(properties: SectionProperties) -> Paragraph {
    Paragraph {
        props: ParagraphProperties {
            section: Some(properties),
            spacing: Some(Spacing {
                before: Some(Twips(0)),
                after: Some(Twips(0)),
                line: Some(Twips(1)),
                line_rule: Some(LineSpacingRule::Exact),
                ..Spacing::default()
            }),
            ..ParagraphProperties::default()
        },
        inlines: Vec::new(),
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: SourceLocation::unknown(),
    }
}
