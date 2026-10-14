//! Structural comparison of two documents (K12, first part).
//!
//! The result is typed: which top-level blocks of which story were added,
//! removed, modified or moved, and which definitions (styles, numbering,
//! defaults, sections) and stories changed. Blocks are aligned by a longest
//! common subsequence where paragraphs with a `w14:paraId` match by identity
//! and the others by content; nothing is paired up by guessing - a block that
//! matches nothing is reported removed, its neighbour added.
//!
//! [`DiffMode::Exact`] compares the model as it is, provenance included.
//! [`DiffMode::Semantic`] ignores what does not change the document: source
//! locations, rsids, paragraph and text ids, derived style chains, and run
//! boundaries between runs with the same properties.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{
    AbstractNumId, Block, Document, Inline, NumId, Paragraph, Rsids, RunContent, Style, StyleId,
};

use crate::{EditError, Story};

/// How strictly two documents are compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffMode {
    /// The model as it is, source locations and identities included.
    #[default]
    Exact,
    /// What the document says: provenance, ids, derived caches and run
    /// boundaries between equal runs are ignored.
    Semantic,
}

/// What happened to one compared item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffChange {
    /// Only in the second document.
    Added,
    /// Only in the first document.
    Removed,
    /// In both, with different content.
    Modified,
    /// In both (the same paragraph identity), at another place in its story.
    Moved,
}

/// One difference between two documents.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffItem {
    /// A top-level block of a story present in both documents; the indices
    /// are its places in the first and the second document.
    Block {
        /// The story.
        story: Story,
        /// Index in the first document.
        before: Option<usize>,
        /// Index in the second document.
        after: Option<usize>,
        /// What happened.
        change: DiffChange,
    },
    /// A whole story: a header/footer part or a note only one document has.
    Story {
        /// The story.
        story: Story,
        /// Added or removed.
        change: DiffChange,
    },
    /// A style definition.
    Style {
        /// Style id.
        id: StyleId,
        /// What happened.
        change: DiffChange,
    },
    /// An abstract numbering definition.
    AbstractNum {
        /// Its id.
        id: AbstractNumId,
        /// What happened.
        change: DiffChange,
    },
    /// A list instance.
    Num {
        /// Its id.
        id: NumId,
        /// What happened.
        change: DiffChange,
    },
    /// The document defaults (`w:docDefaults`) differ.
    Defaults,
    /// A body section, by index.
    Section {
        /// Index in the first document.
        before: Option<usize>,
        /// Index in the second document.
        after: Option<usize>,
        /// What happened.
        change: DiffChange,
    },
}

/// Every difference between two documents, stories first, in document order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentDiff {
    /// The differences.
    pub items: Vec<DiffItem>,
}
impl DocumentDiff {
    /// Whether the documents compare equal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The largest block-alignment table (blocks before × blocks after) of one
/// story; past it the comparison is refused rather than cut short.
const MAX_CELLS: usize = 16 * 1024 * 1024;

/// Compares `before` with `after`.
///
/// # Errors
///
/// [`EditError::LimitExceeded`] when a story is too long to align within
/// the comparison's budget.
pub fn diff(
    before: &Document,
    after: &Document,
    mode: DiffMode,
) -> Result<DocumentDiff, EditError> {
    let mut items = Vec::new();
    blocks(
        &Story::Body,
        &before.body.blocks,
        &after.body.blocks,
        mode,
        &mut items,
    )?;
    let headers_before: BTreeMap<_, _> = before
        .headers_footers
        .iter()
        .map(|h| (h.part.as_str().to_owned(), h))
        .collect();
    let headers_after: BTreeMap<_, _> = after
        .headers_footers
        .iter()
        .map(|h| (h.part.as_str().to_owned(), h))
        .collect();
    for key in keys(&headers_before, &headers_after) {
        match (headers_before.get(&key), headers_after.get(&key)) {
            (Some(a), Some(b)) => blocks(
                &Story::HeaderFooter(a.part.clone()),
                &a.blocks,
                &b.blocks,
                mode,
                &mut items,
            )?,
            (Some(a), None) => items.push(DiffItem::Story {
                story: Story::HeaderFooter(a.part.clone()),
                change: DiffChange::Removed,
            }),
            (None, Some(b)) => items.push(DiffItem::Story {
                story: Story::HeaderFooter(b.part.clone()),
                change: DiffChange::Added,
            }),
            (None, None) => {}
        }
    }
    for endnote in [false, true] {
        let (a, b) = if endnote {
            (&before.endnotes, &after.endnotes)
        } else {
            (&before.footnotes, &after.footnotes)
        };
        let ids: BTreeSet<i32> = a.iter().chain(b.iter()).map(|note| note.id).collect();
        for id in ids {
            let story = if endnote {
                Story::Endnote(id)
            } else {
                Story::Footnote(id)
            };
            match (a.get(id), b.get(id)) {
                (Some(x), Some(y)) => blocks(&story, &x.blocks, &y.blocks, mode, &mut items)?,
                (Some(_), None) => items.push(DiffItem::Story {
                    story,
                    change: DiffChange::Removed,
                }),
                (None, Some(_)) => items.push(DiffItem::Story {
                    story,
                    change: DiffChange::Added,
                }),
                (None, None) => {}
            }
        }
    }
    definitions(before, after, mode, &mut items);
    Ok(DocumentDiff { items })
}

fn keys<K: Ord + Clone, A, B>(a: &BTreeMap<K, A>, b: &BTreeMap<K, B>) -> BTreeSet<K> {
    a.keys().chain(b.keys()).cloned().collect()
}

/// Styles, numbering, defaults and sections.
fn definitions(before: &Document, after: &Document, mode: DiffMode, items: &mut Vec<DiffItem>) {
    let style_ids: BTreeSet<StyleId> = before
        .styles
        .ids()
        .chain(after.styles.ids())
        .cloned()
        .collect();
    for id in style_ids {
        let change = compare(
            before.styles.get(&id).map(|s| style_key(s, mode)),
            after.styles.get(&id).map(|s| style_key(s, mode)),
        );
        if let Some(change) = change {
            items.push(DiffItem::Style { id, change });
        }
    }
    let abstract_ids: BTreeSet<AbstractNumId> = before
        .numbering
        .abstracts()
        .chain(after.numbering.abstracts())
        .map(|a| a.id)
        .collect();
    for id in abstract_ids {
        let key = |document: &Document| {
            document.numbering.abstract_num(id).map(|a| {
                let mut a = a.clone();
                if mode == DiffMode::Semantic {
                    a.location = SourceLocation::unknown();
                }
                a
            })
        };
        if let Some(change) = compare(key(before), key(after)) {
            items.push(DiffItem::AbstractNum { id, change });
        }
    }
    let num_ids: BTreeSet<NumId> = before
        .numbering
        .nums()
        .chain(after.numbering.nums())
        .map(|n| n.num_id)
        .collect();
    for id in num_ids {
        let key = |document: &Document| {
            document.numbering.num(id).map(|n| {
                let mut n = n.clone();
                if mode == DiffMode::Semantic {
                    n.location = SourceLocation::unknown();
                }
                n
            })
        };
        if let Some(change) = compare(key(before), key(after)) {
            items.push(DiffItem::Num { id, change });
        }
    }
    if before.styles.defaults() != after.styles.defaults() {
        items.push(DiffItem::Defaults);
    }
    let sections = before.sections.len().max(after.sections.len());
    for index in 0..sections {
        let key = |document: &Document| {
            document.sections.get(index).map(|s| {
                if mode == DiffMode::Semantic {
                    (s.properties.clone(), SourceLocation::unknown())
                } else {
                    (s.properties.clone(), s.location.clone())
                }
            })
        };
        let (a, b) = (key(before), key(after));
        if let Some(change) = compare(a.as_ref(), b.as_ref()) {
            items.push(DiffItem::Section {
                before: (index < before.sections.len()).then_some(index),
                after: (index < after.sections.len()).then_some(index),
                change,
            });
        }
    }
}

/// Added, removed or modified, or `None` when the two are equal.
fn compare<T: PartialEq>(a: Option<T>, b: Option<T>) -> Option<DiffChange> {
    match (a, b) {
        (Some(a), Some(b)) if a == b => None,
        (Some(_), Some(_)) => Some(DiffChange::Modified),
        (Some(_), None) => Some(DiffChange::Removed),
        (None, Some(_)) => Some(DiffChange::Added),
        (None, None) => None,
    }
}

fn style_key(style: &Style, mode: DiffMode) -> Style {
    let mut style = style.clone();
    if mode == DiffMode::Semantic {
        style.location = SourceLocation::unknown();
        style.based_on_chain.clear();
    }
    style
}

/// The identity of a top-level paragraph, uppercased.
fn identity(block: &Block) -> Option<String> {
    match block {
        Block::Paragraph(p) => p
            .para_id
            .as_ref()
            .map(|id| id.as_str().to_ascii_uppercase()),
        _ => None,
    }
}

/// Aligns one story's blocks and reports what changed.
fn blocks(
    story: &Story,
    before: &[Block],
    after: &[Block],
    mode: DiffMode,
    items: &mut Vec<DiffItem>,
) -> Result<(), EditError> {
    if before.len().saturating_mul(after.len()) > MAX_CELLS {
        return Err(EditError::LimitExceeded);
    }
    let old_keys: Vec<Block> = before.iter().map(|b| canonical(b, mode)).collect();
    let new_keys: Vec<Block> = after.iter().map(|b| canonical(b, mode)).collect();
    let old_ids: Vec<Option<String>> = before.iter().map(identity).collect();
    let new_ids: Vec<Option<String>> = after.iter().map(identity).collect();
    // Two blocks align when they carry the same identity, or when neither
    // carries one and they are equal.
    let pairs = align(before.len(), after.len(), |old, new| {
        match (old_ids.get(old), new_ids.get(new)) {
            (Some(Some(left)), Some(Some(right))) => left == right,
            (Some(None), Some(None)) => old_keys.get(old) == new_keys.get(new),
            _ => false,
        }
    });
    let paired_old: BTreeSet<usize> = pairs.iter().map(|pair| pair.0).collect();
    let paired_new: BTreeSet<usize> = pairs.iter().map(|pair| pair.1).collect();
    // Identities on both sides that the subsequence did not pair moved.
    let unpaired_new: HashMap<&str, usize> = new_ids
        .iter()
        .enumerate()
        .filter(|(index, _)| !paired_new.contains(index))
        .filter_map(|(index, id)| id.as_deref().map(|id| (id, index)))
        .collect();
    let moved: Vec<(usize, usize)> = old_ids
        .iter()
        .enumerate()
        .filter(|(index, _)| !paired_old.contains(index))
        .filter_map(|(index, id)| {
            id.as_deref()
                .and_then(|id| unpaired_new.get(id))
                .map(|&target| (index, target))
        })
        .collect();
    let moved_old: BTreeSet<usize> = moved.iter().map(|pair| pair.0).collect();
    let moved_new: BTreeSet<usize> = moved.iter().map(|pair| pair.1).collect();
    let block = |old: Option<usize>, new: Option<usize>, change| DiffItem::Block {
        story: story.clone(),
        before: old,
        after: new,
        change,
    };
    for &(old, new) in pairs.iter().chain(&moved) {
        if moved_old.contains(&old) {
            items.push(block(Some(old), Some(new), DiffChange::Moved));
        }
        if old_keys.get(old) != new_keys.get(new) {
            items.push(block(Some(old), Some(new), DiffChange::Modified));
        }
    }
    for old in (0..before.len()).filter(|i| !paired_old.contains(i) && !moved_old.contains(i)) {
        items.push(block(Some(old), None, DiffChange::Removed));
    }
    for new in (0..after.len()).filter(|i| !paired_new.contains(i) && !moved_new.contains(i)) {
        items.push(block(None, Some(new), DiffChange::Added));
    }
    Ok(())
}

/// A longest common subsequence of `old_len` and `new_len` items under
/// `aligns`, as index pairs in order.
fn align(
    old_len: usize,
    new_len: usize,
    aligns: impl Fn(usize, usize) -> bool,
) -> Vec<(usize, usize)> {
    // table[old * width + new]: the subsequence length from (old, new) on.
    let width = new_len + 1;
    let at = |old: usize, new: usize| old * width + new;
    let mut table = vec![0_u32; (old_len + 1) * width];
    for old in (0..old_len).rev() {
        for new in (0..new_len).rev() {
            let value = if aligns(old, new) {
                table.get(at(old + 1, new + 1)).copied().unwrap_or(0) + 1
            } else {
                let skip_old = table.get(at(old + 1, new)).copied().unwrap_or(0);
                let skip_new = table.get(at(old, new + 1)).copied().unwrap_or(0);
                skip_old.max(skip_new)
            };
            if let Some(slot) = table.get_mut(at(old, new)) {
                *slot = value;
            }
        }
    }
    let mut pairs = Vec::new();
    let (mut old, mut new) = (0, 0);
    while old < old_len && new < new_len {
        if aligns(old, new) {
            pairs.push((old, new));
            old += 1;
            new += 1;
        } else if table.get(at(old + 1, new)) >= table.get(at(old, new + 1)) {
            old += 1;
        } else {
            new += 1;
        }
    }
    pairs
}

/// The block as compared: unchanged for [`DiffMode::Exact`]; for
/// [`DiffMode::Semantic`], without source locations, rsids and identities,
/// and with equal neighbouring text runs joined.
fn canonical(block: &Block, mode: DiffMode) -> Block {
    let mut block = block.clone();
    if mode == DiffMode::Semantic {
        semantic_block(&mut block);
    }
    block
}

fn semantic_block(block: &mut Block) {
    match block {
        Block::Paragraph(p) => semantic_paragraph(p),
        Block::Table(t) => {
            t.location = SourceLocation::unknown();
            for row in &mut t.rows {
                row.location = SourceLocation::unknown();
                for cell in &mut row.cells {
                    cell.location = SourceLocation::unknown();
                    for inner in &mut cell.blocks {
                        semantic_block(inner);
                    }
                }
            }
        }
        Block::SdtBlock(sdt) => {
            for inner in &mut sdt.blocks {
                semantic_block(inner);
            }
        }
        _ => {}
    }
}

fn semantic_paragraph(p: &mut Paragraph) {
    p.location = SourceLocation::unknown();
    p.rsids = Rsids::default();
    p.para_id = None;
    p.text_id = None;
    let mut joined: Vec<Inline> = Vec::with_capacity(p.inlines.len());
    for inline in std::mem::take(&mut p.inlines) {
        let mut inline = inline;
        if let Inline::Run(run) = &mut inline {
            run.location = SourceLocation::unknown();
            if let (Some(Inline::Run(last)), [RunContent::Text(next)]) =
                (joined.last_mut(), run.content.as_slice())
            {
                if last.props == run.props && last.revision == run.revision {
                    if let [RunContent::Text(text)] = last.content.as_mut_slice() {
                        text.text.push_str(&next.text);
                        continue;
                    }
                }
            }
        }
        joined.push(inline);
    }
    // Joined text keeps no trace of where its pieces came from.
    for inline in &mut joined {
        if let Inline::Run(run) = inline {
            for content in &mut run.content {
                if let RunContent::Text(text) = content {
                    text.space = strict_ooxml_wml::model::Space::Preserve;
                }
            }
        }
    }
    p.inlines = joined;
}
