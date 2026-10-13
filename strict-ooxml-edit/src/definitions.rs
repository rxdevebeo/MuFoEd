//! Style, numbering and default definitions as editor transactions (K02).
//!
//! A definition is document-wide: every paragraph, run and table that names a
//! style or a list may change with it. [`Editor::define`](crate::Editor::define)
//! applies a batch to a copy of the document, recomputes the derived state
//! (`basedOn` chains, `numStyleLink` redirects), checks the model and records
//! the whole document in history, so undo and redo restore definitions and
//! references together.
use crate::{EditError, Invariant};
use strict_ooxml_wml::model::{
    AbstractNum, AbstractNumId, Block, DocDefaults, Document, Graphic, Ilvl, Inline, LevelOverride,
    Num, NumId, ParagraphProperties, RunContent, RunProperties, Style, StyleId, TableProperties,
};

/// One change to the document's definitions.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Definition {
    /// Adds a style; its id must be new.
    CreateStyle(Box<Style>),
    /// Replaces the style with the same id.
    ReplaceStyle(Box<Style>),
    /// Deletes a style. One that content or a list uses is refused unless
    /// `replace_with` names a style of the same kind: every such reference
    /// moves to it. Styles based on the deleted one are based on its parent
    /// instead, as in Word; `next` and `link` to it are cleared. The default
    /// style of its kind cannot be deleted.
    DeleteStyle {
        /// The style to delete.
        id: StyleId,
        /// Where its references move.
        replace_with: Option<StyleId>,
    },
    /// Sets the document defaults (`w:docDefaults`).
    SetDocDefaults(Box<DocDefaults>),
    /// Adds an abstract numbering definition; its id must be new.
    CreateAbstractNum(Box<AbstractNum>),
    /// Replaces the abstract numbering definition with the same id.
    ReplaceAbstractNum(Box<AbstractNum>),
    /// Deletes an abstract numbering definition no list instance uses.
    DeleteAbstractNum {
        /// The definition to delete.
        id: AbstractNumId,
    },
    /// Adds a list instance; its id must be new and not 0, and its abstract
    /// definition must exist.
    CreateNum(Box<Num>),
    /// Replaces the list instance with the same id.
    ReplaceNum(Box<Num>),
    /// Deletes a list instance. One that a paragraph or a style uses is
    /// refused unless `replace_with` names another instance, which every such
    /// reference then names.
    DeleteNum {
        /// The instance to delete.
        id: NumId,
        /// Where its references move.
        replace_with: Option<NumId>,
    },
}

/// A new list instance of `num`'s abstract definition that starts level
/// `ilvl` at `start`: what Word's "Restart numbering" creates. Apply it with
/// [`Definition::CreateNum`] and point the paragraph's `w:numPr` at `id`.
#[must_use]
pub fn restarted(num: &Num, id: NumId, ilvl: Ilvl, start: u32) -> Num {
    Num {
        num_id: id,
        abstract_num_id: num.abstract_num_id,
        overrides: vec![LevelOverride {
            ilvl,
            start_override: Some(start),
            level: None,
        }],
        location: num.location.clone(),
    }
}

/// The smallest list instance id above every one in use (never 0).
#[must_use]
pub fn unused_num_id(document: &Document) -> NumId {
    NumId(
        document
            .numbering
            .nums()
            .map(|num| num.num_id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    )
}

/// The smallest abstract numbering id above every one in use.
#[must_use]
pub fn unused_abstract_num_id(document: &Document) -> AbstractNumId {
    AbstractNumId(
        document
            .numbering
            .abstracts()
            .map(|abstract_num| abstract_num.id.0.saturating_add(1))
            .max()
            .unwrap_or(0),
    )
}

const fn exists() -> EditError {
    EditError::InvalidModel(Invariant::DefinitionExists)
}
const fn missing() -> EditError {
    EditError::InvalidModel(Invariant::DefinitionMissing)
}
const fn in_use() -> EditError {
    EditError::InvalidModel(Invariant::DefinitionInUse)
}

/// Applies one definition change to `document`. Derived state is recomputed
/// by the caller once the batch is through.
pub(crate) fn apply(document: &mut Document, definition: &Definition) -> Result<(), EditError> {
    match definition {
        Definition::CreateStyle(style) => {
            if document.styles.contains(&style.id) {
                return Err(exists());
            }
            document.styles.insert((**style).clone());
        }
        Definition::ReplaceStyle(style) => {
            if !document.styles.contains(&style.id) {
                return Err(missing());
            }
            document.styles.insert((**style).clone());
        }
        Definition::DeleteStyle { id, replace_with } => {
            delete_style(document, id, replace_with.as_ref())?;
        }
        Definition::SetDocDefaults(defaults) => document.styles.set_defaults((**defaults).clone()),
        Definition::CreateAbstractNum(abstract_num) => {
            if !document.numbering.insert_abstract((**abstract_num).clone()) {
                return Err(exists());
            }
        }
        Definition::ReplaceAbstractNum(abstract_num) => {
            if !document
                .numbering
                .replace_abstract((**abstract_num).clone())
            {
                return Err(missing());
            }
        }
        Definition::DeleteAbstractNum { id } => {
            if document.numbering.abstract_num(*id).is_none() {
                return Err(missing());
            }
            if document
                .numbering
                .nums()
                .any(|num| num.abstract_num_id == *id)
            {
                return Err(in_use());
            }
            document.numbering.remove_abstract(*id);
        }
        Definition::CreateNum(num) => {
            check_num(document, num)?;
            if !document.numbering.insert_num((**num).clone()) {
                return Err(exists());
            }
        }
        Definition::ReplaceNum(num) => {
            check_num(document, num)?;
            if !document.numbering.replace_num((**num).clone()) {
                return Err(missing());
            }
        }
        Definition::DeleteNum { id, replace_with } => delete_num(document, *id, *replace_with)?,
    }
    Ok(())
}

/// A list instance must name an existing abstract definition and not use id 0,
/// which `w:numPr` reserves for "no list".
fn check_num(document: &Document, num: &Num) -> Result<(), EditError> {
    if num.num_id.0 == 0
        || document
            .numbering
            .abstract_num(num.abstract_num_id)
            .is_none()
    {
        return Err(EditError::InvalidModel(Invariant::Numbering));
    }
    Ok(())
}

fn delete_style(
    document: &mut Document,
    id: &StyleId,
    replace_with: Option<&StyleId>,
) -> Result<(), EditError> {
    let style = document.styles.get(id).cloned().ok_or_else(missing)?;
    if style.is_default {
        return Err(in_use());
    }
    if let Some(target) = replace_with {
        let same_kind = document
            .styles
            .get(target)
            .is_some_and(|other| other.style_type == style.style_type);
        if target == id || !same_kind {
            return Err(EditError::UnknownStyle(target.clone()));
        }
    }
    let mut uses = 0_usize;
    let mut remap = |slot: &mut Option<StyleId>| {
        if slot.as_ref() == Some(id) {
            uses += 1;
            slot.clone_from(&replace_with.cloned());
        }
    };
    each_site(document, &mut |site| match site {
        Site::Paragraph(props) => {
            remap(&mut props.style);
            if let Some(mark) = &mut props.run_props {
                remap(&mut mark.style);
            }
        }
        Site::Run(props) => remap(&mut props.style),
        Site::Table(props) => remap(&mut props.style),
    });
    let mut abstracts: Vec<AbstractNum> = document.numbering.abstracts().cloned().collect();
    for abstract_num in &mut abstracts {
        remap(&mut abstract_num.num_style_link);
        remap(&mut abstract_num.style_link);
        for level in &mut abstract_num.levels {
            remap(&mut level.paragraph_style);
        }
    }
    if uses > 0 && replace_with.is_none() {
        return Err(in_use());
    }
    for abstract_num in abstracts {
        document.numbering.replace_abstract(abstract_num);
    }
    let others: Vec<StyleId> = document.styles.ids().cloned().collect();
    for other in others {
        if let Some(other) = document.styles.get_mut(&other) {
            if other.based_on.as_ref() == Some(id) {
                other.based_on.clone_from(&style.based_on);
            }
            if other.next.as_ref() == Some(id) {
                other.next = None;
            }
            if other.link.as_ref() == Some(id) {
                other.link = None;
            }
        }
    }
    document.styles.remove(id);
    Ok(())
}

fn delete_num(
    document: &mut Document,
    id: NumId,
    replace_with: Option<NumId>,
) -> Result<(), EditError> {
    if document.numbering.num(id).is_none() {
        return Err(missing());
    }
    if let Some(target) = replace_with {
        if target == id || document.numbering.num(target).is_none() {
            return Err(EditError::InvalidModel(Invariant::Numbering));
        }
    }
    let mut uses = 0_usize;
    let mut remap = |props: &mut ParagraphProperties| {
        if let Some(numbering) = &mut props.numbering {
            if numbering.num_id == Some(id) {
                uses += 1;
                numbering.num_id = replace_with;
            }
        }
    };
    each_site(document, &mut |site| {
        if let Site::Paragraph(props) = site {
            remap(props);
        }
    });
    let ids: Vec<StyleId> = document.styles.ids().cloned().collect();
    for style in ids {
        if let Some(style) = document.styles.get_mut(&style) {
            remap(&mut style.paragraph);
        }
    }
    if uses > 0 && replace_with.is_none() {
        return Err(in_use());
    }
    document.numbering.remove_num(id);
    Ok(())
}

/// Where content names a definition.
enum Site<'a> {
    Paragraph(&'a mut ParagraphProperties),
    Run(&'a mut RunProperties),
    Table(&'a mut TableProperties),
}

/// Calls `f` for every paragraph, run and table of every story - in tables,
/// block SDTs, inline wrappers and the text boxes of drawings.
fn each_site(document: &mut Document, f: &mut dyn FnMut(Site<'_>)) {
    blocks(&mut document.body.blocks, f);
    for header in &mut document.headers_footers {
        blocks(&mut header.blocks, f);
    }
    for table in [&mut document.footnotes, &mut document.endnotes] {
        let ids: Vec<i32> = table.iter().map(|note| note.id).collect();
        for id in ids {
            if let Some(mut note) = table.get(id).cloned() {
                blocks(&mut note.blocks, f);
                table.insert(note);
            }
        }
    }
}

fn blocks(items: &mut [Block], f: &mut dyn FnMut(Site<'_>)) {
    for block in items {
        match block {
            Block::Paragraph(p) => {
                f(Site::Paragraph(&mut p.props));
                inlines(&mut p.inlines, f);
            }
            Block::Table(t) => {
                f(Site::Table(&mut t.props));
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        blocks(&mut cell.blocks, f);
                    }
                }
            }
            Block::SdtBlock(s) => blocks(&mut s.blocks, f),
            _ => {}
        }
    }
}

fn inlines(items: &mut [Inline], f: &mut dyn FnMut(Site<'_>)) {
    for item in items {
        match item {
            Inline::Run(run) => {
                f(Site::Run(&mut run.props));
                for content in &mut run.content {
                    if let RunContent::Drawing(drawing) = content {
                        drawing_boxes(drawing, f);
                    }
                }
            }
            Inline::Drawing(drawing) => drawing_boxes(drawing, f),
            Inline::Hyperlink(v) => inlines(&mut v.inlines, f),
            Inline::Field(v) => inlines(&mut v.inlines, f),
            Inline::SdtInline(v) => inlines(&mut v.inlines, f),
            Inline::Directional(v) => inlines(&mut v.inlines, f),
            _ => {}
        }
    }
}

fn drawing_boxes(drawing: &mut strict_ooxml_wml::model::Drawing, f: &mut dyn FnMut(Site<'_>)) {
    use strict_ooxml_wml::model::DrawingKind;
    match &mut drawing.kind {
        DrawingKind::Inline(v) => graphic_boxes(&mut v.graphic, f),
        DrawingKind::Anchor(v) => graphic_boxes(&mut v.graphic, f),
        DrawingKind::Opaque(_) => {}
    }
}

fn graphic_boxes(graphic: &mut Graphic, f: &mut dyn FnMut(Site<'_>)) {
    match graphic {
        Graphic::Shape(shape) => {
            if let Some(text) = &mut shape.text {
                blocks(&mut text.blocks, f);
            }
        }
        Graphic::Group(group) => {
            for child in &mut group.children {
                graphic_boxes(child, f);
            }
        }
        _ => {}
    }
}
