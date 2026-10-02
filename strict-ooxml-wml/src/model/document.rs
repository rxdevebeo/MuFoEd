//! The top-level immutable document model.

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;

use super::block::Block;
use super::drawing::MediaIndex;
use super::fonts::FontTable;
use super::notes::NoteTable;
use super::numbering::NumberingTable;
use super::props::Section;
use super::settings::Settings;
use super::styles::StyleTable;
use super::support::SupportModel;
use super::theme::Theme;

/// The document body (`w:body`) as a sequence of block-level items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Body {
    /// Block-level content.
    pub blocks: Vec<Block>,
}

/// A parsed header or footer part (`w:hdr`/`w:ftr`).
///
/// Headers and footers contain the same block-level content as the body and
/// are referenced by [`SectionProperties`](super::props::SectionProperties).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderFooter {
    /// Part id of the header/footer XML.
    pub part: PartId,
    /// Whether this is a header (`w:hdr`) rather than a footer (`w:ftr`).
    pub is_header: bool,
    /// Block content.
    pub blocks: Vec<Block>,
    /// Source location of the part root element.
    pub location: SourceLocation,
}

/// The package parts a [`Document`] was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSource {
    /// Main document part (`/word/document.xml`).
    pub main_document: PartId,
    /// Styles part, if present.
    pub styles: Option<PartId>,
    /// Numbering part, if present.
    pub numbering: Option<PartId>,
    /// Settings part, if present.
    pub settings: Option<PartId>,
    /// Font table part, if present.
    pub font_table: Option<PartId>,
    /// Footnotes part, if present.
    pub footnotes: Option<PartId>,
    /// Endnotes part, if present.
    pub endnotes: Option<PartId>,
    /// Theme part, if present.
    pub theme: Option<PartId>,
}

/// An immutable, resolved WordprocessingML Strict document.
///
/// The model is `Send + Sync` and self-contained; it borrows neither the
/// package nor the parser (ADR-0004).
#[derive(Clone, Debug)]
pub struct Document {
    /// Document body.
    pub body: Body,
    /// Style definitions.
    pub styles: StyleTable,
    /// Numbering definitions.
    pub numbering: NumberingTable,
    /// Footnote definitions (`footnotes.xml`).
    pub footnotes: NoteTable,
    /// Endnote definitions (`endnotes.xml`).
    pub endnotes: NoteTable,
    /// Document settings.
    pub settings: Settings,
    /// Parsed theme, if present.
    pub theme: Option<Theme>,
    /// The font table, with the faces it embeds.
    ///
    /// None means the document has no font table part; Some that is empty
    /// means it has one this project read and found nothing to carry. The
    /// difference matters to a writer deciding whether to emit the part at all.
    pub font_table: Option<FontTable>,
    /// Resolved sections, in document order.
    pub sections: Vec<Section>,
    /// Parsed header/footer parts referenced by the sections, in first-seen order.
    pub headers_footers: Vec<HeaderFooter>,
    /// Media parts referenced by the document.
    pub media: MediaIndex,
    /// Support information gathered while parsing.
    pub support: SupportModel,
    /// Source parts.
    pub source: DocumentSource,
}

impl Document {
    /// Returns the support model.
    #[must_use]
    pub fn support(&self) -> &SupportModel {
        &self.support
    }

    /// Returns the media index.
    #[must_use]
    pub fn media(&self) -> &MediaIndex {
        &self.media
    }

    /// Returns the parsed header/footer for `part`, if any.
    #[must_use]
    pub fn header_footer(&self, part: &PartId) -> Option<&HeaderFooter> {
        self.headers_footers
            .iter()
            .find(|header_footer| &header_footer.part == part)
    }

    /// Renders a human-readable support summary.
    #[must_use]
    pub fn support_debug(&self) -> String {
        self.support.debug_summary()
    }
}
