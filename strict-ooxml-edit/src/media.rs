//! Pictures an editing session adds: their bytes, which the source package
//! does not have, and the drawing that shows one.
//!
//! The model indexes media parts but keeps no bytes; layout and the writer
//! read them from a source. [`MediaLayer`] is that source for an edited
//! document: the session's added pictures first, the original package behind
//! them.

use std::collections::BTreeMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::drawing::{BlipRef, DocPr, InlineDrawing, Picture, PictureMarkup};
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Emu, Extent, Graphic, Inline, MediaItem, MediaKind,
    RunContent,
};

use crate::{EditError, Invariant};

/// Bytes of the pictures a session added, by media part.
#[derive(Clone, Debug, Default)]
pub struct AddedMedia {
    parts: BTreeMap<PartId, Arc<[u8]>>,
}

impl AddedMedia {
    /// The bytes of an added part.
    #[must_use]
    pub fn get(&self, part: &PartId) -> Option<&Arc<[u8]>> {
        self.parts.get(part)
    }

    /// Whether nothing was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Keeps `bytes` under a media part name neither `document` nor this
    /// store uses, and returns its index entry: put it in the document with
    /// [`crate::Definition::AddMedia`] in the transaction that inserts the
    /// picture. Only raster formats a renderer and Word both show are taken
    /// (PNG, JPEG, GIF, BMP).
    ///
    /// # Errors
    ///
    /// [`EditError::InvalidModel`] with [`Invariant::Drawing`] for bytes that
    /// are none of those.
    pub fn add(&mut self, document: &Document, bytes: Vec<u8>) -> Result<MediaItem, EditError> {
        let (kind, content_type, extension) =
            picture_format(&bytes).ok_or(EditError::InvalidModel(Invariant::Drawing))?;
        let mut number = self.parts.len() + 1;
        let part = loop {
            let candidate = PartId::new(format!("/word/media/added{number}.{extension}"));
            if document.media.get(&candidate).is_none() && !self.parts.contains_key(&candidate) {
                break candidate;
            }
            number += 1;
        };
        self.parts.insert(part.clone(), Arc::from(bytes));
        Ok(MediaItem {
            part,
            content_type: Some(Arc::from(content_type)),
            kind,
        })
    }
}

/// A picture's format from its first bytes: kind, content type, extension.
#[must_use]
pub fn picture_format(bytes: &[u8]) -> Option<(MediaKind, &'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some((MediaKind::Png, "image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some((MediaKind::Jpeg, "image/jpeg", "jpeg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some((MediaKind::Gif, "image/gif", "gif"))
    } else if bytes.starts_with(b"BM") && bytes.len() > 26 {
        Some((MediaKind::Bmp, "image/bmp", "bmp"))
    } else {
        None
    }
}

/// A picture's size in pixels, read from its header (PNG, JPEG, GIF, BMP).
#[must_use]
pub fn picture_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    let four = |at: usize| -> Option<[u8; 4]> { bytes.get(at..at + 4)?.try_into().ok() };
    let two = |at: usize| -> Option<[u8; 2]> { bytes.get(at..at + 2)?.try_into().ok() };
    let be32 = |at: usize| four(at).map(u32::from_be_bytes);
    let le16 = |at: usize| two(at).map(|b| u32::from(u16::from_le_bytes(b)));
    let le32 = |at: usize| four(at).map(|b| i32::from_le_bytes(b).unsigned_abs());
    let size = match picture_format(bytes)?.0 {
        MediaKind::Png => (be32(16)?, be32(20)?),
        MediaKind::Gif => (le16(6)?, le16(8)?),
        MediaKind::Bmp => (le32(18)?, le32(22)?),
        MediaKind::Jpeg => jpeg_pixels(bytes)?,
        _ => return None,
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// The frame size of a JPEG: the first start-of-frame marker's.
fn jpeg_pixels(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 9 < bytes.len() {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(at + 2)?,
            *bytes.get(at + 3)?,
        ]));
        // SOF0..SOF15, except DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let width = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + length;
    }
    None
}

/// English Metric Units per CSS pixel (1/96 inch).
pub const EMU_PER_PX: i64 = 9525;

/// An inline picture of `media`, `width` by `height`, with drawing id `id`
/// ([`unused_drawing_id`]) and `name` for its alternative text's title.
#[must_use]
pub fn picture_drawing(media: &MediaItem, width: Emu, height: Emu, id: u32, name: &str) -> Drawing {
    let location = SourceLocation::unknown();
    let extent = Extent {
        cx: width,
        cy: height,
    };
    Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            dist_top: Some(0),
            dist_bottom: Some(0),
            dist_left: Some(0),
            dist_right: Some(0),
            doc_pr: Some(DocPr {
                hidden: None,
                id: Some(id),
                name: Some(Arc::from(name)),
                descr: None,
                title: None,
            }),
            graphic_uri: Some(Arc::from("http://purl.oclc.org/ooxml/drawingml/picture")),
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some(Arc::from(name)),
                descr: None,
                nv_id: Some(0),
                bw_mode: None,
                blip: Some(BlipRef {
                    embed: None,
                    link: None,
                    resolved: Some(media.part.clone()),
                    cstate: None,
                    location: location.clone(),
                }),
                extent: Some(extent),
                src_rect: None,
                xfrm: None,
                markup: PictureMarkup::default(),
            })),
            location: location.clone(),
        }),
        location,
    }
}

/// A drawing id (`wp:docPr/@id`) above every one the body uses.
#[must_use]
pub fn unused_drawing_id(document: &Document) -> u32 {
    fn id(drawing: &Drawing) -> u32 {
        let doc_pr = match &drawing.kind {
            DrawingKind::Inline(v) => v.doc_pr.as_ref(),
            DrawingKind::Anchor(v) => v.doc_pr.as_ref(),
            DrawingKind::Opaque(_) => None,
        };
        doc_pr.and_then(|d| d.id).unwrap_or(0)
    }
    fn inlines(list: &[Inline], top: &mut u32) {
        for inline in list {
            match inline {
                Inline::Drawing(d) => *top = (*top).max(id(d)),
                Inline::Run(run) => {
                    for content in &run.content {
                        if let RunContent::Drawing(d) = content {
                            *top = (*top).max(id(d));
                        }
                    }
                }
                Inline::Hyperlink(v) => inlines(&v.inlines, top),
                Inline::SdtInline(v) => inlines(&v.inlines, top),
                Inline::Directional(v) => inlines(&v.inlines, top),
                _ => {}
            }
        }
    }
    fn blocks(list: &[Block], top: &mut u32) {
        for block in list {
            match block {
                Block::Paragraph(p) => inlines(&p.inlines, top),
                Block::Table(t) => {
                    for row in &t.rows {
                        for cell in &row.cells {
                            blocks(&cell.blocks, top);
                        }
                    }
                }
                Block::SdtBlock(s) => blocks(&s.blocks, top),
                _ => {}
            }
        }
    }
    let mut top = 0;
    blocks(&document.body.blocks, &mut top);
    top.saturating_add(1)
}

/// Media for an edited document: the session's added pictures, then `base`
/// (the package it was opened from). It serves layout
/// (`strict_ooxml_render_svg::MediaSource`) and the writer
/// (`strict_ooxml_write::package::Source`).
#[cfg_attr(not(any(feature = "visual", feature = "save")), allow(dead_code))]
pub struct MediaLayer<'a, S: ?Sized> {
    added: &'a AddedMedia,
    base: Option<&'a S>,
}

impl<'a, S: ?Sized> MediaLayer<'a, S> {
    /// `added` over `base`.
    #[must_use]
    pub fn new(added: &'a AddedMedia, base: Option<&'a S>) -> Self {
        Self { added, base }
    }
}

#[cfg_attr(not(any(feature = "visual", feature = "save")), allow(dead_code))]
fn missing(part: &PartId) -> strict_ooxml_core::error::StrictError {
    strict_ooxml_core::error::StrictError::MissingPart(part.clone())
}

#[cfg(feature = "visual")]
impl<S: strict_ooxml_render_svg::MediaSource + ?Sized> strict_ooxml_render_svg::MediaSource
    for MediaLayer<'_, S>
{
    fn read_media(&self, part: &PartId) -> strict_ooxml_core::error::Result<Vec<u8>> {
        if let Some(bytes) = self.added.get(part) {
            return Ok(bytes.to_vec());
        }
        self.base
            .map_or_else(|| Err(missing(part)), |base| base.read_media(part))
    }
}

#[cfg(feature = "save")]
impl<S: strict_ooxml_write::package::Source + ?Sized> strict_ooxml_write::package::Source
    for MediaLayer<'_, S>
{
    fn read_part(&self, part: &PartId) -> strict_ooxml_core::error::Result<Vec<u8>> {
        if let Some(bytes) = self.added.get(part) {
            return Ok(bytes.to_vec());
        }
        self.base
            .map_or_else(|| Err(missing(part)), |base| base.read_part(part))
    }

    fn relationship(
        &self,
        from: &PartId,
        rel_id: &str,
    ) -> Option<strict_ooxml_write::package::RelationshipInfo> {
        self.base?.relationship(from, rel_id)
    }

    fn relationships(&self, from: &PartId) -> Vec<strict_ooxml_write::package::RelationshipInfo> {
        self.base
            .map(|base| base.relationships(from))
            .unwrap_or_default()
    }

    fn content_type(&self, part: &PartId) -> Option<String> {
        if self.added.get(part).is_some() {
            return picture_format(self.added.get(part)?)
                .map(|(_, content_type, _)| content_type.to_owned());
        }
        self.base?.content_type(part)
    }

    fn parts(&self) -> Vec<PartId> {
        let mut parts = self
            .base
            .map(strict_ooxml_write::package::Source::parts)
            .unwrap_or_default();
        parts.extend(self.added.parts.keys().cloned());
        parts
    }

    fn reachable_parts(&self, from: &PartId) -> strict_ooxml_core::error::Result<Vec<PartId>> {
        self.base
            .map_or_else(|| Ok(Vec::new()), |base| base.reachable_parts(from))
    }

    fn was_normalized(&self) -> bool {
        self.base
            .is_some_and(strict_ooxml_write::package::Source::was_normalized)
    }

    fn relationship_ids(&self, from: &PartId) -> Vec<String> {
        self.base
            .map(|base| base.relationship_ids(from))
            .unwrap_or_default()
    }
}
