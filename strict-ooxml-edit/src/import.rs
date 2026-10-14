//! K10 (first part): blocks from another document, with the styles and lists
//! they use.
//!
//! A style the target already has under the same id is the target's: the
//! destination's styles win, as when Word pastes with destination styles. A
//! style it lacks comes over with what it needs (`basedOn`, `next`, `link`,
//! its list). Every list the blocks use comes over as a new list instance and
//! abstract definition, so pasted numbering neither joins nor restarts a
//! list of the target. Bookmarks whose names the target uses already are
//! dropped, as Word does; the others take fresh ids. Section breaks in the
//! blocks are dropped: the blocks join the section they land in.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use strict_ooxml_wml::model::{
    AbstractNumId, Block, BookmarkId, Document, Inline, NumId, Paragraph, RunContent, Style,
    StyleId,
};

use crate::definitions::{each_block_site, Site};
use crate::{EditError, Unsupported};

/// What the blocks name: styles and list instances of the source.
#[derive(Default)]
struct Names {
    styles: BTreeSet<StyleId>,
    nums: BTreeSet<NumId>,
}

/// Prepares `blocks` of `source` for `target`: brings the styles and lists
/// they use into `target` and returns the blocks as they must be inserted.
/// `annotation` hands out bookmark ids the target does not use.
pub(crate) fn bring(
    target: &mut Document,
    source: &Document,
    blocks: &[Block],
    annotation: &mut dyn FnMut() -> u32,
) -> Result<Vec<Block>, EditError> {
    let mut blocks = blocks.to_vec();
    let mut names = Names::default();
    scan(&blocks, &mut names)?;
    let (styles, nums) = closure(target, source, names)?;
    // New ids for the lists, one abstract definition per one used.
    let mut next_num = crate::unused_num_id(target).0;
    let mut next_abstract = crate::unused_abstract_num_id(target).0;
    let mut num_map = BTreeMap::new();
    let mut abstract_map = BTreeMap::new();
    for id in &nums {
        let Some(num) = source.numbering.num(*id) else {
            continue;
        };
        abstract_map.entry(num.abstract_num_id).or_insert_with(|| {
            let id = AbstractNumId(next_abstract);
            next_abstract = next_abstract.saturating_add(1);
            id
        });
        num_map.insert(*id, NumId(next_num));
        next_num = next_num.saturating_add(1);
    }
    for (old, new) in &abstract_map {
        if let Some(abstract_num) = source.numbering.abstract_num(*old) {
            let mut copy = abstract_num.clone();
            copy.id = *new;
            target.numbering.insert_abstract(copy);
        }
    }
    for (old, new) in &num_map {
        if let Some(num) = source.numbering.num(*old) {
            let mut copy = num.clone();
            copy.num_id = *new;
            copy.abstract_num_id = abstract_map
                .get(&num.abstract_num_id)
                .copied()
                .unwrap_or(copy.abstract_num_id);
            target.numbering.insert_num(copy);
        }
    }
    for id in &styles {
        if let Some(style) = source.styles.get(id) {
            let mut copy: Style = style.clone();
            // The target keeps its own default of each kind.
            copy.is_default = false;
            copy.based_on_chain.clear();
            if let Some(numbering) = &mut copy.paragraph.numbering {
                numbering.num_id = remap_num(numbering.num_id, &num_map);
            }
            target.styles.insert(copy);
        }
    }
    // What a copied definition names and neither document has is dropped.
    let present: HashSet<StyleId> = target.styles.ids().cloned().collect();
    let clear = |slot: &mut Option<StyleId>| {
        if slot.as_ref().is_some_and(|id| !present.contains(id)) {
            *slot = None;
        }
    };
    for id in &styles {
        if let Some(style) = target.styles.get_mut(id) {
            clear(&mut style.based_on);
            clear(&mut style.next);
            clear(&mut style.link);
        }
    }
    for id in abstract_map.values() {
        if let Some(abstract_num) = target.numbering.abstract_num(*id) {
            let mut copy = abstract_num.clone();
            for level in &mut copy.levels {
                clear(&mut level.paragraph_style);
            }
            target.numbering.replace_abstract(copy);
        }
    }
    strict_ooxml_wml::resolve::refresh_definitions(target);
    // References the source could not satisfy are dropped, not left dangling.
    let known = |slot: &mut Option<StyleId>| {
        if slot.as_ref().is_some_and(|id| !target.styles.contains(id)) {
            *slot = None;
        }
    };
    each_block_site(&mut blocks, &mut |site| match site {
        Site::Paragraph(props) => {
            known(&mut props.style);
            if let Some(mark) = &mut props.run_props {
                known(&mut mark.style);
            }
            if let Some(numbering) = &mut props.numbering {
                numbering.num_id = remap_num(numbering.num_id, &num_map);
            }
            props.section = None;
        }
        Site::Run(props) => known(&mut props.style),
        Site::Table(props) => known(&mut props.style),
    });
    bookmarks(target, &mut blocks, annotation);
    Ok(blocks)
}

/// The new id of a list the blocks name; `0` (no list) stays, and a list the
/// source lacks becomes no list.
fn remap_num(id: Option<NumId>, map: &BTreeMap<NumId, NumId>) -> Option<NumId> {
    match id {
        Some(NumId(0)) | None => id,
        Some(id) => Some(map.get(&id).copied().unwrap_or(NumId(0))),
    }
}

/// The styles and lists to bring over: what the blocks name, what those
/// styles and lists name in turn, less what the target has already.
fn closure(
    target: &Document,
    source: &Document,
    names: Names,
) -> Result<(BTreeSet<StyleId>, BTreeSet<NumId>), EditError> {
    let mut styles = BTreeSet::new();
    let mut nums = BTreeSet::new();
    let mut pending_styles: Vec<StyleId> = names.styles.into_iter().collect();
    let mut pending_nums: Vec<NumId> = names.nums.into_iter().collect();
    while !pending_styles.is_empty() || !pending_nums.is_empty() {
        while let Some(id) = pending_styles.pop() {
            let Some(style) = source.styles.get(&id) else {
                continue;
            };
            if let Some(existing) = target.styles.get(&id) {
                // One id naming two kinds of style cannot be merged.
                if existing.style_type != style.style_type {
                    return Err(EditError::UnsupportedContent(Unsupported::Import));
                }
                continue;
            }
            if !styles.insert(id) {
                continue;
            }
            pending_styles.extend(
                [&style.based_on, &style.next, &style.link]
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            if let Some(num) = style.paragraph.numbering.and_then(|n| n.num_id) {
                pending_nums.push(num);
            }
        }
        while let Some(id) = pending_nums.pop() {
            if id.0 == 0 || !nums.insert(id) {
                continue;
            }
            let Some(abstract_num) = source.numbering.resolved_abstract(id) else {
                continue;
            };
            // A list defined through a numbering style, or with picture
            // bullets (images of the source package), is not carried over.
            let pictures = abstract_num
                .levels
                .iter()
                .chain(
                    source
                        .numbering
                        .num(id)
                        .into_iter()
                        .flat_map(|num| num.overrides.iter())
                        .filter_map(|o| o.level.as_ref()),
                )
                .any(|level| level.pic_bullet_id.is_some());
            if abstract_num.num_style_link.is_some()
                || abstract_num.style_link.is_some()
                || pictures
            {
                return Err(EditError::UnsupportedContent(Unsupported::Import));
            }
            pending_styles.extend(
                abstract_num
                    .levels
                    .iter()
                    .filter_map(|level| level.paragraph_style.clone()),
            );
        }
    }
    Ok((styles, nums))
}

/// Collects what the blocks name and refuses what an import cannot carry:
/// content tied to the source package (images, hyperlinks by relationship),
/// note and comment references, opaque markup.
fn scan(blocks: &[Block], names: &mut Names) -> Result<(), EditError> {
    let refused = || Err(EditError::UnsupportedContent(Unsupported::Import));
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                names.styles.extend(p.props.style.clone());
                if let Some(mark) = &p.props.run_props {
                    names.styles.extend(mark.style.clone());
                }
                if let Some(num) = p.props.numbering.and_then(|n| n.num_id) {
                    names.nums.insert(num);
                }
                scan_inlines(&p.inlines, names)?;
            }
            Block::Table(t) => {
                names.styles.extend(t.props.style.clone());
                for cell in t.rows.iter().flat_map(|row| &row.cells) {
                    scan(&cell.blocks, names)?;
                }
            }
            Block::SdtBlock(s) => scan(&s.blocks, names)?,
            _ => return refused(),
        }
    }
    Ok(())
}

fn scan_inlines(inlines: &[Inline], names: &mut Names) -> Result<(), EditError> {
    let refused = || Err(EditError::UnsupportedContent(Unsupported::Import));
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                names.styles.extend(run.props.style.clone());
                for content in &run.content {
                    if matches!(
                        content,
                        RunContent::Drawing(_)
                            | RunContent::FootnoteRef(_)
                            | RunContent::EndnoteRef(_)
                            | RunContent::NoteRef
                            | RunContent::NoteSeparator(_)
                    ) {
                        return refused();
                    }
                }
            }
            Inline::Hyperlink(v) if v.rel_id.is_some() => return refused(),
            Inline::Hyperlink(v) => scan_inlines(&v.inlines, names)?,
            Inline::Field(v) => scan_inlines(&v.inlines, names)?,
            Inline::SdtInline(v) => scan_inlines(&v.inlines, names)?,
            Inline::Directional(v) => scan_inlines(&v.inlines, names)?,
            Inline::Drawing(_)
            | Inline::FootnoteRef(_)
            | Inline::EndnoteRef(_)
            | Inline::CommentRangeStart(_)
            | Inline::CommentRangeEnd(_)
            | Inline::CommentReference(_)
            | Inline::Opaque(_) => return refused(),
            _ => {}
        }
    }
    Ok(())
}

/// Gives the blocks' bookmarks fresh ids, dropping those whose name the
/// target has and those not both begun and ended in the blocks.
fn bookmarks(target: &Document, blocks: &mut [Block], annotation: &mut dyn FnMut() -> u32) {
    let mut taken = HashSet::new();
    let stories = std::iter::once(&target.body.blocks)
        .chain(target.headers_footers.iter().map(|h| &h.blocks))
        .chain(
            target
                .footnotes
                .iter()
                .chain(target.endnotes.iter())
                .map(|n| &n.blocks),
        );
    for story in stories {
        each_inline(story, &mut |inline| {
            if let Inline::BookmarkStart(b) = inline {
                taken.insert(b.name.clone());
            }
        });
    }
    // Starts in reading order, so that new ids follow it.
    let mut starts = Vec::new();
    let mut ends = HashSet::new();
    each_inline(blocks, &mut |inline| match inline {
        Inline::BookmarkStart(b) => starts.push((b.id.clone(), b.name.clone())),
        Inline::BookmarkEnd(id) => {
            ends.insert(id.clone());
        }
        _ => {}
    });
    let mut kept: HashMap<BookmarkId, BookmarkId> = HashMap::new();
    for (id, name) in starts {
        if ends.contains(&id) && !kept.contains_key(&id) && taken.insert(name) {
            kept.insert(id, BookmarkId::new(annotation().to_string()));
        }
    }
    each_paragraph(blocks, &mut |p| retain_bookmarks(&mut p.inlines, &kept));
}

fn retain_bookmarks(inlines: &mut Vec<Inline>, kept: &HashMap<BookmarkId, BookmarkId>) {
    inlines.retain_mut(|inline| match inline {
        Inline::BookmarkStart(b) => match kept.get(&b.id) {
            Some(new) => {
                b.id = new.clone();
                true
            }
            None => false,
        },
        Inline::BookmarkEnd(id) => match kept.get(id) {
            Some(new) => {
                *id = new.clone();
                true
            }
            None => false,
        },
        Inline::Hyperlink(v) => {
            retain_bookmarks(&mut v.inlines, kept);
            true
        }
        Inline::Field(v) => {
            retain_bookmarks(&mut v.inlines, kept);
            true
        }
        Inline::SdtInline(v) => {
            retain_bookmarks(&mut v.inlines, kept);
            true
        }
        Inline::Directional(v) => {
            retain_bookmarks(&mut v.inlines, kept);
            true
        }
        _ => true,
    });
}

/// Calls `f` with every inline under `blocks`, wrappers' children included.
fn each_inline(blocks: &[Block], f: &mut dyn FnMut(&Inline)) {
    fn inlines(items: &[Inline], f: &mut dyn FnMut(&Inline)) {
        for item in items {
            f(item);
            match item {
                Inline::Hyperlink(v) => inlines(&v.inlines, f),
                Inline::Field(v) => inlines(&v.inlines, f),
                Inline::SdtInline(v) => inlines(&v.inlines, f),
                Inline::Directional(v) => inlines(&v.inlines, f),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph(p) => inlines(&p.inlines, f),
            Block::Table(t) => {
                for cell in t.rows.iter().flat_map(|row| &row.cells) {
                    each_inline(&cell.blocks, f);
                }
            }
            Block::SdtBlock(s) => each_inline(&s.blocks, f),
            _ => {}
        }
    }
}

/// Calls `f` with every paragraph under `blocks`.
fn each_paragraph(blocks: &mut [Block], f: &mut dyn FnMut(&mut Paragraph)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => f(p),
            Block::Table(t) => {
                for cell in t.rows.iter_mut().flat_map(|row| &mut row.cells) {
                    each_paragraph(&mut cell.blocks, f);
                }
            }
            Block::SdtBlock(s) => each_paragraph(&mut s.blocks, f),
            _ => {}
        }
    }
}
