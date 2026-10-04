//! Editing addresses and structural transactions across document stories.
use crate::{ChangeSet, EditError, EditLimits, FormatPatch};
use std::collections::{HashSet, VecDeque};
use std::ops::Range;
use strict_ooxml_core::{error::SourceLocation, part::PartId};
use strict_ooxml_wml::model::{
    Block, CellProperties, Document, Drawing, DrawingKind, FrameProperties, Graphic, GridCol,
    Inline, ParaId, Paragraph, ParagraphProperties, RunContent, RunProperties, TableProperties,
    TableRow,
};
/// A separately serialized document story.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Story {
    /// Main body.
    Body,
    /// Header or footer identified by its source part.
    HeaderFooter(PartId),
    /// Footnote definition id.
    Footnote(i32),
    /// Endnote definition id.
    Endnote(i32),
}
/// A descent into a block container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Container {
    /// Table block, row and cell indices.
    Cell {
        /// Table block index.
        table: usize,
        /// Row index.
        row: usize,
        /// Cell index.
        cell: usize,
    },
    /// Block SDT index.
    Sdt(usize),
    /// Shape text box, optionally inside nested graphic groups.
    TextBox {
        /// Paragraph block containing the drawing.
        paragraph: usize,
        /// Inline wrapper indices ending at a run or drawing.
        inline: Vec<usize>,
        /// Content index for a drawing inside a run.
        content: Option<usize>,
        /// Graphic group child indices.
        graphics: Vec<usize>,
    },
}
/// Revision-scoped block address, independent of physical pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    /// Document story.
    pub story: Story,
    /// Containers from outermost to innermost.
    pub containers: Vec<Container>,
    /// Index in the final block sequence.
    pub block: usize,
}
impl Address {
    /// A direct body block.
    pub fn body(block: usize) -> Self {
        Self {
            story: Story::Body,
            containers: vec![],
            block,
        }
    }
}
/// One command in a structural transaction.
#[derive(Clone, Debug)]
pub enum Edit {
    /// Split a paragraph; the right half inherits properties and the section boundary.
    Split {
        /// Target paragraph.
        at: Address,
        /// Scalar offset.
        offset: usize,
    },
    /// Join a paragraph with its immediate successor.
    Join {
        /// Left paragraph.
        at: Address,
    },
    /// Insert a block before the address (including at the sequence end).
    Insert {
        /// Insertion address.
        at: Address,
        /// New block.
        block: Box<Block>,
    },
    /// Delete a block, preserving the required final cell paragraph.
    Delete {
        /// Target block.
        at: Address,
    },
    /// Replace plain paragraph text using scalar offsets.
    Text {
        /// Target paragraph.
        at: Address,
        /// Scalar range.
        range: Range<usize>,
        /// Replacement text.
        text: String,
    },
    /// Change specified character properties on plain paragraph text.
    Format {
        /// Target paragraph.
        at: Address,
        /// Scalar range.
        range: Range<usize>,
        /// Direct property patch.
        patch: FormatPatch,
    },
    /// Replace paragraph properties; section boundaries cannot be changed here.
    ParagraphProperties {
        /// Target paragraph.
        at: Address,
        /// Complete direct properties.
        properties: Box<ParagraphProperties>,
    },
    /// Set or clear a text frame.
    Frame {
        /// Target paragraph.
        at: Address,
        /// New frame properties.
        frame: Option<FrameProperties>,
    },
    /// Replace one run's direct properties without flattening wrappers.
    RunProperties {
        /// Target paragraph.
        at: Address,
        /// Inline indices through hyperlink, SDT or directional wrappers.
        inline: Vec<usize>,
        /// Complete direct properties.
        properties: Box<RunProperties>,
    },
    /// Replace a scalar range inside one text node, preserving all other content.
    TextNode {
        /// Target paragraph.
        at: Address,
        /// Inline indices ending at the containing run.
        inline: Vec<usize>,
        /// Run content index.
        content: usize,
        /// Scalar range within the text node.
        range: Range<usize>,
        /// Replacement text.
        text: String,
    },
    /// Insert a typed inline (tabs, breaks, drawings and wrappers remain structured).
    InsertInline {
        /// Target paragraph.
        at: Address,
        /// Top-level inline insertion index.
        index: usize,
        /// New inline.
        inline: Box<Inline>,
    },
    /// Delete one inline; validation refuses dangling bookmark/field boundaries.
    DeleteInline {
        /// Target paragraph.
        at: Address,
        /// Top-level inline index.
        index: usize,
    },
    /// Replace table properties without changing its contents.
    TableProperties {
        /// Target table.
        at: Address,
        /// Complete direct properties.
        properties: Box<TableProperties>,
    },
    /// Replace cell properties; merge topology is validated atomically.
    CellProperties {
        /// Target table.
        at: Address,
        /// Row index.
        row: usize,
        /// Cell index.
        cell: usize,
        /// Complete direct properties.
        properties: Box<CellProperties>,
    },
    /// Set the table grid.
    Grid {
        /// Target table.
        at: Address,
        /// New columns.
        columns: Vec<GridCol>,
    },
    /// Insert a row with fresh paragraph identities.
    InsertRow {
        /// Target table.
        at: Address,
        /// Insertion index.
        index: usize,
        /// New row.
        row: Box<TableRow>,
    },
    /// Delete one table row; an empty table is rejected.
    DeleteRow {
        /// Target table.
        at: Address,
        /// Row index.
        index: usize,
    },
    /// Replace a drawing's modeled geometry/payload, keeping source bytes external.
    Drawing {
        /// Target paragraph.
        at: Address,
        /// Inline wrapper indices.
        inline: Vec<usize>,
        /// Run content index, or None for `Inline::Drawing`.
        content: Option<usize>,
        /// New drawing.
        drawing: Box<Drawing>,
    },
    /// Give a paragraph a stable session identity if it has none.
    Identify {
        /// Target paragraph.
        at: Address,
    },
}
/// Structural editor using the same live WML document as `EditSession`.
pub struct Editor<'a> {
    pub(crate) document: &'a mut Document,
    revision: u64,
    limits: EditLimits,
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
    history_byte_limit: usize,
    pub(crate) support_stale: bool,
    pub(crate) input_pipeline: strict_ooxml_core::pipeline::PipelineSummary,
}
struct Snapshot {
    before: Document,
    after: Document,
    bytes: usize,
}
impl<'a> Editor<'a> {
    /// Opens an exclusive session.
    pub fn new(document: &'a mut Document, limits: EditLimits) -> Result<Self, EditError> {
        validate(document)?;
        Ok(Self {
            document,
            revision: 0,
            limits,
            undo: VecDeque::new(),
            redo: vec![],
            history_byte_limit: 64 * 1024 * 1024,
            support_stale: false,
            input_pipeline: strict_ooxml_core::pipeline::PipelineSummary::new(),
        })
    }
    /// Reads the current document.
    pub fn document(&self) -> &Document {
        self.document
    }
    /// Current revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Whether support metadata must be refreshed before reporting.
    pub fn support_is_stale(&self) -> bool {
        self.support_stale
    }
    /// Carries previous normalization/conversion issues into every save result.
    #[must_use]
    pub fn with_pipeline(mut self, pipeline: strict_ooxml_core::pipeline::PipelineSummary) -> Self {
        self.input_pipeline = pipeline;
        self
    }
    /// Caps retained snapshot metadata/text in bytes (default 64 MiB).
    /// A transaction exceeding this cap is refused before changing the document.
    /// The estimate is the UTF-8 debug serialization of both model snapshots;
    /// media bytes remain external. Older undo states are evicted first.
    #[must_use]
    pub fn with_history_byte_limit(mut self, bytes: usize) -> Self {
        self.history_byte_limit = bytes;
        self.trim_history();
        self
    }
    /// Explicitly validates the current model and all editable stories.
    pub fn validate(&self) -> Result<(), EditError> {
        validate(self.document)
    }
    /// Reads a paragraph without bypassing the command protocol.
    pub fn paragraph(&self, at: &Address) -> Result<&Paragraph, EditError> {
        read_paragraph(self.document, at)
    }
    /// Lists editable paragraph addresses, including table, SDT and shape text boxes.
    pub fn paragraphs(&self, story: &Story) -> Result<Vec<Address>, EditError> {
        addresses(self.document, story)
    }
    /// Finds an identified paragraph in a story, after structural changes.
    pub fn find(&self, story: &Story, id: &ParaId) -> Result<Address, EditError> {
        addresses(self.document, story)?
            .into_iter()
            .find(|a| {
                read_paragraph(self.document, a).is_ok_and(|p| {
                    p.para_id
                        .as_ref()
                        .is_some_and(|v| v.as_str().eq_ignore_ascii_case(id.as_str()))
                })
            })
            .ok_or(EditError::InvalidParagraph)
    }
    /// Executes an atomic command batch.
    pub fn transact(&mut self, revision: u64, commands: &[Edit]) -> Result<ChangeSet, EditError> {
        self.check(revision)?;
        let mut candidate = self.document.clone();
        for command in commands {
            apply(&mut candidate, command, self.limits)?;
        }
        validate(&candidate)?;
        if same_content(self.document, &candidate) {
            return Ok(self.changes(false));
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(EditError::LimitExceeded)?;
        if self.limits.history_transactions > 0 {
            let bytes = snapshot_size(self.document).saturating_add(snapshot_size(&candidate));
            if bytes > self.history_byte_limit {
                return Err(EditError::LimitExceeded);
            }
            self.undo.push_back(Snapshot {
                before: self.document.clone(),
                after: candidate.clone(),
                bytes,
            });
            while self.undo.len() > self.limits.history_transactions {
                self.undo.pop_front();
            }
        }
        *self.document = candidate;
        self.redo.clear();
        self.trim_history();
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
    }
    /// Undo one transaction.
    pub fn undo(&mut self, revision: u64) -> Result<ChangeSet, EditError> {
        self.check(revision)?;
        if self.undo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(EditError::LimitExceeded)?;
        let change = self.undo.pop_back().ok_or(EditError::EmptyHistory)?;
        *self.document = change.before.clone();
        self.redo.push(change);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
    }
    /// Redo one transaction.
    pub fn redo(&mut self, revision: u64) -> Result<ChangeSet, EditError> {
        self.check(revision)?;
        if self.redo.is_empty() {
            return Err(EditError::EmptyHistory);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(EditError::LimitExceeded)?;
        let change = self.redo.pop().ok_or(EditError::EmptyHistory)?;
        *self.document = change.after.clone();
        self.undo.push_back(change);
        self.revision = next;
        self.support_stale = true;
        Ok(self.changes(true))
    }
    fn check(&self, revision: u64) -> Result<(), EditError> {
        if self.revision == revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision)
        }
    }
    fn trim_history(&mut self) {
        while self
            .undo
            .iter()
            .chain(&self.redo)
            .map(|snapshot| snapshot.bytes)
            .fold(0_usize, usize::saturating_add)
            > self.history_byte_limit
        {
            if self.undo.pop_front().is_none() {
                if self.redo.is_empty() {
                    break;
                }
                self.redo.remove(0);
            }
        }
    }
    fn changes(&self, changed: bool) -> ChangeSet {
        ChangeSet {
            revision: self.revision,
            paragraphs: if changed {
                (0..self.document.body.blocks.len()).collect()
            } else {
                vec![]
            },
            invalidate_layout: changed,
            invalidate_support: changed,
        }
    }
}
fn snapshot_size(document: &Document) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0 = self.0.saturating_add(s.len());
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = std::fmt::write(&mut count, format_args!("{document:?}"));
    count.0
}

fn same_content(a: &Document, b: &Document) -> bool {
    a.body == b.body
        && a.headers_footers == b.headers_footers
        && a.sections == b.sections
        && a.footnotes.iter().eq(b.footnotes.iter())
        && a.endnotes.iter().eq(b.endnotes.iter())
}
fn story_blocks<'a>(document: &'a Document, story: &Story) -> Result<&'a [Block], EditError> {
    Ok(match story {
        Story::Body => &document.body.blocks,
        Story::HeaderFooter(part) => {
            &document
                .header_footer(part)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Footnote(id) => {
            &document
                .footnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Endnote(id) => {
            &document
                .endnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
    })
}
fn mutate_story<F>(document: &mut Document, story: &Story, edit: F) -> Result<(), EditError>
where
    F: FnOnce(&mut Vec<Block>) -> Result<(), EditError>,
{
    match story {
        Story::Body => edit(&mut document.body.blocks),
        Story::HeaderFooter(part) => edit(
            &mut document
                .headers_footers
                .iter_mut()
                .find(|h| &h.part == part)
                .ok_or(EditError::InvalidParagraph)?
                .blocks,
        ),
        Story::Footnote(id) | Story::Endnote(id) => {
            let table = if matches!(story, Story::Footnote(_)) {
                &mut document.footnotes
            } else {
                &mut document.endnotes
            };
            let mut note = table.get(*id).ok_or(EditError::InvalidParagraph)?.clone();
            edit(&mut note.blocks)?;
            table.insert(note);
            Ok(())
        }
    }
}
fn descend_mut<'a>(
    blocks: &'a mut Vec<Block>,
    containers: &[Container],
) -> Result<&'a mut Vec<Block>, EditError> {
    let Some((first, rest)) = containers.split_first() else {
        return Ok(blocks);
    };
    let child = match first {
        Container::Cell { table, row, cell } => {
            let Some(Block::Table(t)) = blocks.get_mut(*table) else {
                return Err(EditError::InvalidParagraph);
            };
            &mut t
                .rows
                .get_mut(*row)
                .and_then(|r| r.cells.get_mut(*cell))
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Container::Sdt(index) => {
            let Some(Block::SdtBlock(s)) = blocks.get_mut(*index) else {
                return Err(EditError::InvalidParagraph);
            };
            &mut s.blocks
        }
        Container::TextBox {
            paragraph,
            inline,
            content,
            graphics,
        } => {
            let p = paragraph_mut(blocks, *paragraph)?;
            let drawing = drawing_mut(&mut p.inlines, inline, *content)?;
            let graphic = match &mut drawing.kind {
                DrawingKind::Inline(v) => v.graphic.as_mut(),
                DrawingKind::Anchor(v) => v.graphic.as_mut(),
                DrawingKind::Opaque(_) => return Err(EditError::UnsupportedContent),
            };
            &mut shape_mut(graphic, graphics)?
                .text
                .as_mut()
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
    };
    descend_mut(child, rest)
}
fn shape_mut<'a>(
    graphic: &'a mut Graphic,
    path: &[usize],
) -> Result<&'a mut strict_ooxml_wml::model::Shape, EditError> {
    if let Some((index, rest)) = path.split_first() {
        let Graphic::Group(g) = graphic else {
            return Err(EditError::InvalidParagraph);
        };
        shape_mut(
            g.children
                .get_mut(*index)
                .ok_or(EditError::InvalidParagraph)?,
            rest,
        )
    } else {
        match graphic {
            Graphic::Shape(s) => Ok(s),
            _ => Err(EditError::InvalidParagraph),
        }
    }
}
fn paragraph_mut(blocks: &mut [Block], index: usize) -> Result<&mut Paragraph, EditError> {
    match blocks.get_mut(index) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
    }
}
fn inline_mut<'a>(inlines: &'a mut [Inline], path: &[usize]) -> Result<&'a mut Inline, EditError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let value = inlines.get_mut(*index).ok_or(EditError::InvalidParagraph)?;
    if rest.is_empty() {
        return Ok(value);
    }
    let child = match value {
        Inline::Hyperlink(v) => &mut v.inlines,
        Inline::SdtInline(v) => &mut v.inlines,
        Inline::Directional(v) => &mut v.inlines,
        _ => return Err(EditError::UnsupportedContent),
    };
    inline_mut(child, rest)
}
fn run_mut<'a>(
    inlines: &'a mut [Inline],
    path: &[usize],
) -> Result<&'a mut strict_ooxml_wml::model::Run, EditError> {
    match inline_mut(inlines, path)? {
        Inline::Run(r) if r.revision.is_none() => Ok(r),
        _ => Err(EditError::UnsupportedContent),
    }
}
fn drawing_mut<'a>(
    inlines: &'a mut [Inline],
    path: &[usize],
    content: Option<usize>,
) -> Result<&'a mut Drawing, EditError> {
    match (inline_mut(inlines, path)?, content) {
        (Inline::Drawing(d), None) => Ok(d),
        (Inline::Run(r), Some(i)) if r.revision.is_none() => match r.content.get_mut(i) {
            Some(RunContent::Drawing(d)) => Ok(d),
            _ => Err(EditError::InvalidParagraph),
        },
        _ => Err(EditError::InvalidParagraph),
    }
}
fn empty_paragraph() -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![],
        rsids: strict_ooxml_wml::model::Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::unknown(),
    }
}
pub(crate) fn read_paragraph<'a>(
    document: &'a Document,
    address: &Address,
) -> Result<&'a Paragraph, EditError> {
    // A read-only traversal mirrors the mutating traversal, including text boxes.
    let mut blocks = story_blocks(document, &address.story)?;
    for container in &address.containers {
        blocks = match container {
            Container::Cell { table, row, cell } => match blocks.get(*table) {
                Some(Block::Table(t)) => {
                    &t.rows
                        .get(*row)
                        .and_then(|r| r.cells.get(*cell))
                        .ok_or(EditError::InvalidParagraph)?
                        .blocks
                }
                _ => return Err(EditError::InvalidParagraph),
            },
            Container::Sdt(i) => match blocks.get(*i) {
                Some(Block::SdtBlock(s)) => &s.blocks,
                _ => return Err(EditError::InvalidParagraph),
            },
            Container::TextBox {
                paragraph,
                inline,
                content,
                graphics,
            } => {
                let Some(Block::Paragraph(p)) = blocks.get(*paragraph) else {
                    return Err(EditError::InvalidParagraph);
                };
                let drawing = drawing_ref(&p.inlines, inline, *content)?;
                let graphic = match &drawing.kind {
                    DrawingKind::Inline(v) => v.graphic.as_ref(),
                    DrawingKind::Anchor(v) => v.graphic.as_ref(),
                    DrawingKind::Opaque(_) => return Err(EditError::UnsupportedContent),
                };
                &shape_ref(graphic, graphics)?
                    .text
                    .as_ref()
                    .ok_or(EditError::InvalidParagraph)?
                    .blocks
            }
        };
    }
    match blocks.get(address.block) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
    }
}
fn inline_ref<'a>(items: &'a [Inline], path: &[usize]) -> Result<&'a Inline, EditError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let item = items.get(*index).ok_or(EditError::InvalidParagraph)?;
    if rest.is_empty() {
        return Ok(item);
    }
    let children = match item {
        Inline::Hyperlink(v) => &v.inlines,
        Inline::SdtInline(v) => &v.inlines,
        Inline::Directional(v) => &v.inlines,
        _ => return Err(EditError::UnsupportedContent),
    };
    inline_ref(children, rest)
}
fn drawing_ref<'a>(
    items: &'a [Inline],
    path: &[usize],
    content: Option<usize>,
) -> Result<&'a Drawing, EditError> {
    match (inline_ref(items, path)?, content) {
        (Inline::Drawing(v), None) => Ok(v),
        (Inline::Run(r), Some(index)) => match r.content.get(index) {
            Some(RunContent::Drawing(v)) => Ok(v),
            _ => Err(EditError::InvalidParagraph),
        },
        _ => Err(EditError::InvalidParagraph),
    }
}
fn shape_ref<'a>(
    graphic: &'a Graphic,
    path: &[usize],
) -> Result<&'a strict_ooxml_wml::model::Shape, EditError> {
    if let Some((index, rest)) = path.split_first() {
        let Graphic::Group(g) = graphic else {
            return Err(EditError::InvalidParagraph);
        };
        shape_ref(
            g.children.get(*index).ok_or(EditError::InvalidParagraph)?,
            rest,
        )
    } else {
        match graphic {
            Graphic::Shape(s) => Ok(s),
            _ => Err(EditError::InvalidParagraph),
        }
    }
}
/// Lists ordinary, table and SDT paragraphs in a story in document order.
pub(crate) fn addresses(document: &Document, story: &Story) -> Result<Vec<Address>, EditError> {
    fn walk(blocks: &[Block], story: &Story, parents: &[Container], out: &mut Vec<Address>) {
        for (index, block) in blocks.iter().enumerate() {
            match block {
                Block::Paragraph(para) => {
                    out.push(Address {
                        story: story.clone(),
                        containers: parents.to_vec(),
                        block: index,
                    });
                    visit_drawings(&para.inlines, &[], &mut |drawing, inline, content| {
                        let graphic = match &drawing.kind {
                            DrawingKind::Inline(v) => v.graphic.as_ref(),
                            DrawingKind::Anchor(v) => v.graphic.as_ref(),
                            DrawingKind::Opaque(_) => return,
                        };
                        visit_boxes(graphic, &[], &mut |blocks, graphics| {
                            let mut path = parents.to_vec();
                            path.push(Container::TextBox {
                                paragraph: index,
                                inline: inline.to_vec(),
                                content,
                                graphics: graphics.to_vec(),
                            });
                            walk(blocks, story, &path, out);
                        });
                    });
                }
                Block::Table(t) => {
                    for (row, r) in t.rows.iter().enumerate() {
                        for (cell, c) in r.cells.iter().enumerate() {
                            let mut p = parents.to_vec();
                            p.push(Container::Cell {
                                table: index,
                                row,
                                cell,
                            });
                            walk(&c.blocks, story, &p, out);
                        }
                    }
                }
                Block::SdtBlock(s) => {
                    let mut p = parents.to_vec();
                    p.push(Container::Sdt(index));
                    walk(&s.blocks, story, &p, out);
                }
                _ => {}
            }
        }
    }
    let mut out = vec![];
    walk(story_blocks(document, story)?, story, &[], &mut out);
    Ok(out)
}
fn visit_drawings(
    items: &[Inline],
    parents: &[usize],
    f: &mut impl FnMut(&Drawing, &[usize], Option<usize>),
) {
    for (index, item) in items.iter().enumerate() {
        let mut path = parents.to_vec();
        path.push(index);
        match item {
            Inline::Drawing(v) => f(v, &path, None),
            Inline::Run(r) => {
                for (content, c) in r.content.iter().enumerate() {
                    if let RunContent::Drawing(v) = c {
                        f(v, &path, Some(content));
                    }
                }
            }
            Inline::Hyperlink(v) => visit_drawings(&v.inlines, &path, f),
            Inline::SdtInline(v) => visit_drawings(&v.inlines, &path, f),
            Inline::Directional(v) => visit_drawings(&v.inlines, &path, f),
            _ => {}
        }
    }
}
fn visit_boxes(graphic: &Graphic, parents: &[usize], f: &mut impl FnMut(&[Block], &[usize])) {
    match graphic {
        Graphic::Shape(s) => {
            if let Some(t) = &s.text {
                f(&t.blocks, parents);
            }
        }
        Graphic::Group(g) => {
            for (index, child) in g.children.iter().enumerate() {
                let mut path = parents.to_vec();
                path.push(index);
                visit_boxes(child, &path, f);
            }
        }
        _ => {}
    }
}
fn all_ids(document: &Document) -> HashSet<String> {
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
fn assign_id(p: &mut Paragraph, ids: &mut HashSet<String>, fresh: bool) -> Result<(), EditError> {
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
fn fresh_ids(blocks: &mut [Block], ids: &mut HashSet<String>) -> Result<(), EditError> {
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
fn valid_text(text: &str) -> Result<(), EditError> {
    if text
        .chars()
        .any(|c| !strict_ooxml_core::xml::escape::is_xml_char(c) || matches!(c, '\r' | '\n' | '\t'))
    {
        Err(EditError::InvalidText)
    } else {
        Ok(())
    }
}
fn check_range(total: usize, range: &Range<usize>) -> Result<(), EditError> {
    if range.start > range.end || range.end > total {
        Err(EditError::InvalidRange)
    } else {
        Ok(())
    }
}
#[allow(clippy::too_many_lines)]
fn apply(document: &mut Document, edit: &Edit, limits: EditLimits) -> Result<(), EditError> {
    let mut ids = all_ids(document);
    let at = match edit {
        Edit::Split { at, .. }
        | Edit::Join { at }
        | Edit::Insert { at, .. }
        | Edit::Delete { at }
        | Edit::Text { at, .. }
        | Edit::Format { at, .. }
        | Edit::ParagraphProperties { at, .. }
        | Edit::Frame { at, .. }
        | Edit::RunProperties { at, .. }
        | Edit::TextNode { at, .. }
        | Edit::InsertInline { at, .. }
        | Edit::DeleteInline { at, .. }
        | Edit::TableProperties { at, .. }
        | Edit::CellProperties { at, .. }
        | Edit::Grid { at, .. }
        | Edit::InsertRow { at, .. }
        | Edit::DeleteRow { at, .. }
        | Edit::Drawing { at, .. }
        | Edit::Identify { at } => at,
    };
    mutate_story(document, &at.story, |root| {
        let blocks = descend_mut(root, &at.containers)?;
        match edit {
            Edit::Insert { block, .. } => {
                if at.block > blocks.len() {
                    return Err(EditError::InvalidParagraph);
                }
                let mut b = *block.clone();
                fresh_ids(std::slice::from_mut(&mut b), &mut ids)?;
                blocks.insert(at.block, b);
            }
            Edit::Delete { .. } => {
                let b = blocks.get(at.block).ok_or(EditError::InvalidParagraph)?;
                if contains_boundary(b) {
                    return Err(EditError::InvalidModel);
                }
                blocks.remove(at.block);
                if blocks.is_empty() && at.containers.is_empty() && matches!(at.story, Story::Body)
                {
                    let mut p = empty_paragraph();
                    assign_id(&mut p, &mut ids, true)?;
                    blocks.push(Block::Paragraph(p));
                }
            }
            Edit::Split { offset, .. } => {
                let p = paragraph_mut(blocks, at.block)?;
                let runs = super::plain_runs(p, limits.paragraph_scalars)?;
                let count = runs.iter().map(|(_, t)| t.chars().count()).sum();
                check_range(count, &(*offset..*offset))?;
                assign_id(p, &mut ids, false)?;
                let mut right = p.clone();
                assign_id(&mut right, &mut ids, true)?;
                super::replace_text(p, &runs, &(*offset..count), "");
                super::replace_text(&mut right, &runs, &(0..*offset), "");
                p.props.section = None;
                blocks.insert(at.block + 1, Block::Paragraph(right));
            }
            Edit::Join { .. } => {
                let mut left = paragraph_mut(blocks, at.block)?.clone();
                let right = paragraph_mut(blocks, at.block + 1)?.clone();
                if left.props.section.is_some() {
                    return Err(EditError::InvalidModel);
                }
                let a = super::plain_runs(&left, limits.paragraph_scalars)?;
                let b = super::plain_runs(&right, limits.paragraph_scalars)?;
                if a.iter()
                    .chain(&b)
                    .map(|(_, t)| t.chars().count())
                    .sum::<usize>()
                    > limits.paragraph_scalars
                {
                    return Err(EditError::LimitExceeded);
                }
                left.inlines.extend(right.inlines);
                left.props.section = right.props.section;
                blocks[at.block] = Block::Paragraph(left);
                blocks.remove(at.block + 1);
            }
            Edit::Text { range, .. } | Edit::Format { range, .. } => {
                let p = paragraph_mut(blocks, at.block)?;
                let runs = super::plain_runs(p, limits.paragraph_scalars)?;
                let total = runs.iter().map(|(_, t)| t.chars().count()).sum();
                check_range(total, range)?;
                match edit {
                    Edit::Text { text, .. } => {
                        valid_text(text)?;
                        if total - (range.end - range.start) + text.chars().count()
                            > limits.paragraph_scalars
                        {
                            return Err(EditError::LimitExceeded);
                        }
                        super::replace_text(p, &runs, range, text);
                    }
                    Edit::Format { patch, .. } => super::format_text(p, &runs, range, patch),
                    _ => unreachable!(),
                }
            }
            Edit::ParagraphProperties { properties, .. } => {
                let p = paragraph_mut(blocks, at.block)?;
                if properties.section != p.props.section {
                    return Err(EditError::InvalidModel);
                }
                p.props = *properties.clone();
            }
            Edit::Frame { frame, .. } => paragraph_mut(blocks, at.block)?
                .props
                .frame
                .clone_from(frame),
            Edit::RunProperties {
                inline, properties, ..
            } => {
                let p = paragraph_mut(blocks, at.block)?;
                if p.revision.is_some() {
                    return Err(EditError::UnsupportedContent);
                }
                run_mut(&mut p.inlines, inline)?.props = *properties.clone();
            }
            Edit::TextNode {
                inline,
                content,
                range,
                text,
                ..
            } => {
                valid_text(text)?;
                let p = paragraph_mut(blocks, at.block)?;
                if p.revision.is_some() {
                    return Err(EditError::UnsupportedContent);
                }
                if complex_fields(&p.inlines) {
                    return Err(EditError::UnsupportedContent);
                }
                let paragraph_count = text_count(&p.inlines);
                let run = run_mut(&mut p.inlines, inline)?;
                // Cached complex field results must not be edited as ordinary text.
                if run
                    .content
                    .iter()
                    .any(|c| matches!(c, RunContent::InstrText(_) | RunContent::FieldChar(_)))
                {
                    return Err(EditError::UnsupportedContent);
                }
                let Some(RunContent::Text(node)) = run.content.get_mut(*content) else {
                    return Err(EditError::InvalidParagraph);
                };
                let total = node.text.chars().count();
                check_range(total, range)?;
                if paragraph_count - (range.end - range.start) + text.chars().count()
                    > limits.paragraph_scalars
                {
                    return Err(EditError::LimitExceeded);
                }
                if super::slice(&node.text, range.start, range.end) == *text {
                    return Ok(());
                }
                node.text = format!(
                    "{}{}{}",
                    super::slice(&node.text, 0, range.start),
                    text,
                    super::slice(&node.text, range.end, total)
                );
                node.space = strict_ooxml_wml::model::Space::Preserve;
            }
            Edit::InsertInline { index, inline, .. } => {
                let p = paragraph_mut(blocks, at.block)?;
                if p.revision.is_some() {
                    return Err(EditError::UnsupportedContent);
                }
                if *index > p.inlines.len() {
                    return Err(EditError::InvalidRange);
                }
                p.inlines.insert(*index, *inline.clone());
                if text_count(&p.inlines) > limits.paragraph_scalars {
                    return Err(EditError::LimitExceeded);
                }
            }
            Edit::DeleteInline { index, .. } => {
                let p = paragraph_mut(blocks, at.block)?;
                if p.revision.is_some() {
                    return Err(EditError::UnsupportedContent);
                }
                if *index >= p.inlines.len() {
                    return Err(EditError::InvalidRange);
                }
                p.inlines.remove(*index);
            }
            Edit::Drawing {
                inline,
                content,
                drawing,
                ..
            } => {
                let p = paragraph_mut(blocks, at.block)?;
                *drawing_mut(&mut p.inlines, inline, *content)? = *drawing.clone();
            }
            Edit::Identify { .. } => assign_id(paragraph_mut(blocks, at.block)?, &mut ids, false)?,
            _ => {
                let Some(Block::Table(t)) = blocks.get_mut(at.block) else {
                    return Err(EditError::InvalidParagraph);
                };
                match edit {
                    Edit::TableProperties { properties, .. } => t.props = *properties.clone(),
                    Edit::CellProperties {
                        row,
                        cell,
                        properties,
                        ..
                    } => {
                        t.rows
                            .get_mut(*row)
                            .and_then(|r| r.cells.get_mut(*cell))
                            .ok_or(EditError::InvalidParagraph)?
                            .props = *properties.clone();
                    }
                    Edit::Grid { columns, .. } => t.grid.clone_from(columns),
                    Edit::InsertRow { index, row, .. } => {
                        if *index > t.rows.len() {
                            return Err(EditError::InvalidRange);
                        }
                        let mut row = *row.clone();
                        for c in &mut row.cells {
                            fresh_ids(&mut c.blocks, &mut ids)?;
                        }
                        t.rows.insert(*index, row);
                    }
                    Edit::DeleteRow { index, .. } => {
                        if *index >= t.rows.len() {
                            return Err(EditError::InvalidRange);
                        }
                        t.rows.remove(*index);
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    })
}
fn text_count(items: &[Inline]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Inline::Run(r) => r
                .content
                .iter()
                .filter_map(|c| {
                    if let RunContent::Text(t) = c {
                        Some(t.text.chars().count())
                    } else {
                        None
                    }
                })
                .sum(),
            Inline::Hyperlink(v) => text_count(&v.inlines),
            Inline::Field(v) => text_count(&v.inlines),
            Inline::SdtInline(v) => text_count(&v.inlines),
            Inline::Directional(v) => text_count(&v.inlines),
            _ => 0,
        })
        .sum()
}
pub(crate) fn complex_fields(items: &[Inline]) -> bool {
    items.iter().any(|i| match i {
        Inline::Run(r) => r
            .content
            .iter()
            .any(|c| matches!(c, RunContent::FieldChar(_) | RunContent::InstrText(_))),
        Inline::Hyperlink(v) => complex_fields(&v.inlines),
        Inline::SdtInline(v) => complex_fields(&v.inlines),
        Inline::Directional(v) => complex_fields(&v.inlines),
        _ => false,
    })
}
fn contains_boundary(b: &Block) -> bool {
    match b {
        Block::Paragraph(p) => p.props.section.is_some(),
        Block::Table(t) => t.rows.iter().any(|r| {
            r.cells
                .iter()
                .any(|c| c.blocks.iter().any(contains_boundary))
        }),
        Block::SdtBlock(s) => s.blocks.iter().any(contains_boundary),
        _ => false,
    }
}
pub(crate) fn validate(document: &Document) -> Result<(), EditError> {
    super::validate_body(document)?;
    strict_ooxml_wml::nesting::check_document(
        document,
        &strict_ooxml_core::limits::ResourceLimits::default(),
    )
    .map_err(|_| EditError::LimitExceeded)?;
    validate_global_references(document)?;
    for story in std::iter::once(&document.body.blocks)
        .chain(document.headers_footers.iter().map(|h| &h.blocks))
        .chain(
            document
                .footnotes
                .iter()
                .chain(document.endnotes.iter())
                .map(|n| &n.blocks),
        )
    {
        super::validate_blocks(story, document, &mut HashSet::new(), 0)?;
        validate_structure(story, document)?;
        validate_boundaries(story)?;
    }
    let mut stories = vec![Story::Body];
    stories.extend(
        document
            .headers_footers
            .iter()
            .map(|h| Story::HeaderFooter(h.part.clone())),
    );
    stories.extend(document.footnotes.iter().map(|n| Story::Footnote(n.id)));
    stories.extend(document.endnotes.iter().map(|n| Story::Endnote(n.id)));
    for story in stories {
        let mut ids = HashSet::new();
        let mut text_ids = HashSet::new();
        let mut numbered_locations = HashSet::new();
        for address in addresses(document, &story)? {
            let p = read_paragraph(document, &address)?;
            if p.para_id
                .as_ref()
                .is_some_and(|id| !ids.insert(id.as_str().to_ascii_uppercase()))
            {
                return Err(EditError::InvalidModel);
            }
            if p.text_id.as_ref().is_some_and(|id| {
                p.para_id.is_none() || !text_ids.insert(id.as_str().to_ascii_uppercase())
            }) {
                return Err(EditError::InvalidModel);
            }
            if p.props.numbering.is_some() && !numbered_locations.insert(p.location.clone()) {
                return Err(EditError::InvalidModel);
            }
            if let Some(frame) = &p.props.frame {
                if frame.width.is_some_and(|v| v.value() < 0)
                    || frame.height.is_some_and(|v| v.value() < 0)
                {
                    return Err(EditError::InvalidModel);
                }
            }
        }
    }
    Ok(())
}
fn validate_global_references(document: &Document) -> Result<(), EditError> {
    for style in document.styles.iter() {
        let mut ancestors = vec![];
        let mut parent = style.based_on.as_ref();
        while let Some(id) = parent {
            if id == &style.id || ancestors.contains(id) || ancestors.len() >= 64 {
                return Err(EditError::InvalidModel);
            }
            let definition = document.styles.get(id).ok_or(EditError::UnknownStyle)?;
            ancestors.push(id.clone());
            parent = definition.based_on.as_ref();
        }
        if ancestors != style.based_on_chain {
            return Err(EditError::InvalidModel);
        }
        if style
            .next
            .as_ref()
            .is_some_and(|id| document.styles.get(id).is_none())
            || style
                .link
                .as_ref()
                .is_some_and(|id| document.styles.get(id).is_none())
        {
            return Err(EditError::UnknownStyle);
        }
    }
    for number in document.numbering.nums() {
        if document
            .numbering
            .abstract_num(number.abstract_num_id)
            .is_none()
            || number.overrides.iter().any(|v| !v.ilvl.is_valid())
        {
            return Err(EditError::InvalidModel);
        }
    }
    for section in &document.sections {
        for (references, header) in [
            (&section.properties.headers, true),
            (&section.properties.footers, false),
        ] {
            for reference in references {
                if reference
                    .part
                    .as_ref()
                    .and_then(|part| document.header_footer(part))
                    .is_none_or(|part| part.is_header != header)
                {
                    return Err(EditError::InvalidModel);
                }
            }
        }
    }
    Ok(())
}
fn validate_structure(blocks: &[Block], document: &Document) -> Result<(), EditError> {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                if p.props.style.as_ref().is_some_and(|id| {
                    !document.styles.get(id).is_some_and(|s| {
                        s.style_type == strict_ooxml_wml::model::StyleType::Paragraph
                    })
                }) {
                    return Err(EditError::UnknownStyle);
                }
                validate_inlines(&p.inlines, document)?;
                if let Some(n) = p.props.numbering {
                    if n.ilvl.is_some_and(|v| !v.is_valid())
                        || n.num_id
                            .is_some_and(|id| id.0 != 0 && document.numbering.num(id).is_none())
                    {
                        return Err(EditError::InvalidModel);
                    }
                }
            }
            Block::Table(t) => {
                if t.rows.is_empty()
                    || t.grid
                        .iter()
                        .any(|g| g.width.is_some_and(|v| v.value() <= 0))
                {
                    return Err(EditError::InvalidModel);
                }
                if t.props.style.as_ref().is_some_and(|id| {
                    !document
                        .styles
                        .get(id)
                        .is_some_and(|s| s.style_type == strict_ooxml_wml::model::StyleType::Table)
                }) {
                    return Err(EditError::UnknownStyle);
                }
                let mut previous = std::collections::BTreeSet::new();
                for r in &t.rows {
                    if r.cells.is_empty() {
                        return Err(EditError::InvalidModel);
                    }
                    let mut column = usize::try_from(r.props.grid_before.unwrap_or(0))
                        .map_err(|_| EditError::InvalidModel)?;
                    let mut next = std::collections::BTreeSet::new();
                    for c in &r.cells {
                        let span = usize::from(c.props.grid_span.unwrap_or(1));
                        if span == 0 || !matches!(c.blocks.last(), Some(Block::Paragraph(_))) {
                            return Err(EditError::InvalidModel);
                        }
                        let region = (column, span);
                        match c.props.vertical_merge {
                            Some(strict_ooxml_wml::model::VerticalMerge::Continue) => {
                                if !previous.contains(&region) {
                                    return Err(EditError::InvalidModel);
                                }
                                next.insert(region);
                            }
                            Some(strict_ooxml_wml::model::VerticalMerge::Restart) => {
                                next.insert(region);
                            }
                            None => {}
                        }
                        column += span;
                        validate_structure(&c.blocks, document)?;
                    }
                    let end = column
                        + usize::try_from(r.props.grid_after.unwrap_or(0))
                            .map_err(|_| EditError::InvalidModel)?;
                    if !t.grid.is_empty() && end > t.grid.len() {
                        return Err(EditError::InvalidModel);
                    }
                    previous = next;
                }
            }
            Block::SdtBlock(s) => validate_structure(&s.blocks, document)?,
            _ => {}
        }
    }
    Ok(())
}
fn validate_inlines(inlines: &[Inline], document: &Document) -> Result<(), EditError> {
    for i in inlines {
        match i {
            Inline::Run(r) => {
                if r.props.style.as_ref().is_some_and(|id| {
                    !document.styles.get(id).is_some_and(|s| {
                        s.style_type == strict_ooxml_wml::model::StyleType::Character
                    })
                }) {
                    return Err(EditError::UnknownStyle);
                }
                for c in &r.content {
                    match c {
                        RunContent::Drawing(d) => validate_drawing(d, document)?,
                        RunContent::Text(t) => {
                            if t.text
                                .chars()
                                .any(|v| !strict_ooxml_core::xml::escape::is_xml_char(v))
                            {
                                return Err(EditError::InvalidText);
                            }
                        }
                        RunContent::FootnoteRef(id) => {
                            if i32::try_from(*id)
                                .ok()
                                .and_then(|id| document.footnotes.get(id))
                                .is_none()
                            {
                                return Err(EditError::InvalidModel);
                            }
                        }
                        RunContent::EndnoteRef(id) => {
                            if i32::try_from(*id)
                                .ok()
                                .and_then(|id| document.endnotes.get(id))
                                .is_none()
                            {
                                return Err(EditError::InvalidModel);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Inline::Hyperlink(v) => validate_inlines(&v.inlines, document)?,
            Inline::Field(v) => validate_inlines(&v.inlines, document)?,
            Inline::SdtInline(v) => validate_inlines(&v.inlines, document)?,
            Inline::Directional(v) => validate_inlines(&v.inlines, document)?,
            Inline::Drawing(d) => validate_drawing(d, document)?,
            Inline::FootnoteRef(id) => {
                if i32::try_from(*id)
                    .ok()
                    .and_then(|id| document.footnotes.get(id))
                    .is_none()
                {
                    return Err(EditError::InvalidModel);
                }
            }
            Inline::EndnoteRef(id) => {
                if i32::try_from(*id)
                    .ok()
                    .and_then(|id| document.endnotes.get(id))
                    .is_none()
                {
                    return Err(EditError::InvalidModel);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn validate_drawing(drawing: &Drawing, document: &Document) -> Result<(), EditError> {
    fn graphic(value: &Graphic, document: &Document) -> Result<(), EditError> {
        match value {
            Graphic::Picture(p) => {
                if p.blip
                    .as_ref()
                    .and_then(|b| b.resolved.as_ref())
                    .is_some_and(|part| document.media.get(part).is_none())
                {
                    return Err(EditError::InvalidModel);
                }
            }
            Graphic::Shape(s) => {
                if let Some(t) = &s.text {
                    validate_structure(&t.blocks, document)?;
                    validate_boundaries(&t.blocks)?;
                }
            }
            Graphic::Group(g) => {
                for child in &g.children {
                    graphic(child, document)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let (extent, payload) = match &drawing.kind {
        DrawingKind::Inline(v) => (v.extent, v.graphic.as_ref()),
        DrawingKind::Anchor(v) => (v.extent, v.graphic.as_ref()),
        DrawingKind::Opaque(_) => return Ok(()),
    };
    if extent.is_some_and(|v| v.cx.value() <= 0 || v.cy.value() <= 0) {
        return Err(EditError::InvalidModel);
    }
    graphic(payload, document)
}
fn validate_boundaries(blocks: &[Block]) -> Result<(), EditError> {
    #[derive(Default)]
    struct Boundaries {
        bookmarks: HashSet<String>,
        seen: HashSet<String>,
        comments: HashSet<String>,
        fields: Vec<bool>,
    }
    fn inlines(items: &[Inline], state: &mut Boundaries) -> Result<(), EditError> {
        for i in items {
            match i {
                Inline::BookmarkStart(v) => {
                    let id = v.id.as_str().to_owned();
                    if !state.seen.insert(id.clone()) || !state.bookmarks.insert(id) {
                        return Err(EditError::InvalidModel);
                    }
                }
                Inline::BookmarkEnd(v) => {
                    if !state.bookmarks.remove(v.as_str()) {
                        return Err(EditError::InvalidModel);
                    }
                }
                Inline::CommentRangeStart(v) => {
                    if !state.comments.insert(v.as_str().to_owned()) {
                        return Err(EditError::InvalidModel);
                    }
                }
                Inline::CommentRangeEnd(v) => {
                    if !state.comments.remove(v.as_str()) {
                        return Err(EditError::InvalidModel);
                    }
                }
                Inline::Run(r) => {
                    for c in &r.content {
                        if let RunContent::FieldChar(v) = c {
                            use strict_ooxml_wml::model::FieldCharType;
                            match v.kind {
                                FieldCharType::Begin => state.fields.push(false),
                                FieldCharType::Separate => {
                                    let separated =
                                        state.fields.last_mut().ok_or(EditError::InvalidModel)?;
                                    if *separated {
                                        return Err(EditError::InvalidModel);
                                    }
                                    *separated = true;
                                }
                                FieldCharType::End => {
                                    state.fields.pop().ok_or(EditError::InvalidModel)?;
                                }
                            }
                        }
                    }
                }
                Inline::Hyperlink(v) => inlines(&v.inlines, state)?,
                Inline::SdtInline(v) => inlines(&v.inlines, state)?,
                Inline::Directional(v) => inlines(&v.inlines, state)?,
                Inline::Field(v) => {
                    let mut child = Boundaries::default();
                    inlines(&v.inlines, &mut child)?;
                    if !child.fields.is_empty() || !child.bookmarks.is_empty() {
                        return Err(EditError::InvalidModel);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn walk(blocks: &[Block], state: &mut Boundaries) -> Result<(), EditError> {
        for b in blocks {
            match b {
                Block::Paragraph(p) => inlines(&p.inlines, state)?,
                Block::Table(t) => {
                    for r in &t.rows {
                        for c in &r.cells {
                            walk(&c.blocks, state)?;
                        }
                    }
                }
                Block::SdtBlock(s) => walk(&s.blocks, state)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut state = Boundaries::default();
    walk(blocks, &mut state)?;
    if state.bookmarks.is_empty() && state.comments.is_empty() && state.fields.is_empty() {
        Ok(())
    } else {
        Err(EditError::InvalidModel)
    }
}
