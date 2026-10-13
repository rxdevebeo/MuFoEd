//! Section properties and section breaks of the body (K04, sections part).
//!
//! The body's sections are kept twice: `Document::sections` lists them all,
//! and every section but the last ends with a top-level body paragraph whose
//! `w:sectPr` carries the same properties. The commands here change both, so
//! the check that they agree keeps holding.
use crate::{EditError, Invariant, Unsupported};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{Block, Document, Paragraph, Section, SectionProperties};

use super::address::{Address, Story};

/// The top-level body paragraph `at` names.
fn body_paragraph(at: &Address) -> Result<usize, EditError> {
    if at.story != Story::Body || !at.containers.is_empty() {
        return Err(EditError::UnsupportedContent(Unsupported::Target));
    }
    Ok(at.block)
}

/// The index of the section the body block `block` belongs to (a boundary
/// paragraph belongs to the section it ends).
fn section_of(document: &Document, block: usize) -> usize {
    document
        .body
        .blocks
        .iter()
        .take(block)
        .filter(|b| matches!(b, Block::Paragraph(p) if p.props.section.is_some()))
        .count()
}

fn paragraph_at(document: &mut Document, block: usize) -> Result<&mut Paragraph, EditError> {
    match document.body.blocks.get_mut(block) {
        Some(Block::Paragraph(p)) => Ok(p),
        _ => Err(EditError::InvalidParagraph),
    }
}

/// The boundary paragraph that ends section `section`, if it is not the last.
fn boundary_of(document: &mut Document, section: usize) -> Option<&mut Paragraph> {
    document
        .body
        .blocks
        .iter_mut()
        .filter_map(|b| match b {
            Block::Paragraph(p) if p.props.section.is_some() => Some(p),
            _ => None,
        })
        .nth(section)
}

/// Sets the properties of the section that the body paragraph at `at`
/// belongs to (or ends), in the section list and on its boundary.
pub(super) fn set_properties(
    document: &mut Document,
    at: &Address,
    properties: &SectionProperties,
) -> Result<(), EditError> {
    let block = body_paragraph(at)?;
    paragraph_at(document, block)?;
    let index = section_of(document, block);
    let section = document
        .sections
        .get_mut(index)
        .ok_or(EditError::InvalidModel(Invariant::Sections))?;
    section.properties = properties.clone();
    if let Some(boundary) = boundary_of(document, index) {
        boundary.props.section = Some(properties.clone());
    }
    Ok(())
}

/// Ends a new section with the body paragraph at `at`: it takes the
/// properties of the section it was in, which now continues after it.
pub(super) fn insert_break(document: &mut Document, at: &Address) -> Result<(), EditError> {
    let block = body_paragraph(at)?;
    if paragraph_at(document, block)?.props.section.is_some() {
        return Err(EditError::InvalidModel(Invariant::SectionBoundary));
    }
    let index = section_of(document, block);
    let properties = document
        .sections
        .get(index)
        .map(|section| section.properties.clone())
        .ok_or(EditError::InvalidModel(Invariant::Sections))?;
    paragraph_at(document, block)?.props.section = Some(properties.clone());
    document.sections.insert(
        index,
        Section {
            properties,
            location: SourceLocation::unknown(),
        },
    );
    Ok(())
}

/// Removes the section break the body paragraph at `at` carries. The merged
/// section keeps the properties of the section before the break, or with
/// `keep_after` those of the section after it.
pub(super) fn remove_break(
    document: &mut Document,
    at: &Address,
    keep_after: bool,
) -> Result<(), EditError> {
    let block = body_paragraph(at)?;
    let properties = paragraph_at(document, block)?
        .props
        .section
        .clone()
        .ok_or(EditError::InvalidModel(Invariant::SectionBoundary))?;
    let index = section_of(document, block);
    if index + 1 >= document.sections.len() {
        return Err(EditError::InvalidModel(Invariant::Sections));
    }
    if !keep_after {
        if let Some(next) = document.sections.get_mut(index + 1) {
            next.properties = properties.clone();
        }
        if let Some(boundary) = boundary_of(document, index + 1) {
            boundary.props.section = Some(properties);
        }
    }
    paragraph_at(document, block)?.props.section = None;
    document.sections.remove(index);
    Ok(())
}
