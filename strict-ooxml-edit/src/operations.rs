//! Read-only query planning and compound operations over public editing commands.
use crate::{Address, ChangeSet, EditError, Editor, Story};
use std::ops::Range;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{ParaId, StyleId};
/// A literal query. Case-insensitive matching uses Unicode lowercase, not full case folding.
#[derive(Clone, Debug)]
pub struct TextQuery {
    /// Literal text; empty text is rejected.
    pub text: String,
    /// Preserve case when matching.
    pub case_sensitive: bool,
    /// Require alphanumeric/underscore word boundaries.
    pub whole_word: bool,
}
impl TextQuery {
    /// A case-sensitive literal query.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            case_sensitive: true,
            whole_word: false,
        }
    }
}
/// Which paragraphs are queried.
#[derive(Clone, Debug)]
pub enum SearchScope {
    /// Body, headers/footers and all notes.
    All,
    /// One story, including its nested paragraphs.
    Story(Story),
    /// One exact paragraph.
    Paragraph(Address),
}
/// A read-only query over the modeled document.
#[derive(Clone, Debug)]
pub enum SearchQuery {
    /// Literal modeled text, including hidden run text and cached field results.
    Text(TextQuery),
    /// Direct paragraph style reference.
    ParagraphStyle(StyleId),
    /// Direct run character style reference.
    CharacterStyle(StyleId),
    /// Exact paragraph source identity.
    Location(SourceLocation),
    /// A paragraph containing a complex field.
    HasComplexField,
}
/// One portion of a text match in a run's text node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSlice {
    /// Inline path ending at a run.
    pub inline: Vec<usize>,
    /// Content index in that run.
    pub content: usize,
    /// Scalar interval within that text node.
    pub range: Range<usize>,
}
/// A revision-scoped match. Metadata queries have no text range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    /// Paragraph address.
    pub paragraph: Address,
    /// Scalar interval in the modeled paragraph projection, if a text match.
    pub range: Option<Range<usize>>,
    /// Matched original text.
    pub text: String,
    /// Portions of a text match in original nodes.
    pub slices: Vec<TextSlice>,
    /// Whether existing `TextNode` commands can change the whole match.
    pub editable: bool,
}
/// Search evidence tied to the queried model revision.
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// Queried revision.
    pub revision: u64,
    /// Ordered matches.
    pub hits: Vec<SearchHit>,
}
/// Resource limits for read/operation planning.
#[derive(Clone, Copy, Debug)]
pub struct OperationLimits {
    /// Maximum returned matches.
    pub matches: usize,
    /// Maximum scanned text scalars.
    pub scalars: usize,
    /// Maximum compiled editing commands.
    pub commands: usize,
    /// Maximum character comparisons and segment visits.
    pub work: usize,
}
impl Default for OperationLimits {
    fn default() -> Self {
        Self {
            matches: 10_000,
            scalars: 16_000_000,
            commands: 100_000,
            work: 64_000_000,
        }
    }
}
/// Compound operation refusal never mutates the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationError {
    /// Existing kernel refusal.
    Edit(EditError),
    /// An empty literal query.
    InvalidQuery,
    /// Planning exceeded a resource budget.
    LimitExceeded,
    /// A strict replacement encountered protected content.
    ProtectedContent,
    /// Move endpoints are in different containers.
    UnsupportedMove,
}
impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for OperationError {}
impl From<EditError> for OperationError {
    fn from(e: EditError) -> Self {
        Self::Edit(e)
    }
}
/// Explicit handling of noneditable text matches.
#[derive(Clone, Copy, Debug)]
pub enum ReplacePolicy {
    /// Refuse the whole operation if any changed match is protected.
    Strict,
    /// Replace only editable matches and report every protected match.
    EditableOnly,
}
/// Whole-document replacement evidence.
#[derive(Clone, Debug)]
pub struct ReplaceReport {
    /// Matches found, including no-ops and skipped content.
    pub matched: usize,
    /// Matches whose text actually changed.
    pub replaced: usize,
    /// Explicit protected matches.
    pub skipped: Vec<SearchHit>,
    /// Kernel transaction result.
    pub change: ChangeSet,
}
/// Paragraph identity changes produced by existing insertion commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityChange {
    /// Identity before movement; imported paragraphs may have none.
    pub before: Option<ParaId>,
    /// Identity after movement.
    pub after: Option<ParaId>,
}
/// Atomic movement evidence.
#[derive(Clone, Debug)]
pub struct MoveReport {
    /// Final block address (table address for a row move).
    pub destination: Address,
    /// Final row index for a row move; absent for a block move.
    pub row: Option<usize>,
    /// Paragraph identities, in the moved subtree's traversal order.
    pub identities: Vec<IdentityChange>,
    /// Existing kernel transaction result.
    pub change: ChangeSet,
}
/// An adapter borrowing an existing session; it never directly mutates Document.
pub struct Operations<'session, 'document> {
    editor: &'session mut Editor<'document>,
    limits: OperationLimits,
}

impl<'session, 'document> Operations<'session, 'document> {
    /// Opens an adapter using the public command protocol.
    pub fn new(editor: &'session mut Editor<'document>, limits: OperationLimits) -> Self {
        Self { editor, limits }
    }
    fn check(&self, revision: u64) -> Result<(), OperationError> {
        if self.editor.revision() == revision {
            Ok(())
        } else {
            Err(EditError::StaleRevision.into())
        }
    }
    /// Finds text or paragraph metadata without issuing editing commands.
    pub fn search(
        &self,
        revision: u64,
        scope: &SearchScope,
        query: &SearchQuery,
    ) -> Result<SearchResult, OperationError> {
        self.check(revision)?;
        let mut budget = Budget {
            scalars: self.limits.scalars,
            work: self.limits.work,
        };
        let mut hits = vec![];
        if let SearchQuery::Text(q) = query {
            if q.text.is_empty() {
                return Err(OperationError::InvalidQuery);
            }
            if q.text.chars().count() > self.limits.scalars {
                return Err(OperationError::LimitExceeded);
            }
        }
        for at in scope_addresses(self.editor, scope)? {
            let p = self.editor.paragraph(&at)?;
            let mut projection = Projection::default();
            project(
                &p.inlines,
                &[],
                p.revision.is_none(),
                &mut projection,
                &mut budget,
            )?;
            if projection.has_field {
                for segment in &mut projection.segments {
                    segment.editable = false;
                }
            }
            if let SearchQuery::Text(q) = query {
                text_matches(
                    &at,
                    &projection,
                    q,
                    &mut hits,
                    self.limits.matches,
                    &mut budget,
                )?;
            } else {
                let matches = match query {
                    SearchQuery::ParagraphStyle(id) => p.props.style.as_ref() == Some(id),
                    SearchQuery::CharacterStyle(id) => projection.styles.contains(id),
                    SearchQuery::Location(location) => &p.location == location,
                    SearchQuery::HasComplexField => projection.has_field,
                    SearchQuery::Text(_) => false,
                };
                if matches {
                    push_hit(
                        &mut hits,
                        SearchHit {
                            paragraph: at,
                            range: None,
                            text: String::new(),
                            slices: vec![],
                            editable: false,
                        },
                        self.limits.matches,
                    )?;
                }
            }
        }
        Ok(SearchResult { revision, hits })
    }
    /// Compiles all replacements into one atomic existing transaction.
    pub fn replace_all(
        &mut self,
        revision: u64,
        scope: &SearchScope,
        query: &TextQuery,
        replacement: &str,
        policy: ReplacePolicy,
    ) -> Result<ReplaceReport, OperationError> {
        self.check(revision)?;
        if replacement.chars().any(|c| {
            !strict_ooxml_core::xml::escape::is_xml_char(c) || matches!(c, '\r' | '\n' | '\t')
        }) {
            return Err(EditError::InvalidText.into());
        }
        let result = self.search(revision, scope, &SearchQuery::Text(query.clone()))?;
        let matched = result.hits.len();
        let mut commands = vec![];
        let mut skipped = vec![];
        let mut replaced = 0;
        // Reverse order keeps every original offset valid, even across nodes.
        for hit in result.hits.into_iter().rev() {
            if hit.text == replacement {
                continue;
            }
            if !hit.editable {
                if matches!(policy, ReplacePolicy::Strict) {
                    return Err(OperationError::ProtectedContent);
                }
                skipped.push(hit);
                continue;
            }
            if commands.len().saturating_add(hit.slices.len()) > self.limits.commands {
                return Err(OperationError::LimitExceeded);
            }
            for (index, slice) in hit.slices.iter().enumerate().rev() {
                commands.push(crate::Edit::TextNode {
                    at: hit.paragraph.clone(),
                    inline: slice.inline.clone(),
                    content: slice.content,
                    range: slice.range.clone(),
                    text: if index == 0 {
                        replacement.into()
                    } else {
                        String::new()
                    },
                });
            }
            replaced += 1;
        }
        skipped.reverse();
        let change = self.editor.transact(revision, &commands)?;
        Ok(ReplaceReport {
            matched,
            replaced,
            skipped,
            change,
        })
    }
    /// Moves a block within the same container, using an original insertion boundary.
    pub fn move_block(
        &mut self,
        revision: u64,
        source: &Address,
        before: &Address,
    ) -> Result<MoveReport, OperationError> {
        self.check(revision)?;
        if source.story != before.story || source.containers != before.containers {
            return Err(OperationError::UnsupportedMove);
        }
        let blocks = blocks_at(self.editor.document(), source)?;
        let block = blocks
            .get(source.block)
            .ok_or(EditError::InvalidParagraph)?
            .clone();
        if before.block > blocks.len() {
            return Err(EditError::InvalidParagraph.into());
        }
        let old = block_ids(std::slice::from_ref(&block));
        let index = if before.block > source.block {
            before.block - 1
        } else {
            before.block
        };
        let mut destination = source.clone();
        destination.block = index;
        let change = if index == source.block {
            self.editor.transact(revision, &[])?
        } else {
            if self.limits.commands < 2 {
                return Err(OperationError::LimitExceeded);
            }
            // Keep source IDs occupied while Insert allocates fresh identities.
            let mut delete = source.clone();
            if before.block <= source.block {
                delete.block += 1;
            }
            self.editor.transact(
                revision,
                &[
                    crate::Edit::Insert {
                        at: before.clone(),
                        block: Box::new(block),
                    },
                    crate::Edit::Delete { at: delete },
                ],
            )?
        };
        let block = blocks_at(self.editor.document(), &destination)?
            .get(index)
            .ok_or(EditError::InvalidParagraph)?;
        let new = block_ids(std::slice::from_ref(block));
        Ok(MoveReport {
            destination,
            row: None,
            identities: identity_changes(old, new),
            change,
        })
    }
    /// Moves a row within one table, using an original insertion boundary.
    pub fn move_row(
        &mut self,
        revision: u64,
        table: &Address,
        source: usize,
        before: usize,
    ) -> Result<MoveReport, OperationError> {
        self.check(revision)?;
        let Some(strict_ooxml_wml::model::Block::Table(t)) =
            blocks_at(self.editor.document(), table)?.get(table.block)
        else {
            return Err(EditError::InvalidParagraph.into());
        };
        let row = t
            .rows
            .get(source)
            .ok_or(EditError::InvalidParagraph)?
            .clone();
        if before > t.rows.len() {
            return Err(EditError::InvalidParagraph.into());
        }
        let old = row_ids(&row);
        let index = if before > source { before - 1 } else { before };
        let change = if index == source {
            self.editor.transact(revision, &[])?
        } else {
            if self.limits.commands < 2 {
                return Err(OperationError::LimitExceeded);
            }
            self.editor.transact(
                revision,
                &[
                    crate::Edit::InsertRow {
                        at: table.clone(),
                        index: before,
                        row: Box::new(row),
                    },
                    crate::Edit::DeleteRow {
                        at: table.clone(),
                        index: if before <= source { source + 1 } else { source },
                    },
                ],
            )?
        };
        let Some(strict_ooxml_wml::model::Block::Table(t)) =
            blocks_at(self.editor.document(), table)?.get(table.block)
        else {
            return Err(EditError::InvalidParagraph.into());
        };
        let row = t.rows.get(index).ok_or(EditError::InvalidParagraph)?;
        Ok(MoveReport {
            destination: table.clone(),
            row: Some(index),
            identities: identity_changes(old, row_ids(row)),
            change,
        })
    }
}
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, Graphic, Inline, RunContent, TableRow,
};
struct Budget {
    scalars: usize,
    work: usize,
}
impl Budget {
    fn spend(&mut self, n: usize) -> Result<(), OperationError> {
        self.work = self
            .work
            .checked_sub(n)
            .ok_or(OperationError::LimitExceeded)?;
        Ok(())
    }
    fn scan(&mut self, n: usize) -> Result<(), OperationError> {
        self.scalars = self
            .scalars
            .checked_sub(n)
            .ok_or(OperationError::LimitExceeded)?;
        self.spend(n)
    }
}
#[derive(Default)]
struct Projection {
    chars: Vec<char>,
    segments: Vec<Segment>,
    styles: Vec<StyleId>,
    has_field: bool,
}
struct Segment {
    inline: Vec<usize>,
    content: usize,
    range: Range<usize>,
    editable: bool,
}
fn scope_addresses(
    editor: &Editor<'_>,
    scope: &SearchScope,
) -> Result<Vec<Address>, OperationError> {
    match scope {
        SearchScope::Paragraph(at) => {
            editor.paragraph(at)?;
            Ok(vec![at.clone()])
        }
        SearchScope::Story(story) => Ok(editor.paragraphs(story)?),
        SearchScope::All => {
            let d = editor.document();
            let mut stories = vec![Story::Body];
            stories.extend(
                d.headers_footers
                    .iter()
                    .map(|h| Story::HeaderFooter(h.part.clone())),
            );
            stories.extend(d.footnotes.iter().map(|n| Story::Footnote(n.id)));
            stories.extend(d.endnotes.iter().map(|n| Story::Endnote(n.id)));
            let mut out = vec![];
            for story in stories {
                out.extend(editor.paragraphs(&story)?);
            }
            Ok(out)
        }
    }
}
fn project(
    items: &[Inline],
    parents: &[usize],
    editable: bool,
    out: &mut Projection,
    budget: &mut Budget,
) -> Result<(), OperationError> {
    for (index, item) in items.iter().enumerate() {
        budget.spend(1)?;
        let mut path = parents.to_vec();
        path.push(index);
        match item {
            Inline::Run(run) => {
                if let Some(style) = &run.props.style {
                    out.styles.push(style.clone());
                }
                for (content, c) in run.content.iter().enumerate() {
                    budget.spend(1)?;
                    match c {
                        RunContent::Text(t) => {
                            let count = t.text.chars().count();
                            budget.scan(count)?;
                            let start = out.chars.len();
                            out.chars.extend(t.text.chars());
                            out.segments.push(Segment {
                                inline: path.clone(),
                                content,
                                range: start..out.chars.len(),
                                editable: editable && run.revision.is_none(),
                            });
                        }
                        RunContent::FieldChar(_) | RunContent::InstrText(_) => out.has_field = true,
                        RunContent::Tab => {
                            budget.scan(1)?;
                            out.chars.push('\t');
                        }
                        RunContent::Break(_) => {
                            budget.scan(1)?;
                            out.chars.push('\n');
                        }
                        _ => {
                            budget.scan(1)?;
                            out.chars.push('\u{fffc}');
                        }
                    }
                }
            }
            Inline::Hyperlink(v) => project(&v.inlines, &path, editable, out, budget)?,
            Inline::SdtInline(v) => project(&v.inlines, &path, editable, out, budget)?,
            Inline::Directional(v) => project(&v.inlines, &path, editable, out, budget)?,
            Inline::Field(v) => project(&v.inlines, &path, false, out, budget)?,
            Inline::BookmarkStart(_)
            | Inline::BookmarkEnd(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_) => {}
            _ => {
                budget.scan(1)?;
                out.chars.push('\u{fffc}');
            }
        }
    }
    Ok(())
}
fn fold(chars: &[char], sensitive: bool) -> (Vec<char>, Vec<Option<usize>>) {
    let mut keys = vec![];
    let mut boundaries = vec![Some(0)];
    for (index, ch) in chars.iter().enumerate() {
        if sensitive {
            keys.push(*ch);
            boundaries.push(Some(index + 1));
        } else {
            for lower in ch.to_lowercase() {
                keys.push(lower);
                boundaries.push(None);
            }
            if let Some(last) = boundaries.last_mut() {
                *last = Some(index + 1);
            }
        }
    }
    (keys, boundaries)
}
fn word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
fn text_matches(
    at: &Address,
    p: &Projection,
    q: &TextQuery,
    hits: &mut Vec<SearchHit>,
    limit: usize,
    budget: &mut Budget,
) -> Result<(), OperationError> {
    let (needle, _) = fold(&q.text.chars().collect::<Vec<_>>(), q.case_sensitive);
    let (keys, boundaries) = fold(&p.chars, q.case_sensitive);
    let mut cursor = 0;
    while let Some(window) = keys.get(cursor..cursor + needle.len()) {
        budget.spend(1)?;
        let mut matches = true;
        for (a, b) in window.iter().zip(&needle) {
            budget.spend(1)?;
            if a != b {
                matches = false;
                break;
            }
        }
        if matches {
            if let (Some(&Some(start)), Some(&Some(end))) = (
                boundaries.get(cursor),
                boundaries.get(cursor + needle.len()),
            ) {
                let before = start.checked_sub(1).and_then(|i| p.chars.get(i));
                if !q.whole_word
                    || before.is_none_or(|&c| !word(c))
                        && p.chars.get(end).is_none_or(|&c| !word(c))
                {
                    let mut slices = vec![];
                    let mut covered = 0;
                    let mut editable = true;
                    let text = p.chars.get(start..end).unwrap_or_default();
                    for s in &p.segments {
                        budget.spend(1)?;
                        let left = start.max(s.range.start);
                        let right = end.min(s.range.end);
                        if left < right {
                            covered += right - left;
                            editable &= s.editable;
                            slices.push(TextSlice {
                                inline: s.inline.clone(),
                                content: s.content,
                                range: left - s.range.start..right - s.range.start,
                            });
                        }
                    }
                    push_hit(
                        hits,
                        SearchHit {
                            paragraph: at.clone(),
                            range: Some(start..end),
                            text: text.iter().collect(),
                            slices,
                            editable: editable && covered == end - start,
                        },
                        limit,
                    )?;
                    cursor += needle.len();
                    continue;
                }
            }
        }
        cursor += 1;
    }
    Ok(())
}
fn push_hit(hits: &mut Vec<SearchHit>, hit: SearchHit, limit: usize) -> Result<(), OperationError> {
    if hits.len() >= limit {
        return Err(OperationError::LimitExceeded);
    }
    hits.push(hit);
    Ok(())
}
fn blocks_at<'a>(d: &'a Document, at: &Address) -> Result<&'a [Block], OperationError> {
    let mut blocks = match &at.story {
        Story::Body => d.body.blocks.as_slice(),
        Story::HeaderFooter(part) => {
            &d.header_footer(part)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Footnote(id) => {
            &d.footnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
        Story::Endnote(id) => {
            &d.endnotes
                .get(*id)
                .ok_or(EditError::InvalidParagraph)?
                .blocks
        }
    };
    for parent in &at.containers {
        blocks = match parent {
            crate::Container::Cell { table, row, cell } => {
                let Some(Block::Table(t)) = blocks.get(*table) else {
                    return Err(EditError::InvalidParagraph.into());
                };
                &t.rows
                    .get(*row)
                    .and_then(|r| r.cells.get(*cell))
                    .ok_or(EditError::InvalidParagraph)?
                    .blocks
            }
            crate::Container::Sdt(index) => {
                let Some(Block::SdtBlock(s)) = blocks.get(*index) else {
                    return Err(EditError::InvalidParagraph.into());
                };
                &s.blocks
            }
            crate::Container::TextBox {
                paragraph,
                inline,
                content,
                graphics,
            } => {
                let Some(Block::Paragraph(p)) = blocks.get(*paragraph) else {
                    return Err(EditError::InvalidParagraph.into());
                };
                let item = read_inline(&p.inlines, inline)?;
                let drawing = match (item, content) {
                    (Inline::Drawing(d), None) => d,
                    (Inline::Run(r), Some(index)) => match r.content.get(*index) {
                        Some(RunContent::Drawing(d)) => d,
                        _ => return Err(EditError::InvalidParagraph.into()),
                    },
                    _ => return Err(EditError::InvalidParagraph.into()),
                };
                let mut g = graphic(drawing)?;
                for index in graphics {
                    let Graphic::Group(group) = g else {
                        return Err(EditError::InvalidParagraph.into());
                    };
                    g = group
                        .children
                        .get(*index)
                        .ok_or(EditError::InvalidParagraph)?;
                }
                let Graphic::Shape(shape) = g else {
                    return Err(EditError::InvalidParagraph.into());
                };
                &shape
                    .text
                    .as_ref()
                    .ok_or(EditError::InvalidParagraph)?
                    .blocks
            }
        }
    }
    Ok(blocks)
}
fn read_inline<'a>(items: &'a [Inline], path: &[usize]) -> Result<&'a Inline, OperationError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidParagraph)?;
    let item = items.get(*index).ok_or(EditError::InvalidParagraph)?;
    if rest.is_empty() {
        return Ok(item);
    }
    let children = match item {
        Inline::Hyperlink(v) => &v.inlines,
        Inline::SdtInline(v) => &v.inlines,
        Inline::Directional(v) => &v.inlines,
        _ => return Err(EditError::UnsupportedContent.into()),
    };
    read_inline(children, rest)
}
fn graphic(d: &Drawing) -> Result<&Graphic, OperationError> {
    match &d.kind {
        DrawingKind::Inline(v) => Ok(&v.graphic),
        DrawingKind::Anchor(v) => Ok(&v.graphic),
        DrawingKind::Opaque(_) => Err(EditError::UnsupportedContent.into()),
    }
}
fn block_ids(blocks: &[Block]) -> Vec<Option<ParaId>> {
    let mut out = vec![];
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                out.push(p.para_id.clone());
                inline_ids(&p.inlines, &mut out);
            }
            Block::Table(t) => {
                for row in &t.rows {
                    out.extend(row_ids(row));
                }
            }
            Block::SdtBlock(s) => out.extend(block_ids(&s.blocks)),
            _ => {}
        }
    }
    out
}
fn row_ids(row: &TableRow) -> Vec<Option<ParaId>> {
    row.cells
        .iter()
        .flat_map(|c| block_ids(&c.blocks))
        .collect()
}
fn inline_ids(items: &[Inline], out: &mut Vec<Option<ParaId>>) {
    fn graphic_ids(g: &Graphic, out: &mut Vec<Option<ParaId>>) {
        match g {
            Graphic::Shape(s) => {
                if let Some(t) = &s.text {
                    out.extend(block_ids(&t.blocks));
                }
            }
            Graphic::Group(g) => {
                for child in &g.children {
                    graphic_ids(child, out);
                }
            }
            _ => {}
        }
    }
    for item in items {
        match item {
            Inline::Drawing(d) => {
                if let Ok(g) = graphic(d) {
                    graphic_ids(g, out);
                }
            }
            Inline::Run(r) => {
                for c in &r.content {
                    if let RunContent::Drawing(d) = c {
                        if let Ok(g) = graphic(d) {
                            graphic_ids(g, out);
                        }
                    }
                }
            }
            Inline::Hyperlink(v) => inline_ids(&v.inlines, out),
            Inline::SdtInline(v) => inline_ids(&v.inlines, out),
            Inline::Directional(v) => inline_ids(&v.inlines, out),
            Inline::Field(v) => inline_ids(&v.inlines, out),
            _ => {}
        }
    }
}
fn identity_changes(
    before: Vec<Option<ParaId>>,
    after: Vec<Option<ParaId>>,
) -> Vec<IdentityChange> {
    before
        .into_iter()
        .zip(after)
        .map(|(before, after)| IdentityChange { before, after })
        .collect()
}
