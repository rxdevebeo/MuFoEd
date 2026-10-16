//! The model invariants an accepted transaction must keep.
use crate::{EditError, Invariant};
use std::collections::{HashMap, HashSet};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, FieldCharType, Graphic, Inline, Paragraph, RunContent,
};

use super::address::{addresses, read_paragraph, story_blocks, visit_boxes, visit_drawings, Story};
use super::ids::IdIndex;
use strict_ooxml_wml::model::StyleId;

/// One broken invariant, keyed so that the same defect is recognised after
/// edits move it: by the style, bookmark, note, media part or paragraph id it
/// concerns, or by its kind alone where it has none.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Violation {
    /// A broken model invariant and its key.
    Model(Invariant, String),
    /// A reference to a style the document does not define.
    Style(StyleId),
    /// Text with a character XML cannot carry.
    Text,
}

impl Violation {
    pub(crate) fn model(invariant: Invariant, key: &str) -> Self {
        Self::Model(invariant, key.to_owned())
    }

    fn error(&self) -> EditError {
        match self {
            Self::Model(invariant, _) => EditError::InvalidModel(*invariant),
            Self::Style(id) => EditError::UnknownStyle(id.clone()),
            Self::Text => EditError::InvalidText,
        }
    }
}

/// The defects a document came with, which edits may keep but not add to.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tolerance {
    known: HashSet<Violation>,
}

impl Tolerance {
    /// The defects, as the errors an edit introducing them would get.
    pub(crate) fn errors(&self) -> Vec<EditError> {
        let mut out: Vec<EditError> = self.known.iter().map(Violation::error).collect();
        out.sort_by_key(|error| format!("{error:?}"));
        out
    }
}

/// Where a check reports what it finds: collected as the source's own
/// defects when an editor opens a document, or refused unless the source
/// already had it.
pub(crate) enum Sink<'a> {
    Collect(&'a mut Tolerance),
    Check(&'a Tolerance),
}

impl Sink<'_> {
    pub(crate) fn report(&mut self, violation: Violation) -> Result<(), EditError> {
        match self {
            Self::Collect(tolerance) => {
                tolerance.known.insert(violation);
                Ok(())
            }
            Self::Check(tolerance) if tolerance.known.contains(&violation) => Ok(()),
            Self::Check(_) => Err(violation.error()),
        }
    }
}

pub(super) fn contains_boundary(b: &Block) -> bool {
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
pub(crate) fn validate(
    document: &Document,
    limits: &ResourceLimits,
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    crate::validate_body(document, sink)?;
    strict_ooxml_wml::nesting::check_document(document, limits)
        .map_err(|_| EditError::LimitExceeded)?;
    validate_global_references(document, sink)?;
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
        crate::validate_blocks(story, document, &mut HashSet::new(), 0, sink)?;
        validate_structure(story, document, sink)?;
        validate_boundaries(story, sink)?;
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
        validate_story_ids(document, &story, sink)?;
    }
    Ok(())
}
/// Paragraph identities are unique within `story`, a text id needs a paragraph
/// id, and frames are not negative.
fn validate_story_ids(
    document: &Document,
    story: &Story,
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    let mut ids = HashSet::new();
    let mut text_ids = HashSet::new();
    for address in addresses(document, story)? {
        let p = read_paragraph(document, &address)?;
        if let Some(id) = &p.para_id {
            let id = id.as_str().to_ascii_uppercase();
            if !ids.insert(id.clone()) {
                sink.report(Violation::model(Invariant::DuplicateId, &id))?;
            }
        }
        if let Some(id) = &p.text_id {
            if p.para_id.is_none() {
                sink.report(Violation::model(Invariant::TextIdWithoutParaId, ""))?;
            }
            let id = id.as_str().to_ascii_uppercase();
            if !text_ids.insert(id.clone()) {
                sink.report(Violation::model(Invariant::DuplicateId, &id))?;
            }
        }
        if negative_frame(p) {
            sink.report(Violation::model(Invariant::Frame, ""))?;
        }
    }
    Ok(())
}
fn negative_frame(p: &Paragraph) -> bool {
    p.props.frame.as_ref().is_some_and(|frame| {
        frame.width.is_some_and(|v| v.value() < 0) || frame.height.is_some_and(|v| v.value() < 0)
    })
}
/// The model after one patch of `story` that replaced `before` with `after`
/// (its final state), checked where the patch can have broken something.
///
/// Local invariants - nesting, styles, table topology, numbering, frames -
/// are checked on `after` alone. The ones that span blocks are checked across
/// the story only if the patch changed their input: paragraph identities if
/// the identities under the patch differ, bookmark/comment/field pairing if
/// its boundary marks differ. A section boundary on either side changes the
/// section structure and the header and footer references with it, so that
/// patch gets the full [`validate`]. Styles, numbering and the rest of the
/// global references are not edited by any command.
#[allow(clippy::too_many_arguments)]
pub(super) fn validate_patch(
    document: &Document,
    limits: &ResourceLimits,
    ids: &IdIndex,
    story: &Story,
    before: &[Block],
    after: &[Block],
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    if before.iter().chain(after).any(contains_boundary) {
        return validate(document, limits, sink);
    }
    strict_ooxml_wml::nesting::check_blocks(after, limits).map_err(|_| EditError::LimitExceeded)?;
    crate::validate_blocks(after, document, &mut HashSet::new(), 0, sink)?;
    validate_structure(after, document, sink)?;
    let mut frames = false;
    each_paragraph(after, &mut |p| frames |= negative_frame(p));
    if frames {
        sink.report(Violation::model(Invariant::Frame, ""))?;
    }
    if !identities_kept(ids, before, after, sink)? {
        validate_story_ids(document, story, sink)?;
    }
    if boundary_marks(before) != boundary_marks(after) {
        validate_boundaries(story_blocks(document, story)?, sink)?;
    }
    Ok(())
}
/// Calls `f` with every paragraph under `blocks`, in tables, block SDTs and
/// the text boxes of drawings - the paragraphs [`addresses`] lists.
fn each_paragraph(blocks: &[Block], f: &mut dyn FnMut(&Paragraph)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                f(p);
                visit_drawings(&p.inlines, &[], &mut |drawing, _, _| {
                    let graphic = match &drawing.kind {
                        DrawingKind::Inline(v) => v.graphic.as_ref(),
                        DrawingKind::Anchor(v) => v.graphic.as_ref(),
                        DrawingKind::Opaque(_) => return,
                    };
                    visit_boxes(graphic, &[], &mut |inner, _| each_paragraph(inner, f));
                });
            }
            Block::Table(t) => {
                for r in &t.rows {
                    for c in &r.cells {
                        each_paragraph(&c.blocks, f);
                    }
                }
            }
            Block::SdtBlock(s) => each_paragraph(&s.blocks, f),
            _ => {}
        }
    }
}
/// A paragraph's id and text id, uppercased.
type Identity = (Option<String>, Option<String>);
/// The paragraph and text identities under `blocks`, in order.
fn identities(blocks: &[Block]) -> Vec<Identity> {
    let mut out = Vec::new();
    each_paragraph(blocks, &mut |p| {
        out.push((
            p.para_id
                .as_ref()
                .map(|id| id.as_str().to_ascii_uppercase()),
            p.text_id
                .as_ref()
                .map(|id| id.as_str().to_ascii_uppercase()),
        ));
    });
    out
}
/// Whether the identities under `after` keep the story's identities unique
/// without a walk of the story, or `Ok(false)` if only the walk can tell.
///
/// A paragraph id that occurs more often under `after` than under `before`
/// is new to this range; it is unique in the story if `ids` (already updated
/// for the patch) counts it once in the whole document. A text id needs a
/// paragraph id, and new text ids - which no command mints - need the walk.
fn identities_kept(
    ids: &IdIndex,
    before: &[Block],
    after: &[Block],
    sink: &mut Sink<'_>,
) -> Result<bool, EditError> {
    let old = identities(before);
    let new = identities(after);
    if old == new {
        return Ok(true);
    }
    if new
        .iter()
        .any(|(para, text)| para.is_none() && text.is_some())
    {
        sink.report(Violation::model(Invariant::TextIdWithoutParaId, ""))?;
        return Ok(false);
    }
    let paragraphs = grown(&old, &new, |(para, _)| para.as_deref());
    let texts = grown(&old, &new, |(_, text)| text.as_deref());
    Ok(texts.is_empty() && paragraphs.iter().all(|id| ids.count(id) == 1))
}
/// The ids `pick` reads that occur more often in `new` than in `old`.
fn grown<'a>(
    old: &'a [Identity],
    new: &'a [Identity],
    pick: fn(&'a Identity) -> Option<&'a str>,
) -> Vec<&'a str> {
    let mut counts: HashMap<&str, isize> = HashMap::new();
    for id in new.iter().filter_map(pick) {
        *counts.entry(id).or_insert(0) += 1;
    }
    for id in old.iter().filter_map(pick) {
        *counts.entry(id).or_insert(0) -= 1;
    }
    counts
        .into_iter()
        .filter_map(|(id, count)| (count > 0).then_some(id))
        .collect()
}
/// One input of [`validate_boundaries`].
#[derive(PartialEq, Eq)]
enum Mark {
    BookmarkStart(String),
    BookmarkEnd(String),
    CommentStart(String),
    CommentEnd(String),
    FieldChar(FieldCharType),
    FieldOpen,
    FieldClose,
}
/// The marks [`validate_boundaries`] reads under `blocks`, in its walk order:
/// two ranges with the same marks leave its verdict on the story unchanged.
fn boundary_marks(blocks: &[Block]) -> Vec<Mark> {
    fn inlines(items: &[Inline], out: &mut Vec<Mark>) {
        for item in items {
            match item {
                Inline::BookmarkStart(v) => {
                    out.push(Mark::BookmarkStart(v.id.as_str().to_owned()));
                }
                Inline::BookmarkEnd(v) => out.push(Mark::BookmarkEnd(v.as_str().to_owned())),
                Inline::CommentRangeStart(v) => {
                    out.push(Mark::CommentStart(v.as_str().to_owned()));
                }
                Inline::CommentRangeEnd(v) => out.push(Mark::CommentEnd(v.as_str().to_owned())),
                Inline::Run(r) => {
                    for c in &r.content {
                        if let RunContent::FieldChar(v) = c {
                            out.push(Mark::FieldChar(v.kind));
                        }
                    }
                }
                Inline::Hyperlink(v) => inlines(&v.inlines, out),
                Inline::SdtInline(v) => inlines(&v.inlines, out),
                Inline::Directional(v) => inlines(&v.inlines, out),
                Inline::Field(v) => {
                    out.push(Mark::FieldOpen);
                    inlines(&v.inlines, out);
                    out.push(Mark::FieldClose);
                }
                _ => {}
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<Mark>) {
        for b in blocks {
            match b {
                Block::Paragraph(p) => inlines(&p.inlines, out),
                Block::Table(t) => {
                    for r in &t.rows {
                        for c in &r.cells {
                            walk(&c.blocks, out);
                        }
                    }
                }
                Block::SdtBlock(s) => walk(&s.blocks, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}
fn validate_global_references(document: &Document, sink: &mut Sink<'_>) -> Result<(), EditError> {
    for style in document.styles.iter() {
        let mut ancestors = vec![];
        let mut parent = style.based_on.as_ref();
        let mut broken = false;
        while let Some(id) = parent {
            if id == &style.id || ancestors.contains(id) || ancestors.len() >= 64 {
                sink.report(Violation::model(Invariant::StyleChain, style.id.as_str()))?;
                broken = true;
                break;
            }
            let Some(definition) = document.styles.get(id) else {
                sink.report(Violation::Style(id.clone()))?;
                broken = true;
                break;
            };
            ancestors.push(id.clone());
            parent = definition.based_on.as_ref();
        }
        if !broken && ancestors != style.based_on_chain {
            sink.report(Violation::model(Invariant::StyleChain, style.id.as_str()))?;
        }
        for id in [&style.next, &style.link]
            .into_iter()
            .flatten()
            .filter(|id| document.styles.get(id).is_none())
        {
            sink.report(Violation::Style(id.clone()))?;
        }
    }
    for number in document.numbering.nums() {
        if document
            .numbering
            .abstract_num(number.abstract_num_id)
            .is_none()
            || number.overrides.iter().any(|v| !v.ilvl.is_valid())
        {
            sink.report(Violation::model(
                Invariant::Numbering,
                &number.num_id.0.to_string(),
            ))?;
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
                    sink.report(Violation::model(Invariant::HeaderFooterReference, ""))?;
                }
            }
        }
    }
    Ok(())
}
#[allow(clippy::too_many_lines)]
fn validate_structure(
    blocks: &[Block],
    document: &Document,
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                if let Some(id) = p.props.style.as_ref().filter(|id| {
                    !document.styles.get(id).is_some_and(|s| {
                        s.style_type == strict_ooxml_wml::model::StyleType::Paragraph
                    })
                }) {
                    sink.report(Violation::Style(id.clone()))?;
                }
                validate_inlines(&p.inlines, document, sink)?;
                if let Some(n) = p.props.numbering {
                    if n.ilvl.is_some_and(|v| !v.is_valid())
                        || n.num_id
                            .is_some_and(|id| id.0 != 0 && document.numbering.num(id).is_none())
                    {
                        let key = n.num_id.map_or_else(String::new, |id| id.0.to_string());
                        sink.report(Violation::model(Invariant::Numbering, &key))?;
                    }
                }
            }
            Block::Table(t) => {
                if t.rows.is_empty() {
                    sink.report(Violation::model(Invariant::TableTopology, ""))?;
                }
                if t.grid
                    .iter()
                    .any(|g| g.width.is_some_and(|v| v.value() <= 0))
                {
                    sink.report(Violation::model(Invariant::Grid, ""))?;
                }
                if let Some(id) = t.props.style.as_ref().filter(|id| {
                    !document
                        .styles
                        .get(id)
                        .is_some_and(|s| s.style_type == strict_ooxml_wml::model::StyleType::Table)
                }) {
                    sink.report(Violation::Style(id.clone()))?;
                }
                let mut previous = std::collections::BTreeSet::new();
                for r in &t.rows {
                    if r.cells.is_empty() {
                        sink.report(Violation::model(Invariant::TableTopology, ""))?;
                    }
                    let mut column = usize::try_from(r.props.grid_before.unwrap_or(0)).unwrap_or(0);
                    let mut next = std::collections::BTreeSet::new();
                    for c in &r.cells {
                        let span = usize::from(c.props.grid_span.unwrap_or(1));
                        if span == 0 || !matches!(c.blocks.last(), Some(Block::Paragraph(_))) {
                            sink.report(Violation::model(Invariant::TableTopology, ""))?;
                        }
                        let region = (column, span);
                        match c.props.vertical_merge {
                            Some(strict_ooxml_wml::model::VerticalMerge::Continue) => {
                                if !previous.contains(&region) {
                                    sink.report(Violation::model(Invariant::TableTopology, ""))?;
                                }
                                next.insert(region);
                            }
                            Some(strict_ooxml_wml::model::VerticalMerge::Restart) => {
                                next.insert(region);
                            }
                            None => {}
                        }
                        column += span;
                        validate_structure(&c.blocks, document, sink)?;
                    }
                    let end =
                        column + usize::try_from(r.props.grid_after.unwrap_or(0)).unwrap_or(0);
                    if !t.grid.is_empty() && end > t.grid.len() {
                        sink.report(Violation::model(Invariant::TableTopology, ""))?;
                    }
                    previous = next;
                }
            }
            Block::SdtBlock(s) => validate_structure(&s.blocks, document, sink)?,
            _ => {}
        }
    }
    Ok(())
}
fn validate_inlines(
    inlines: &[Inline],
    document: &Document,
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    for i in inlines {
        match i {
            Inline::Run(r) => {
                if let Some(id) = r.props.style.as_ref().filter(|id| {
                    !document.styles.get(id).is_some_and(|s| {
                        s.style_type == strict_ooxml_wml::model::StyleType::Character
                    })
                }) {
                    sink.report(Violation::Style(id.clone()))?;
                }
                for c in &r.content {
                    match c {
                        RunContent::Drawing(d) => validate_drawing(d, document, sink)?,
                        RunContent::Text(t) => {
                            if t.text
                                .chars()
                                .any(|v| !strict_ooxml_core::xml::escape::is_xml_char(v))
                            {
                                sink.report(Violation::Text)?;
                            }
                        }
                        RunContent::FootnoteRef(id) => {
                            if i32::try_from(*id)
                                .ok()
                                .and_then(|id| document.footnotes.get(id))
                                .is_none()
                            {
                                sink.report(Violation::model(
                                    Invariant::NoteReference,
                                    &format!("{}{id}", "f"),
                                ))?;
                            }
                        }
                        RunContent::EndnoteRef(id) => {
                            if i32::try_from(*id)
                                .ok()
                                .and_then(|id| document.endnotes.get(id))
                                .is_none()
                            {
                                sink.report(Violation::model(
                                    Invariant::NoteReference,
                                    &format!("{}{id}", "e"),
                                ))?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Inline::Hyperlink(v) => validate_inlines(&v.inlines, document, sink)?,
            Inline::Field(v) => validate_inlines(&v.inlines, document, sink)?,
            Inline::SdtInline(v) => validate_inlines(&v.inlines, document, sink)?,
            Inline::Directional(v) => validate_inlines(&v.inlines, document, sink)?,
            Inline::Drawing(d) => validate_drawing(d, document, sink)?,
            Inline::FootnoteRef(id) => {
                if i32::try_from(*id)
                    .ok()
                    .and_then(|id| document.footnotes.get(id))
                    .is_none()
                {
                    sink.report(Violation::model(
                        Invariant::NoteReference,
                        &format!("{}{id}", "f"),
                    ))?;
                }
            }
            Inline::EndnoteRef(id) => {
                if i32::try_from(*id)
                    .ok()
                    .and_then(|id| document.endnotes.get(id))
                    .is_none()
                {
                    sink.report(Violation::model(
                        Invariant::NoteReference,
                        &format!("{}{id}", "e"),
                    ))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn validate_drawing(
    drawing: &Drawing,
    document: &Document,
    sink: &mut Sink<'_>,
) -> Result<(), EditError> {
    fn graphic(value: &Graphic, document: &Document, sink: &mut Sink<'_>) -> Result<(), EditError> {
        match value {
            Graphic::Picture(p) => {
                if let Some(part) = p
                    .blip
                    .as_ref()
                    .and_then(|b| b.resolved.as_ref())
                    .filter(|part| document.media.get(part).is_none())
                {
                    sink.report(Violation::model(Invariant::Drawing, part.as_str()))?;
                }
            }
            Graphic::Shape(s) => {
                if let Some(t) = &s.text {
                    validate_structure(&t.blocks, document, sink)?;
                    validate_boundaries(&t.blocks, sink)?;
                }
            }
            Graphic::Group(g) => {
                for child in &g.children {
                    graphic(child, document, sink)?;
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
    // `ST_PositiveCoordinate` is zero or more: an empty drawing is valid.
    if extent.is_some_and(|v| v.cx.value() < 0 || v.cy.value() < 0) {
        sink.report(Violation::model(Invariant::Drawing, "extent"))?;
    }
    graphic(payload, document, sink)
}
#[allow(clippy::too_many_lines)]
fn validate_boundaries(blocks: &[Block], sink: &mut Sink<'_>) -> Result<(), EditError> {
    #[derive(Default)]
    struct Boundaries {
        bookmarks: HashSet<String>,
        seen: HashSet<String>,
        comments: HashSet<String>,
        fields: Vec<bool>,
    }
    fn inlines(
        items: &[Inline],
        state: &mut Boundaries,
        sink: &mut Sink<'_>,
    ) -> Result<(), EditError> {
        for i in items {
            match i {
                Inline::BookmarkStart(v) => {
                    let id = v.id.as_str().to_owned();
                    if !state.seen.insert(id.clone()) || !state.bookmarks.insert(id.clone()) {
                        sink.report(Violation::model(Invariant::Bookmark, &id))?;
                    }
                }
                Inline::BookmarkEnd(v) => {
                    if !state.bookmarks.remove(v.as_str()) {
                        sink.report(Violation::model(Invariant::Bookmark, v.as_str()))?;
                    }
                }
                Inline::CommentRangeStart(v) => {
                    if !state.comments.insert(v.as_str().to_owned()) {
                        sink.report(Violation::model(Invariant::Comment, v.as_str()))?;
                    }
                }
                Inline::CommentRangeEnd(v) => {
                    if !state.comments.remove(v.as_str()) {
                        sink.report(Violation::model(Invariant::Comment, v.as_str()))?;
                    }
                }
                Inline::Run(r) => {
                    for c in &r.content {
                        if let RunContent::FieldChar(v) = c {
                            use strict_ooxml_wml::model::FieldCharType;
                            match v.kind {
                                FieldCharType::Begin => state.fields.push(false),
                                FieldCharType::Separate => match state.fields.last_mut() {
                                    Some(separated) if !*separated => *separated = true,
                                    _ => sink.report(Violation::model(Invariant::Field, ""))?,
                                },
                                FieldCharType::End => {
                                    if state.fields.pop().is_none() {
                                        sink.report(Violation::model(Invariant::Field, ""))?;
                                    }
                                }
                            }
                        }
                    }
                }
                Inline::Hyperlink(v) => inlines(&v.inlines, state, sink)?,
                Inline::SdtInline(v) => inlines(&v.inlines, state, sink)?,
                Inline::Directional(v) => inlines(&v.inlines, state, sink)?,
                Inline::Field(v) => {
                    let mut child = Boundaries::default();
                    inlines(&v.inlines, &mut child, sink)?;
                    if !child.fields.is_empty() || !child.bookmarks.is_empty() {
                        sink.report(Violation::model(Invariant::Field, ""))?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn walk(
        blocks: &[Block],
        state: &mut Boundaries,
        sink: &mut Sink<'_>,
    ) -> Result<(), EditError> {
        for b in blocks {
            match b {
                Block::Paragraph(p) => inlines(&p.inlines, state, sink)?,
                Block::Table(t) => {
                    for r in &t.rows {
                        for c in &r.cells {
                            walk(&c.blocks, state, sink)?;
                        }
                    }
                }
                Block::SdtBlock(s) => walk(&s.blocks, state, sink)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut state = Boundaries::default();
    walk(blocks, &mut state, sink)?;
    let mut open: Vec<String> = state.bookmarks.into_iter().collect();
    open.sort();
    for id in open {
        sink.report(Violation::model(Invariant::Bookmark, &id))?;
    }
    let mut open: Vec<String> = state.comments.into_iter().collect();
    open.sort();
    for id in open {
        sink.report(Violation::model(Invariant::Comment, &id))?;
    }
    if !state.fields.is_empty() {
        sink.report(Violation::model(Invariant::Field, ""))?;
    }
    Ok(())
}
