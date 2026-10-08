//! Paragraph identities (`w14:paraId`) kept unique across every story.
use crate::EditError;
use std::collections::HashSet;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Graphic, Inline, ParaId, Paragraph, RunContent,
};

use super::address::{visit_boxes, visit_drawings};

pub(super) fn all_ids(document: &Document) -> HashSet<String> {
    fn collect(blocks: &[Block], ids: &mut HashSet<String>) {
        for b in blocks {
            match b {
                Block::Paragraph(p) => {
                    if let Some(id) = &p.para_id {
                        ids.insert(id.as_str().to_ascii_uppercase());
                    }
                    visit_drawings(&p.inlines, &[], &mut |d, _, _| {
                        let graphic = match &d.kind {
                            DrawingKind::Inline(v) => v.graphic.as_ref(),
                            DrawingKind::Anchor(v) => v.graphic.as_ref(),
                            DrawingKind::Opaque(_) => return,
                        };
                        visit_boxes(graphic, &[], &mut |blocks, _| collect(blocks, ids));
                    });
                }
                Block::Table(t) => {
                    for r in &t.rows {
                        for c in &r.cells {
                            collect(&c.blocks, ids);
                        }
                    }
                }
                Block::SdtBlock(s) => collect(&s.blocks, ids),
                _ => {}
            }
        }
    }
    let mut ids = HashSet::new();
    collect(&document.body.blocks, &mut ids);
    for h in &document.headers_footers {
        collect(&h.blocks, &mut ids);
    }
    for n in document.footnotes.iter().chain(document.endnotes.iter()) {
        collect(&n.blocks, &mut ids);
    }
    ids
}
pub(super) fn assign_id(
    p: &mut Paragraph,
    ids: &mut HashSet<String>,
    fresh: bool,
) -> Result<(), EditError> {
    if !fresh && p.para_id.is_some() {
        return Ok(());
    }
    for n in 1..0x8000_0000_u32 {
        let id = format!("{n:08X}");
        if ids.insert(id.clone()) {
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
pub(super) fn fresh_ids(blocks: &mut [Block], ids: &mut HashSet<String>) -> Result<(), EditError> {
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
fn fresh_inline_ids(items: &mut [Inline], ids: &mut HashSet<String>) -> Result<(), EditError> {
    fn graphic(g: &mut Graphic, ids: &mut HashSet<String>) -> Result<(), EditError> {
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
    fn drawing(d: &mut Drawing, ids: &mut HashSet<String>) -> Result<(), EditError> {
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
