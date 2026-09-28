//! The top-level immutable document model.

use strict_ooxml_core::part::PartId;

use super::block::Block;
use super::drawing::MediaIndex;
use super::numbering::NumberingTable;
use super::props::Section;
use super::settings::Settings;
use super::styles::StyleTable;
use super::support::SupportModel;

/// The document body (`w:body`) as a sequence of block-level items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Body {
    /// Block-level content.
    pub blocks: Vec<Block>,
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
    /// Document settings.
    pub settings: Settings,
    /// Resolved sections, in document order.
    pub sections: Vec<Section>,
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

    /// Renders a human-readable support summary.
    #[must_use]
    pub fn support_debug(&self) -> String {
        self.support.debug_summary()
    }
}
