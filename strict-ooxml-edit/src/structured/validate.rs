//! The model invariants an accepted transaction must keep.
use crate::EditError;
use std::collections::{HashMap, HashSet};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_wml::model::{
    Block, Document, Drawing, DrawingKind, FieldCharType, Graphic, Inline, Paragraph, RunContent,
};

use super::address::{addresses, read_paragraph, story_blocks, visit_boxes, visit_drawings, Story};
use super::ids::IdIndex;

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
pub(crate) fn validate(document: &Document, limits: &ResourceLimits) -> Result<(), EditError> {
    crate::validate_body(document)?;
    strict_ooxml_wml::nesting::check_document(document, limits)
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
        crate::validate_blocks(story, document, &mut HashSet::new(), 0)?;
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
        validate_story_ids(document, &story)?;
    }
    Ok(())
}
/// Paragraph identities are unique within `story`, a text id needs a paragraph
/// id, and frames are not negative.
fn validate_story_ids(document: &Document, story: &Story) -> Result<(), EditError> {
    let mut ids = HashSet::new();
    let mut text_ids = HashSet::new();
    for address in addresses(document, story)? {
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
        if negative_frame(p) {
            return Err(EditError::InvalidModel);
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
pub(super) fn validate_patch(
    document: &Document,
    limits: &ResourceLimits,
    ids: &IdIndex,
    story: &Story,
    before: &[Block],
    after: &[Block],
) -> Result<(), EditError> {
    if before.iter().chain(after).any(contains_boundary) {
        return validate(document, limits);
    }
    strict_ooxml_wml::nesting::check_blocks(after, limits).map_err(|_| EditError::LimitExceeded)?;
    crate::validate_blocks(after, document, &mut HashSet::new(), 0)?;
    validate_structure(after, document)?;
    let mut frames = false;
    each_paragraph(after, &mut |p| frames |= negative_frame(p));
    if frames {
        return Err(EditError::InvalidModel);
    }
    if !identities_kept(ids, before, after)? {
        validate_story_ids(document, story)?;
    }
    if boundary_marks(before) != boundary_marks(after) {
        validate_boundaries(story_blocks(document, story)?)?;
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
fn identities_kept(ids: &IdIndex, before: &[Block], after: &[Block]) -> Result<bool, EditError> {
    let old = identities(before);
    let new = identities(after);
    if old == new {
        return Ok(true);
    }
    if new
        .iter()
        .any(|(para, text)| para.is_none() && text.is_some())
    {
        return Err(EditError::InvalidModel);
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
