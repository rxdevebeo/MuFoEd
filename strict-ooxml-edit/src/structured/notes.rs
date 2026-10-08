//! Footnotes and endnotes referenced more than once are split into copies.
use crate::EditError;
use std::collections::HashSet;
use strict_ooxml_wml::model::{Block, Document, Drawing, DrawingKind, Graphic, Inline, RunContent};

/// Gives every note reference its own note.
///
/// A footnote or endnote is referenced once (`w:footnoteReference` /
/// `w:endnoteReference`); copying a paragraph that holds a reference (an
/// `Insert` of a cloned block, a row, an inline) would leave two references to
/// one note, which Word renumbers into a document whose second mark shows the
/// first note's text. Walking the body and the headers/footers in document
/// order, the first reference keeps its note and every later one gets a copy
/// of it under the next free id - what copy and paste does in Word.
pub(super) fn split_shared_notes(document: &mut Document) -> Result<(), EditError> {
    let mut split = NoteSplit {
        footnotes: std::mem::take(&mut document.footnotes),
        endnotes: std::mem::take(&mut document.endnotes),
        seen_footnotes: HashSet::new(),
        seen_endnotes: HashSet::new(),
        exhausted: false,
    };
    split.blocks(&mut document.body.blocks);
    for part in &mut document.headers_footers {
        split.blocks(&mut part.blocks);
    }
    document.footnotes = split.footnotes;
    document.endnotes = split.endnotes;
    if split.exhausted {
        Err(EditError::LimitExceeded)
    } else {
        Ok(())
    }
}
struct NoteSplit {
    footnotes: strict_ooxml_wml::model::notes::NoteTable,
    endnotes: strict_ooxml_wml::model::notes::NoteTable,
    seen_footnotes: HashSet<u32>,
    seen_endnotes: HashSet<u32>,
    exhausted: bool,
}
impl NoteSplit {
    fn blocks(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => self.inlines(&mut p.inlines),
                Block::Table(t) => {
                    for row in &mut t.rows {
                        for cell in &mut row.cells {
                            self.blocks(&mut cell.blocks);
                        }
                    }
                }
                Block::SdtBlock(sdt) => self.blocks(&mut sdt.blocks),
                _ => {}
            }
        }
    }

    fn inlines(&mut self, items: &mut [Inline]) {
        for item in items {
            match item {
                Inline::FootnoteRef(id) => self.reference(id, false),
                Inline::EndnoteRef(id) => self.reference(id, true),
                Inline::Run(run) => {
                    for content in &mut run.content {
                        match content {
                            RunContent::FootnoteRef(id) => self.reference(id, false),
                            RunContent::EndnoteRef(id) => self.reference(id, true),
                            RunContent::Drawing(d) => self.drawing(d),
                            _ => {}
                        }
                    }
                }
                Inline::Drawing(d) => self.drawing(d),
                Inline::Hyperlink(v) => self.inlines(&mut v.inlines),
                Inline::Directional(v) => self.inlines(&mut v.inlines),
                Inline::Field(v) => self.inlines(&mut v.inlines),
                Inline::SdtInline(v) => {
                    self.blocks(&mut v.blocks);
                    self.inlines(&mut v.inlines);
                }
                _ => {}
            }
        }
    }

    fn drawing(&mut self, drawing: &mut Drawing) {
        match &mut drawing.kind {
            DrawingKind::Inline(v) => self.graphic(&mut v.graphic),
            DrawingKind::Anchor(v) => self.graphic(&mut v.graphic),
            DrawingKind::Opaque(_) => {}
        }
    }

    fn graphic(&mut self, graphic: &mut Graphic) {
        match graphic {
            Graphic::Shape(shape) => {
                if let Some(text) = &mut shape.text {
                    self.blocks(&mut text.blocks);
                }
            }
            Graphic::Group(group) => {
                for child in &mut group.children {
                    self.graphic(child);
                }
            }
            _ => {}
        }
    }

    fn reference(&mut self, id: &mut u32, endnote: bool) {
        let (table, seen) = if endnote {
            (&mut self.endnotes, &mut self.seen_endnotes)
        } else {
            (&mut self.footnotes, &mut self.seen_footnotes)
        };
        if seen.insert(*id) {
            return;
        }
        // An unknown id is left for validation to refuse.
        let Some(note) = i32::try_from(*id).ok().and_then(|key| table.get(key)) else {
            return;
        };
        let next = table
            .iter()
            .map(|note| note.id)
            .max()
            .unwrap_or(0)
            .max(0)
            .checked_add(1)
            .and_then(|next| u32::try_from(next).ok().map(|unsigned| (next, unsigned)));
        let Some((next, unsigned)) = next else {
            self.exhausted = true;
            return;
        };
        let mut copy = note.clone();
        copy.id = next;
        table.insert(copy);
        seen.insert(unsigned);
        *id = unsigned;
    }
}
