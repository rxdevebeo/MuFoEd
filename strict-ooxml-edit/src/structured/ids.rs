//! Paragraph identities (`w14:paraId`) kept unique across every story.
use crate::EditError;
use std::collections::HashMap;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Graphic, Inline, ParaId, Paragraph, RunContent,
};

use super::address::{visit_boxes, visit_drawings};

/// Every paragraph identity in the document, uppercased, with how many
/// paragraphs carry it: identities are unique within a story, not across
/// stories, so one id can stand in two.
///
/// The editor keeps one up to date by [`settle`](Self::settle) after every
/// patch, so minting a fresh id needs no walk of the document; ids minted by
/// the current command are logged until it settles or is undone.
#[derive(Clone, Debug, Default)]
pub(super) struct IdIndex {
    counts: HashMap<String, usize>,
    claimed: Vec<String>,
}
impl IdIndex {
    /// The index of every story of `document`.
    pub(super) fn of(document: &Document) -> Self {
        let mut index = Self::default();
        let stories = std::iter::once(&document.body.blocks)
            .chain(document.headers_footers.iter().map(|h| &h.blocks))
            .chain(
                document
                    .footnotes
                    .iter()
                    .chain(document.endnotes.iter())
                    .map(|n| &n.blocks),
            );
        for blocks in stories {
            index.add(blocks);
        }
        index
    }
    /// Whether some paragraph carries `id` (uppercased).
    pub(super) fn contains(&self, id: &str) -> bool {
        self.counts.contains_key(id)
    }
    /// How many paragraphs carry `id` (uppercased).
    pub(super) fn count(&self, id: &str) -> usize {
        self.counts.get(id).copied().unwrap_or(0)
    }
    /// Takes `id` for a new paragraph if no paragraph has it.
    fn claim(&mut self, id: &str) -> bool {
        if self.contains(id) {
            return false;
        }
        self.counts.insert(id.to_owned(), 1);
        self.claimed.push(id.to_owned());
        true
    }
    /// Forgets what the current command claimed; its paragraphs are counted by
    /// [`add`](Self::add) once they are in the document, or never were.
    pub(super) fn release_claims(&mut self) {
        for id in std::mem::take(&mut self.claimed) {
            self.remove_one(&id);
        }
    }
    /// The index after `before` was replaced by `after`, the current
    /// command's claims included in `after`.
    pub(super) fn settle(&mut self, before: &[Block], after: &[Block]) {
        self.release_claims();
        self.remove(before);
        self.add(after);
    }
    /// Counts the identities under `blocks`.
    pub(super) fn add(&mut self, blocks: &[Block]) {
        collect(blocks, &mut |id| *self.counts.entry(id).or_insert(0) += 1);
    }
    /// Uncounts the identities under `blocks`.
    pub(super) fn remove(&mut self, blocks: &[Block]) {
        let mut ids = Vec::new();
        collect(blocks, &mut |id| ids.push(id));
        for id in ids {
            self.remove_one(&id);
        }
    }
    fn remove_one(&mut self, id: &str) {
        if let Some(count) = self.counts.get_mut(id) {
            *count -= 1;
            if *count == 0 {
                self.counts.remove(id);
            }
        }
    }
}
/// Calls `f` with the uppercased identity of every paragraph under `blocks`,
/// in tables, block SDTs and the text boxes of drawings.
pub(super) fn collect(blocks: &[Block], f: &mut dyn FnMut(String)) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                if let Some(id) = &p.para_id {
                    f(id.as_str().to_ascii_uppercase());
                }
                visit_drawings(&p.inlines, &[], &mut |d, _, _| {
                    let graphic = match &d.kind {
                        DrawingKind::Inline(v) => v.graphic.as_ref(),
                        DrawingKind::Anchor(v) => v.graphic.as_ref(),
                        DrawingKind::Opaque(_) => return,
                    };
                    visit_boxes(graphic, &[], &mut |blocks, _| collect(blocks, f));
                });
            }
            Block::Table(t) => {
                for r in &t.rows {
                    for c in &r.cells {
                        collect(&c.blocks, f);
                    }
                }
            }
            Block::SdtBlock(s) => collect(&s.blocks, f),
            _ => {}
        }
    }
}
pub(super) fn assign_id(
    p: &mut Paragraph,
    ids: &mut IdIndex,
    fresh: bool,
) -> Result<(), EditError> {
    if !fresh && p.para_id.is_some() {
        return Ok(());
    }
    for n in 1..0x8000_0000_u32 {
        let id = format!("{n:08X}");
        if ids.claim(&id) {
            if fresh {
                // This is a logical identity for a newly synthesized paragraph,
                // not a claimed position in the input package. The renderer's
                // numbering map keys paragraphs by location, so clones must not
                // retain the original paragraph's key.
                p.location = SourceLocation::new(
                    PartId::new(format!("/__strict_edit__/paragraph_{id}")),
                    1,
                    1,
                    0,
                );
            }
            p.para_id = Some(ParaId::new(id));
            p.text_id = None;
            return Ok(());
        }
    }
    Err(EditError::LimitExceeded)
}
pub(super) fn fresh_ids(blocks: &mut [Block], ids: &mut IdIndex) -> Result<(), EditError> {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                assign_id(p, ids, true)?;
                fresh_inline_ids(&mut p.inlines, ids)?;
            }
            Block::Table(t) => {
                for r in &mut t.rows {
                    for c in &mut r.cells {
                        fresh_ids(&mut c.blocks, ids)?;
                    }
                }
            }
            Block::SdtBlock(s) => fresh_ids(&mut s.blocks, ids)?,
            _ => {}
        }
    }
    Ok(())
}
fn fresh_inline_ids(items: &mut [Inline], ids: &mut IdIndex) -> Result<(), EditError> {
    fn graphic(g: &mut Graphic, ids: &mut IdIndex) -> Result<(), EditError> {
        match g {
            Graphic::Shape(s) => {
                if let Some(t) = &mut s.text {
                    fresh_ids(&mut t.blocks, ids)?;
                }
            }
            Graphic::Group(g) => {
                for child in &mut g.children {
                    graphic(child, ids)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn drawing(d: &mut Drawing, ids: &mut IdIndex) -> Result<(), EditError> {
        match &mut d.kind {
            DrawingKind::Inline(v) => graphic(&mut v.graphic, ids),
            DrawingKind::Anchor(v) => graphic(&mut v.graphic, ids),
            DrawingKind::Opaque(_) => Ok(()),
        }
    }
    for item in items {
        match item {
            Inline::Drawing(d) => drawing(d, ids)?,
            Inline::Run(r) => {
                for c in &mut r.content {
                    if let RunContent::Drawing(d) = c {
                        drawing(d, ids)?;
                    }
                }
            }
            Inline::Hyperlink(v) => fresh_inline_ids(&mut v.inlines, ids)?,
            Inline::SdtInline(v) => fresh_inline_ids(&mut v.inlines, ids)?,
            Inline::Directional(v) => fresh_inline_ids(&mut v.inlines, ids)?,
            Inline::Field(v) => fresh_inline_ids(&mut v.inlines, ids)?,
            _ => {}
        }
    }
    Ok(())
}
