//! Footnotes and endnotes referenced more than once are split into copies.
use crate::{EditError, Invariant, Unsupported};
use std::collections::HashSet;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Graphic, Inline, Note, NoteKind, Run, RunContent,
    RunProperties, StyleId, StyleType, VertAlign,
};

use super::address::{descend_mut, mutate_story, paragraph_mut, Address, Story};
use super::ids::{fresh_ids, IdIndex};

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

/// Inserts a footnote (or an endnote) holding `blocks` under the next free
/// positive id, and a run with its reference mark at top-level inline
/// `index` of the paragraph at `at` (K03).
///
/// The note's first paragraph starts with the number marker, as Word writes
/// it. Both marks take the `FootnoteReference`/`EndnoteReference` character
/// style when the document has it, and are superscript otherwise. A note
/// cannot hold a note reference, so `at` must not be in a note.
pub(super) fn insert_note(
    document: &mut Document,
    at: &Address,
    index: usize,
    endnote: bool,
    blocks: &[Block],
    ids: &mut IdIndex,
) -> Result<(), EditError> {
    if matches!(at.story, Story::Footnote(_) | Story::Endnote(_)) {
        return Err(EditError::UnsupportedContent(Unsupported::Target));
    }
    let table = if endnote {
        &document.endnotes
    } else {
        &document.footnotes
    };
    // Ids -1 and 0 belong to the separators; normal notes count up from 1.
    let id = table
        .iter()
        .map(|note| note.id)
        .max()
        .unwrap_or(0)
        .max(0)
        .checked_add(1)
        .ok_or(EditError::LimitExceeded)?;
    let reference = u32::try_from(id).map_err(|_| EditError::LimitExceeded)?;
    let props = mark_props(document, endnote);
    let mark = |content| {
        Inline::Run(Run {
            props: props.clone(),
            content: vec![content],
            revision: None,
            location: SourceLocation::unknown(),
        })
    };
    let mut blocks = blocks.to_vec();
    if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
        blocks.push(Block::Paragraph(super::apply::empty_paragraph()));
    }
    fresh_ids(&mut blocks, ids)?;
    if let Some(Block::Paragraph(first)) = blocks
        .iter_mut()
        .find(|block| matches!(block, Block::Paragraph(_)))
    {
        first.inlines.insert(0, mark(RunContent::NoteRef));
    }
    mutate_story(document, &at.story, |root| {
        let paragraph = paragraph_mut(descend_mut(root, &at.containers)?, at.block)?;
        if index > paragraph.inlines.len() {
            return Err(EditError::InvalidRange);
        }
        paragraph.inlines.insert(
            index,
            mark(if endnote {
                RunContent::EndnoteRef(reference)
            } else {
                RunContent::FootnoteRef(reference)
            }),
        );
        Ok(())
    })?;
    let table = if endnote {
        &mut document.endnotes
    } else {
        &mut document.footnotes
    };
    table.insert(Note {
        id,
        kind: NoteKind::Normal,
        blocks,
        location: SourceLocation::unknown(),
    });
    Ok(())
}

/// The run properties of a note's reference and number marks.
fn mark_props(document: &Document, endnote: bool) -> RunProperties {
    let style = StyleId::new(if endnote {
        "EndnoteReference"
    } else {
        "FootnoteReference"
    });
    let mut props = RunProperties::default();
    if document
        .styles
        .get(&style)
        .is_some_and(|s| s.style_type == StyleType::Character)
    {
        props.style = Some(style);
    } else {
        props.vert_align = Some(VertAlign::Superscript);
    }
    props
}

/// Deletes the normal note whose story `at` names. One that a reference
/// still names is refused: remove the reference in the same transaction.
/// Deleting content never drops a note on its own.
pub(super) fn delete_note(document: &mut Document, at: &Address) -> Result<(), EditError> {
    let (id, endnote) = match at.story {
        Story::Footnote(id) => (id, false),
        Story::Endnote(id) => (id, true),
        _ => return Err(EditError::UnsupportedContent(Unsupported::Target)),
    };
    let table = if endnote {
        &document.endnotes
    } else {
        &document.footnotes
    };
    let note = table.get(id).ok_or(EditError::InvalidParagraph)?;
    if note.kind != NoteKind::Normal {
        return Err(EditError::UnsupportedContent(Unsupported::Target));
    }
    if references(document, id, endnote) > 0 {
        return Err(EditError::InvalidModel(Invariant::DefinitionInUse));
    }
    let table = if endnote {
        &mut document.endnotes
    } else {
        &mut document.footnotes
    };
    table.remove(id);
    Ok(())
}

/// How many references - inline or in a run - name note `id`, in every story.
pub(super) fn references(document: &Document, id: i32, endnote: bool) -> usize {
    let Ok(id) = u32::try_from(id) else {
        return 0;
    };
    let mut count = 0;
    let mut visit = |blocks: &[Block]| count_blocks(blocks, id, endnote, &mut count);
    visit(&document.body.blocks);
    for header in &document.headers_footers {
        visit(&header.blocks);
    }
    for note in document.footnotes.iter().chain(document.endnotes.iter()) {
        visit(&note.blocks);
    }
    count
}

fn count_blocks(blocks: &[Block], id: u32, endnote: bool, count: &mut usize) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => count_inlines(&p.inlines, id, endnote, count),
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        count_blocks(&cell.blocks, id, endnote, count);
                    }
                }
            }
            Block::SdtBlock(sdt) => count_blocks(&sdt.blocks, id, endnote, count),
            _ => {}
        }
    }
}

fn count_inlines(items: &[Inline], id: u32, endnote: bool, count: &mut usize) {
    for item in items {
        match item {
            Inline::FootnoteRef(found) => *count += usize::from(!endnote && *found == id),
            Inline::EndnoteRef(found) => *count += usize::from(endnote && *found == id),
            Inline::Run(run) => {
                for content in &run.content {
                    match content {
                        RunContent::FootnoteRef(found) => {
                            *count += usize::from(!endnote && *found == id);
                        }
                        RunContent::EndnoteRef(found) => {
                            *count += usize::from(endnote && *found == id);
                        }
                        RunContent::Drawing(d) => count_drawing(d, id, endnote, count),
                        _ => {}
                    }
                }
            }
            Inline::Drawing(d) => count_drawing(d, id, endnote, count),
            Inline::Hyperlink(v) => count_inlines(&v.inlines, id, endnote, count),
            Inline::Directional(v) => count_inlines(&v.inlines, id, endnote, count),
            Inline::Field(v) => count_inlines(&v.inlines, id, endnote, count),
            Inline::SdtInline(v) => {
                count_blocks(&v.blocks, id, endnote, count);
                count_inlines(&v.inlines, id, endnote, count);
            }
            _ => {}
        }
    }
}

fn count_drawing(drawing: &Drawing, id: u32, endnote: bool, count: &mut usize) {
    fn graphic(g: &Graphic, id: u32, endnote: bool, count: &mut usize) {
        match g {
            Graphic::Shape(shape) => {
                if let Some(text) = &shape.text {
                    count_blocks(&text.blocks, id, endnote, count);
                }
            }
            Graphic::Group(group) => {
                for child in &group.children {
                    graphic(child, id, endnote, count);
                }
            }
            _ => {}
        }
    }
    match &drawing.kind {
        DrawingKind::Inline(v) => graphic(&v.graphic, id, endnote, count),
        DrawingKind::Anchor(v) => graphic(&v.graphic, id, endnote, count),
        DrawingKind::Opaque(_) => {}
    }
}
