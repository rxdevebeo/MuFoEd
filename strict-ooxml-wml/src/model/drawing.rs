//! DrawingML inline-graphics model and the media index.

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::part::PartId;

use super::inline::OpaqueInline;
use super::values::Emu;

/// A DrawingML extent (`wp:extent`, `a:ext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Extent {
    /// Width in EMU.
    pub cx: Emu,
    /// Height in EMU.
    pub cy: Emu,
}

/// Non-visual drawing properties (`wp:docPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DocPr {
    /// Drawing object id.
    pub id: Option<u32>,
    /// Name.
    pub name: Option<Arc<str>>,
    /// Description.
    pub descr: Option<Arc<str>>,
}

/// A reference to an image part (`a:blip`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlipRef {
    /// `r:embed` relationship id.
    pub embed: Option<RelId>,
    /// `r:link` relationship id (external; not fetched).
    pub link: Option<RelId>,
    /// Resolved media part for `embed`.
    pub resolved: Option<PartId>,
    /// Source location.
    pub location: SourceLocation,
}

/// A `pic:pic` picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Picture name (`pic:cNvPr/@name`).
    pub name: Option<Arc<str>>,
    /// Picture description (`pic:cNvPr/@descr`).
    pub descr: Option<Arc<str>>,
    /// The image reference.
    pub blip: Option<BlipRef>,
    /// Picture geometry extent.
    pub extent: Option<Extent>,
}

/// An inline drawing (`wp:inline`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineDrawing {
    /// Drawing extent (`wp:extent`).
    pub extent: Option<Extent>,
    /// Non-visual properties (`wp:docPr`).
    pub doc_pr: Option<DocPr>,
    /// `a:graphicData/@uri`.
    pub graphic_uri: Option<Arc<str>>,
    /// Embedded picture, when the graphic is a picture.
    pub picture: Option<Picture>,
    /// Source location.
    pub location: SourceLocation,
}

/// A floating drawing (`wp:anchor`), recorded but not parsed in Stage 2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorStub {
    /// `wp:docPr` if present.
    pub doc_pr: Option<DocPr>,
    /// Source location.
    pub location: SourceLocation,
}

/// A DrawingML drawing (`w:drawing`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    /// Drawing payload.
    pub kind: DrawingKind,
    /// Source location.
    pub location: SourceLocation,
}

/// Discriminates the payload of a [`Drawing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawingKind {
    /// Inline drawing (`wp:inline`), fully parsed.
    Inline(InlineDrawing),
    /// Floating drawing (`wp:anchor`), not supported in Stage 2.
    Anchor(AnchorStub),
    /// Unrecognised drawing content.
    Opaque(OpaqueInline),
}

/// Media kind inferred from a part's content type or extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// PNG image.
    Png,
    /// JPEG image.
    Jpeg,
    /// GIF image.
    Gif,
    /// BMP image.
    Bmp,
    /// TIFF image.
    Tiff,
    /// EMF metafile.
    Emf,
    /// WMF metafile.
    Wmf,
    /// SVG image.
    Svg,
    /// Any other media.
    Other,
}

impl MediaKind {
    /// Infers the media kind from a content type or file extension string.
    #[must_use]
    pub fn from_content_type(content_type: &str) -> Self {
        let normalized = content_type.to_ascii_lowercase();
        if normalized.contains("png") {
            return Self::Png;
        }
        if normalized.contains("jpeg") || normalized.contains("jpg") {
            return Self::Jpeg;
        }
        if normalized.contains("gif") {
            return Self::Gif;
        }
        if normalized.contains("bmp") {
            return Self::Bmp;
        }
        if normalized.contains("tiff") || normalized.contains("tif") {
            return Self::Tiff;
        }
        if normalized.contains("emf") {
            return Self::Emf;
        }
        if normalized.contains("wmf") {
            return Self::Wmf;
        }
        if normalized.contains("svg") {
            return Self::Svg;
        }
        Self::Other
    }

    /// Infers the media kind from a part path's extension.
    #[must_use]
    pub fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "bmp" => Self::Bmp,
            "tif" | "tiff" => Self::Tiff,
            "emf" => Self::Emf,
            "wmf" => Self::Wmf,
            "svg" => Self::Svg,
            _ => Self::Other,
        }
    }
}

/// Metadata for one media part (image bytes are not decoded in Stage 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaItem {
    /// Media part id.
    pub part: PartId,
    /// Resolved content type.
    pub content_type: Option<Arc<str>>,
    /// Inferred media kind.
    pub kind: MediaKind,
}

/// Index of media parts referenced by the document.
#[derive(Clone, Debug, Default)]
pub struct MediaIndex {
    items: Vec<MediaItem>,
    by_part: HashMap<PartId, usize>,
}

impl MediaIndex {
    /// Creates an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a media item, returning its index (deduplicating by part).
    pub fn insert(&mut self, item: MediaItem) -> usize {
        if let Some(&index) = self.by_part.get(&item.part) {
            return index;
        }
        let index = self.items.len();
        self.by_part.insert(item.part.clone(), index);
        self.items.push(item);
        index
    }

    /// Returns the item for a part, if indexed.
    #[must_use]
    pub fn get(&self, part: &PartId) -> Option<&MediaItem> {
        self.by_part
            .get(part)
            .and_then(|&index| self.items.get(index))
    }

    /// Iterates over indexed media items.
    pub fn iter(&self) -> impl Iterator<Item = &MediaItem> {
        self.items.iter()
    }

    /// Returns the number of media items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if no media was indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
