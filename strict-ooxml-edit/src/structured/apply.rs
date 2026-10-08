//! Applying one command to a candidate document.
use crate::{EditError, EditLimits};
use std::ops::Range;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{Block, Document, Inline, Paragraph, ParagraphProperties, RunContent};

use super::address::{descend_mut, drawing_mut, mutate_story, paragraph_mut, run_mut, Story};
use super::command::Edit;
use super::ids::{all_ids, assign_id, fresh_ids};
use super::validate::contains_boundary;

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
pub(super) fn apply(
    document: &mut Document,
    edit: &Edit,
    limits: EditLimits,
) -> Result<(), EditError> {
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
                let runs = crate::plain_runs(p, limits.paragraph_scalars)?;
                let count = runs.iter().map(|(_, t)| t.chars().count()).sum();
                check_range(count, &(*offset..*offset))?;
                assign_id(p, &mut ids, false)?;
                let mut right = p.clone();
                assign_id(&mut right, &mut ids, true)?;
                crate::replace_text(p, &runs, &(*offset..count), "");
                crate::replace_text(&mut right, &runs, &(0..*offset), "");
                p.props.section = None;
                blocks.insert(at.block + 1, Block::Paragraph(right));
            }
            Edit::Join { .. } => {
                let mut left = paragraph_mut(blocks, at.block)?.clone();
                let right = paragraph_mut(blocks, at.block + 1)?.clone();
                if left.props.section.is_some() {
                    return Err(EditError::InvalidModel);
                }
                let a = crate::plain_runs(&left, limits.paragraph_scalars)?;
                let b = crate::plain_runs(&right, limits.paragraph_scalars)?;
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
                let runs = crate::plain_runs(p, limits.paragraph_scalars)?;
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
                        crate::replace_text(p, &runs, range, text);
                    }
                    Edit::Format { patch, .. } => crate::format_text(p, &runs, range, patch),
                    // The outer arm admits only the variants matched above.
                    _ => return Err(EditError::UnsupportedContent),
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
                if crate::slice(&node.text, range.start, range.end) == *text {
                    return Ok(());
                }
                node.text = format!(
                    "{}{}{}",
                    crate::slice(&node.text, 0, range.start),
                    text,
                    crate::slice(&node.text, range.end, total)
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
                    // The outer arm admits only the variants matched above.
                    _ => return Err(EditError::UnsupportedContent),
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
