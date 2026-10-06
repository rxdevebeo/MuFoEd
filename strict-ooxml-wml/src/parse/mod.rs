//! Event-driven WordprocessingML Strict parser.
//!
//! The parser is a recursive-descent consumer of [`XmlReader`] events
//! (ADR-0004). It is split into small modules by element family; each module
//! adds inherent methods to the internal `PartParser`.
//!
//! Entry point: [`parse_document`].

mod depth;

pub mod dispatch;
pub mod document;
pub mod drawing;
pub mod fonts;

pub use fonts::LOST_FONT_PART;
pub mod headerfooter;
pub mod interner;
pub mod math;
pub mod notes;
pub mod numbering;
pub mod props;
pub mod settings;
pub mod styles;
pub mod table;
pub mod theme;

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::{Result, SourceLocation, StrictError};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::rels::{RelId, RelType, Relationship};
use strict_ooxml_core::opc::Package;
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent, XmlReader};

use crate::model::block::Block;
use crate::model::document::{Document, DocumentSource, HeaderFooter};
use crate::model::drawing::MediaIndex;
use crate::model::props::Section;
use crate::model::styles::StyleTable;
use crate::model::support::{SupportModel, SupportStatus};
use crate::model::theme::Theme;
use crate::model::{Body, FontTable, NoteTable, NumberingTable, Settings};

use self::interner::Interner;

/// Namespace of `w14` extensions (`w14:paraId`, `w14:textId`).
pub(crate) const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

/// Namespace of `w15` extensions (`w15:docId`, `w15:chartTrackingRefBased`).
pub(crate) const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

/// Namespace of Markup Compatibility and Extensibility (MCE).
pub(crate) const MCE_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// The reserved `xml:` namespace URI.
pub(crate) const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Options controlling document parsing.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    /// Resource limits applied while reading parts.
    pub limits: ResourceLimits,
}

/// Parses the WordprocessingML Strict parts of `package` into an immutable
/// [`Document`].
///
/// Runs both phases (parse + resolve). The conformance policy is no longer
/// this function's concern (AUD-23 / ADR-0016): `Package::open_*` is the only
/// place a [`ConformancePolicy`] is weighed, through
/// `opc::policy::decide`, so a `Package` that opened at all has already
/// cleared that gate. What this function still enforces is that every part it
/// reads actually arrives in the WordprocessingML Strict namespace — see
/// [`PartParser::expect_root_ns`].
///
/// # Errors
///
/// Returns a [`StrictError`] for a non-Strict root element, malformed XML, a
/// resource-limit violation or an unresolved required reference.
pub fn parse_document(package: &Package, options: &ParseOptions) -> Result<Document> {
    let main = package.main_document_part()?.clone();

    let styles_part = find_related_part(package, &main, &RelType::Styles);
    let numbering_part = find_related_part(package, &main, &RelType::Numbering);
    let settings_part = find_related_part(package, &main, &RelType::Settings);
    let font_table_part = find_related_part(package, &main, &RelType::FontTable);
    let footnotes_part = find_related_part(package, &main, &RelType::Footnotes);
    let endnotes_part = find_related_part(package, &main, &RelType::Endnotes);
    let theme_part = find_related_part(package, &main, &RelType::Theme);

    let mut parser = PartParser::new(
        package,
        main.clone(),
        package.read_part(&main)?,
        &options.limits,
    )?;
    let mut body = parser.parse_document_root()?;
    let body_section = std::mem::take(&mut parser.body_section);
    let mut media = std::mem::take(&mut parser.media);
    let mut support = std::mem::take(&mut parser.support);
    let section_gutter_at_top = std::mem::take(&mut parser.section_gutter_at_top);
    drop(parser);

    // AUD-40: collect sections with the same document-order walk that sync uses,
    // so nested containers (sdt / table cells / flattened revisions) stay aligned.
    let mut sections = collect_sections_from_body(&mut body, body_section, &mut support);

    let headers_footers = parse_decoration_parts(
        package,
        &main,
        &mut sections,
        &mut body,
        options,
        &mut support,
        &mut media,
    )?;

    let (styles, numbering, settings, font_table, aux_support) = parse_auxiliary(
        package,
        styles_part.as_ref(),
        numbering_part.as_ref(),
        settings_part.as_ref(),
        font_table_part.as_ref(),
        options,
    )?;
    support.merge(aux_support);

    // `w:gutterAtTop` in a `w:sectPr`, which is where Transitional puts it and
    // where Strict has no slot for it. An OR, not an overwrite: it is a
    // document-wide setting, so `settings.xml` saying it and the last section
    // saying it cannot disagree.
    let settings = merge_gutter_at_top(settings, section_gutter_at_top);

    let (footnotes, endnotes, note_support, note_media) = parse_notes_parts(
        package,
        footnotes_part.as_ref(),
        endnotes_part.as_ref(),
        options,
    )?;
    support.merge(note_support);
    // AUD-48: footnotes/endnotes discover media the same way headers do.
    merge_media(&mut media, &note_media);

    let theme = parse_theme_part(package, theme_part.as_ref(), options, &mut support)?;

    let mut document = Document {
        body,
        styles: styles.unwrap_or_default(),
        numbering: numbering.unwrap_or_default(),
        footnotes,
        endnotes,
        settings,
        font_table,
        theme,
        sections,
        headers_footers,
        media,
        support,
        source: DocumentSource {
            main_document: main,
            styles: styles_part,
            numbering: numbering_part,
            settings: settings_part,
            font_table: font_table_part,
            footnotes: footnotes_part,
            endnotes: endnotes_part,
            theme: theme_part,
        },
    };
    crate::resolve::resolve(&mut document, package, options)?;
    Ok(document)
}

/// Parses one auxiliary part, returning its table and support model.
fn parse_part_with<T>(
    package: &Package,
    part: &PartId,
    options: &ParseOptions,
    parse: impl FnOnce(&mut PartParser<'_>) -> Result<T>,
) -> Result<(T, SupportModel)> {
    let mut parser = PartParser::new(
        package,
        part.clone(),
        package.read_part(part)?,
        &options.limits,
    )?;
    let value = parse(&mut parser)?;
    Ok((value, std::mem::take(&mut parser.support)))
}

#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_aux_styles(
    package: &Package,
    part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(Option<StyleTable>, SupportModel)> {
    let Some(part) = part else {
        return Ok((None, SupportModel::new()));
    };
    let (table, support) = parse_part_with(package, part, options, |p| p.parse_styles_root())?;
    Ok((Some(table), support))
}

#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_aux_numbering(
    package: &Package,
    part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(Option<NumberingTable>, SupportModel)> {
    let Some(part) = part else {
        return Ok((None, SupportModel::new()));
    };
    let (table, support) = parse_part_with(package, part, options, |p| p.parse_numbering_root())?;
    Ok((Some(table), support))
}

#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_aux_settings(
    package: &Package,
    part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(Option<Settings>, SupportModel)> {
    let Some(part) = part else {
        return Ok((None, SupportModel::new()));
    };
    let (settings, support) = parse_part_with(package, part, options, |p| p.parse_settings_root())?;
    Ok((Some(settings), support))
}

/// The optional auxiliary parts and their merged support model.
type AuxiliaryParts = (
    Option<StyleTable>,
    Option<NumberingTable>,
    Option<Settings>,
    Option<FontTable>,
    SupportModel,
);

/// Parses the independent `styles`/`numbering`/`settings`/`fontTable` parts.
///
/// Under `feature = "parallel"` the styles and numbering parts are parsed
/// concurrently with `rayon` (STAGE-2 §4.4).
fn parse_auxiliary(
    package: &Package,
    styles_part: Option<&PartId>,
    numbering_part: Option<&PartId>,
    settings_part: Option<&PartId>,
    font_table_part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<AuxiliaryParts> {
    #[cfg(feature = "parallel")]
    {
        let (styles_result, numbering_result) = rayon::join(
            || parse_aux_styles(package, styles_part, options),
            || parse_aux_numbering(package, numbering_part, options),
        );
        let (styles, styles_support) = styles_result?;
        let (numbering, numbering_support) = numbering_result?;
        let (settings, settings_support) = parse_aux_settings(package, settings_part, options)?;
        let (fonts, fonts_support) = parse_aux_font_table(package, font_table_part, options)?;
        let mut support = styles_support;
        support.merge(numbering_support);
        support.merge(settings_support);
        support.merge(fonts_support);
        Ok((styles, numbering, settings, fonts, support))
    }
    #[cfg(not(feature = "parallel"))]
    {
        let (styles, styles_support) = parse_aux_styles(package, styles_part, options)?;
        let (numbering, numbering_support) = parse_aux_numbering(package, numbering_part, options)?;
        let (settings, settings_support) = parse_aux_settings(package, settings_part, options)?;
        let (fonts, fonts_support) = parse_aux_font_table(package, font_table_part, options)?;
        let mut support = styles_support;
        support.merge(numbering_support);
        support.merge(settings_support);
        support.merge(fonts_support);
        Ok((styles, numbering, settings, fonts, support))
    }
}

/// Folds a `w:gutterAtTop` found in a `w:sectPr` into the document's settings.
///
/// Transitional puts the flag inside the section properties and Strict declares it
/// in `CT_Settings`, so the parser has to move it. An **OR**, not an overwrite: it
/// is a document-wide setting, so `settings.xml` saying it and the last section
/// saying it cannot disagree, and taking either one's word for the other would
/// lose a flag one of them set.
fn merge_gutter_at_top(settings: Option<Settings>, from_section: bool) -> Settings {
    settings.map_or_else(
        || Settings {
            gutter_at_top: from_section,
            ..Settings::default()
        },
        |mut settings| {
            settings.gutter_at_top |= from_section;
            settings
        },
    )
}

/// Parses `word/fontTable.xml` and the embedded fonts it names.
///
/// A missing part is `None` and an empty one is `Some(empty)`, because the
/// difference is the writer's: the first is a document without a font table, the
/// second a document whose font table carried nothing this model keeps.
#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_aux_font_table(
    package: &Package,
    part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(Option<FontTable>, SupportModel)> {
    let Some(part) = part else {
        return Ok((None, SupportModel::new()));
    };
    let (table, support) = parse_part_with(package, part, options, |p| p.parse_font_table_root())?;
    Ok((Some(table), support))
}

/// Parses the independent `footnotes`/`endnotes` parts and returns their tables.
///
/// Media discovered inside note bodies is returned separately so
/// [`parse_document`] can [`merge_media`] it into the document index (AUD-48).
fn parse_notes_parts(
    package: &Package,
    footnotes_part: Option<&PartId>,
    endnotes_part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(NoteTable, NoteTable, SupportModel, MediaIndex)> {
    let mut media = MediaIndex::new();
    let (footnotes, footnotes_support) = match footnotes_part {
        Some(part) => parse_note_part(package, part, options, &mut media, |parser| {
            parser.parse_footnotes_root().map(|(table, _)| table)
        })?,
        None => (NoteTable::new(), SupportModel::new()),
    };
    let (endnotes, endnotes_support) = match endnotes_part {
        Some(part) => parse_note_part(package, part, options, &mut media, |parser| {
            parser.parse_endnotes_root().map(|(table, _)| table)
        })?,
        None => (NoteTable::new(), SupportModel::new()),
    };
    let mut support = footnotes_support;
    support.merge(endnotes_support);
    Ok((footnotes, endnotes, support, media))
}

/// Parses one notes part, merging its media into `media`.
fn parse_note_part<T>(
    package: &Package,
    part: &PartId,
    options: &ParseOptions,
    media: &mut MediaIndex,
    parse: impl FnOnce(&mut PartParser<'_>) -> Result<T>,
) -> Result<(T, SupportModel)> {
    let mut parser = PartParser::new(
        package,
        part.clone(),
        package.read_part(part)?,
        &options.limits,
    )?;
    let value = parse(&mut parser)?;
    let support = std::mem::take(&mut parser.support);
    let part_media = std::mem::take(&mut parser.media);
    drop(parser);
    merge_media(media, &part_media);
    Ok((value, support))
}

/// Parses the theme part into a [`Theme`], if present.
#[allow(clippy::redundant_closure_for_method_calls)]
fn parse_theme_part(
    package: &Package,
    theme_part: Option<&PartId>,
    options: &ParseOptions,
    support: &mut SupportModel,
) -> Result<Option<Theme>> {
    let Some(part) = theme_part else {
        return Ok(None);
    };
    let (theme, theme_support) =
        parse_part_with(package, part, options, |parser| parser.parse_theme_root())?;
    support.merge(theme_support);
    Ok(Some(theme))
}

/// Finds the first part related to `source` (or the package root) by type.
fn find_related_part(package: &Package, source: &PartId, rel_type: &RelType) -> Option<PartId> {
    let root = PartId::new("/");
    for current in [source, &root] {
        if let Some(part) = package
            .relationships(current)
            .iter()
            .find(|rel| &rel.rel_type == rel_type)
            .and_then(|rel: &Relationship| rel.resolved.clone())
        {
            return Some(part);
        }
    }
    None
}

/// Discovers and parses the header/footer parts referenced by the sections.
///
/// Parts are resolved by relationship type, never by file name (STAGE-5 §5.1).
/// A referenced part that is absent from the package is an error carrying the
/// location of the reference; an unresolved or mistyped relationship is recorded
/// as `Partial` and left unresolved (the resolve phase re-reports it).
#[allow(clippy::too_many_arguments)]
fn parse_decoration_parts(
    package: &Package,
    main: &PartId,
    sections: &mut [Section],
    body: &mut Body,
    options: &ParseOptions,
    support: &mut SupportModel,
    media: &mut MediaIndex,
) -> Result<Vec<HeaderFooter>> {
    let mut decorations: Vec<HeaderFooter> = Vec::new();
    let mut seen: HashMap<PartId, usize> = HashMap::new();
    for section in sections.iter_mut() {
        let location = section.location.clone();

        let headers = std::mem::take(&mut section.properties.headers);
        let mut restored = Vec::with_capacity(headers.len());
        for mut reference in headers {
            reference.part = resolve_decoration_reference(
                package,
                main,
                &reference.rel_id,
                true,
                &location,
                options,
                support,
                media,
                &mut decorations,
                &mut seen,
            )?;
            restored.push(reference);
        }
        section.properties.headers = restored;

        let footers = std::mem::take(&mut section.properties.footers);
        let mut restored = Vec::with_capacity(footers.len());
        for mut reference in footers {
            reference.part = resolve_decoration_reference(
                package,
                main,
                &reference.rel_id,
                false,
                &location,
                options,
                support,
                media,
                &mut decorations,
                &mut seen,
            )?;
            restored.push(reference);
        }
        section.properties.footers = restored;
    }
    // A section's properties exist twice in the model: once in `sections`, and
    // once on the `w:pPr` of the paragraph that ends the section — the parser
    // clones the former out of the latter. Only the `sections` copy was being
    // resolved, so the paragraph copy kept `part: None`, and the writer — which
    // has to emit the mid-document `w:sectPr` from the paragraph copy — dropped
    // every header and footer reference of every section except the last. That
    // is not a difference a byte comparison notices, it is a page that loses
    // its footer; the corpus document `2024_application_form_ en.docx` was the
    // one that showed it.
    sync_resolved_sections_into_body(body, sections);
    Ok(decorations)
}

/// Walks paragraphs in document order through body blocks, `SdtBlock`, nested
/// table cells, and (once modelled) revision containers.
///
/// Collection of `sections` and [`sync_resolved_sections_into_body`] must use
/// this same walk so a `w:sectPr` inside an sdt or cell stays aligned with the
/// paragraph that carries it (AUD-40).
pub(crate) fn walk_paragraphs_in_order(
    blocks: &mut [Block],
    visit: &mut impl FnMut(&mut crate::model::block::Paragraph),
) {
    walk_paragraphs_in_order_ctx(blocks, false, &mut |paragraph, _in_cell| visit(paragraph));
}

fn walk_paragraphs_in_order_ctx(
    blocks: &mut [Block],
    in_table_cell: bool,
    visit: &mut impl FnMut(&mut crate::model::block::Paragraph, bool),
) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => visit(paragraph, in_table_cell),
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        walk_paragraphs_in_order_ctx(&mut cell.blocks, true, visit);
                    }
                }
            }
            Block::SdtBlock(sdt) => {
                walk_paragraphs_in_order_ctx(&mut sdt.blocks, in_table_cell, visit);
            }
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
}

/// Builds `sections` from every paragraph-level `w:sectPr` in document order,
/// then appends the body-level `w:sectPr` last.
fn collect_sections_from_body(
    body: &mut Body,
    body_section: Option<Section>,
    support: &mut SupportModel,
) -> Vec<Section> {
    let mut sections = Vec::new();
    walk_paragraphs_in_order_ctx(&mut body.blocks, false, &mut |paragraph, in_table_cell| {
        if let Some(props) = &paragraph.props.section {
            if in_table_cell {
                support.record(
                    "w:sectPr",
                    SupportStatus::Partial,
                    Some("section break inside a table cell".to_owned()),
                    props
                        .location
                        .clone()
                        .or_else(|| Some(paragraph.location.clone())),
                );
            }
            sections.push(Section {
                properties: props.clone(),
                location: props
                    .location
                    .clone()
                    .unwrap_or_else(|| paragraph.location.clone()),
            });
        }
    });
    if let Some(section) = body_section {
        sections.push(section);
    }
    sections
}

/// Copies resolved section properties back onto the paragraphs that carry them.
///
/// `sections` is built in document order from the paragraph-level `w:sectPr`
/// first and the body-level one last, so the *n*-th paragraph-level `sectPr`
/// is `sections[n]`.
fn sync_resolved_sections_into_body(body: &mut Body, sections: &[Section]) {
    let mut next = 0;
    walk_paragraphs_in_order(&mut body.blocks, &mut |paragraph| {
        if paragraph.props.section.is_some() {
            if let Some(section) = sections.get(next) {
                paragraph.props.section = Some(section.properties.clone());
            }
            next += 1;
        }
    });
}

/// Resolves one section header/footer reference to a parsed [`HeaderFooter`].
#[allow(clippy::too_many_arguments)]
fn resolve_decoration_reference(
    package: &Package,
    main: &PartId,
    rel_id: &RelId,
    is_header: bool,
    reference_location: &SourceLocation,
    options: &ParseOptions,
    support: &mut SupportModel,
    media: &mut MediaIndex,
    decorations: &mut Vec<HeaderFooter>,
    seen: &mut HashMap<PartId, usize>,
) -> Result<Option<PartId>> {
    let feature = if is_header {
        "w:headerReference"
    } else {
        "w:footerReference"
    };
    let expected = if is_header {
        RelType::Header
    } else {
        RelType::Footer
    };
    let Ok(relationship) = package.resolve_relationship(main, rel_id.as_str()) else {
        // Left unresolved; `resolve::rels` records it with the section location.
        return Ok(None);
    };
    if relationship.rel_type != expected {
        support.record(
            feature,
            SupportStatus::Partial,
            Some(format!(
                "relationship '{rel_id}' is not a {} part",
                if is_header { "header" } else { "footer" }
            )),
            Some(reference_location.clone()),
        );
        return Ok(None);
    }
    let Some(part) = relationship.resolved.clone() else {
        support.record(
            feature,
            SupportStatus::Partial,
            Some(format!("relationship '{rel_id}' has no resolved target")),
            Some(reference_location.clone()),
        );
        return Ok(None);
    };
    if package.part(&part).is_none() {
        return Err(StrictError::MissingReferencedPart {
            part,
            location: reference_location.clone(),
        });
    }
    if seen.contains_key(&part) {
        return Ok(Some(part));
    }

    let bytes = package.read_part(&part)?;
    let mut parser = PartParser::new(package, part.clone(), bytes, &options.limits)?;
    let (blocks, location) = if is_header {
        parser.parse_header_root()?
    } else {
        parser.parse_footer_root()?
    };
    let decoration_media = std::mem::take(&mut parser.media);
    let decoration_support = std::mem::take(&mut parser.support);
    drop(parser);
    support.merge(decoration_support);
    merge_media(media, &decoration_media);

    let index = decorations.len();
    decorations.push(HeaderFooter {
        part: part.clone(),
        is_header,
        blocks,
        location,
    });
    seen.insert(part.clone(), index);
    Ok(Some(part))
}

/// Merges media items discovered in an auxiliary part into the document index.
fn merge_media(target: &mut MediaIndex, source: &MediaIndex) {
    for item in source.iter() {
        target.insert(item.clone());
    }
}

/// Internal parser state for one part.
pub(crate) struct PartParser<'a> {
    pub(crate) reader: XmlReader,
    pub(crate) package: &'a Package,
    pub(crate) part: PartId,
    pub(crate) interner: Interner,
    pub(crate) support: SupportModel,
    pub(crate) media: MediaIndex,
    /// The XML recursion guard; see [`depth::Depth`].
    ///
    /// A type of its own rather than two fields, because its `enter`/`leave` are
    /// private to `parse::depth` and every parser file goes through
    /// [`PartParser::nested`](Self::nested) instead (AUD-07).
    recursion: depth::Depth,
    /// Deepest block-container nesting seen so far, and the bound it is held to.
    ///
    /// Kept apart from [`depth`](Self::depth) on purpose: `depth` counts XML
    /// elements the parser recursed through, and a table cell is seven of them
    /// before the next table starts. A document of 40 nested tables is 280 XML
    /// levels - past `max_xml_depth`, but the overflow happens long before that,
    /// because each level is a stack frame of parser state. This counter counts
    /// the containers themselves, and 12 of them is what a 1 MiB stack (the main
    /// thread on Windows) takes with room to spare.
    pub(crate) block_depth: u32,
    pub(crate) max_block_nesting: u32,
    /// Deepest text-box nesting, and the bound it is held to.
    ///
    /// A text box is ten frames of parser state where a table is one, measured
    /// at 125 408 bytes per level in a debug build - so a 1 MiB stack carries
    /// six and no setting of `max_block_nesting` changes that. Its own counter
    /// is the honest answer: one bound for both would either overflow the stack
    /// or refuse twelve-deep tables over a text box the document never had.
    pub(crate) text_box_depth: u32,
    pub(crate) max_text_box_nesting: u32,
    /// The per-formula budgets, from `ResourceLimits`.
    ///
    /// Kept on the parser rather than as constants in `parse/math.rs`, because
    /// G-4 of the rework plan says a limit that is not in `ResourceLimits` is not
    /// a limit the caller has: a host with its own stack and its own appetite for
    /// a long formula needs to be able to say so.
    pub(crate) max_math_nodes: u32,
    pub(crate) max_math_depth: u32,
    /// `w:gutterAtTop` seen inside a `w:sectPr`, where Transitional puts it.
    ///
    /// Strict has no slot for it there - `EG_SectPrContents` does not declare it
    /// and `CT_Settings` does, at position 20 - so the flag belongs on
    /// [`Settings`]. The main document and `settings.xml` are parsed by two
    /// different parsers, so the flag is parked here and merged into the settings
    /// by [`parse_document`] once both are in hand. Two spellings, one flag: a
    /// Strict document that already carries it in `settings.xml` sets the same
    /// field through [`parse_settings_root`], and the merge is an OR because a
    /// document-wide setting cannot be true in one section and false in another.
    pub(crate) section_gutter_at_top: bool,
    /// When true, a bare `w:sectPr` child of the current block sequence is the
    /// body-level final section (AUD-40). Off for headers, notes, sdt content,
    /// and table cells — those must not steal the document's trailing sectPr.
    pub(crate) capture_body_section: bool,
    /// Body-level `w:sectPr` captured while `capture_body_section` is set.
    pub(crate) body_section: Option<Section>,
}

impl<'a> PartParser<'a> {
    /// Creates a parser over one part's bytes.
    pub(crate) fn new(
        package: &'a Package,
        part: PartId,
        bytes: Vec<u8>,
        limits: &'a ResourceLimits,
    ) -> Result<Self> {
        let reader = XmlReader::from_vec(bytes, part.clone(), limits)?;
        Ok(Self {
            reader,
            package,
            part,
            interner: Interner::new(),
            support: SupportModel::with_limit(limits.max_support_features),
            media: MediaIndex::new(),
            recursion: depth::Depth::new(limits.max_xml_depth),
            block_depth: 0,
            max_block_nesting: limits.max_block_nesting,
            text_box_depth: 0,
            max_text_box_nesting: limits.max_text_box_nesting,
            max_math_nodes: limits.max_math_nodes,
            max_math_depth: limits.max_math_depth,
            section_gutter_at_top: false,
            capture_body_section: false,
            body_section: None,
        })
    }

    /// Advances to the next XML event.
    pub(crate) fn next_event(&mut self) -> Result<XmlEvent> {
        self.reader.next_event()
    }

    /// Returns the location of the most recently returned event.
    pub(crate) fn location(&self) -> SourceLocation {
        self.reader.last_event_location()
    }

    /// Consumes the prolog and the root `StartElement` of a part.
    ///
    /// XML permits whitespace, comments, processing instructions and the
    /// declaration between the document start and the root element. The reader
    /// has already dropped comments, processing instructions and the
    /// declaration, so only whitespace-only `Text`/`CData` events remain; these
    /// are skipped. Non-whitespace text or any other leading element is
    /// malformed input.
    ///
    /// The root element must always be in the WML Strict namespace: either
    /// the package was already Strict, or a normalizer rewrote it on the way
    /// in — `Package::open_*` is the only place that lets anything else
    /// through (AUD-23 / ADR-0016, point 5). The root occurs once, so this is
    /// not recursive.
    pub(crate) fn expect_root(&mut self, expected_local: &str) -> Result<()> {
        self.expect_root_ns(expected_local, crate::WML_STRICT_NS)
    }

    /// Like [`expect_root`](Self::expect_root) but for another schema namespace
    /// (for example DrawingML for `theme1.xml`).
    pub(crate) fn expect_root_ns(&mut self, expected_local: &str, namespace: &str) -> Result<()> {
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } if name.local() == expected_local => {
                    if name.ns.as_ref().is_some_and(|ns| ns == namespace) {
                        self.record_root_ignorable(expected_local, namespace, &attrs);
                        return Ok(());
                    }
                    return Err(self.root_namespace_error());
                }
                XmlEvent::StartElement { name, .. } => {
                    return Err(self.invalid(format!(
                        "expected '{expected_local}' root element, found '{}'",
                        name.local()
                    )));
                }
                XmlEvent::Text(text) | XmlEvent::CData(text) if is_prolog_whitespace(&text) => {}
                XmlEvent::Text(_) | XmlEvent::CData(_) => {
                    return Err(self.invalid(format!(
                        "unexpected character data before '{expected_local}' root element"
                    )));
                }
                XmlEvent::EndElement { .. } | XmlEvent::Eof => {
                    return Err(self.invalid(format!("expected '{expected_local}' root element")));
                }
            }
        }
    }

    fn record_root_ignorable(&mut self, local: &str, namespace: &str, attrs: &[Attr]) {
        for attr in attrs {
            if attr.name.local() != "Ignorable" {
                continue;
            }
            let prefix = if namespace == crate::WML_STRICT_NS {
                "w"
            } else if namespace == crate::DRAWINGML_STRICT_NS {
                "a"
            } else {
                "w"
            };
            self.record(
                &format!("{prefix}:{local}@Ignorable"),
                SupportStatus::Partial,
                Some("mc:Ignorable is not rewritten onto Strict roots".to_owned()),
                Some(self.location()),
            );
        }
    }

    /// The error for a root element that parsed but is not in the namespace
    /// [`expect_root_ns`](Self::expect_root_ns) required (AUD-23 / ADR-0016,
    /// point 5).
    ///
    /// A package `opc::policy::decide` let open with `conformance() ==
    /// Transitional` and no normalizer cannot actually reach here: that cell
    /// of the matrix requires a normalizer, and the normalizer that made
    /// detection say `Transitional` always touched the very part whose root
    /// carried the signal, so [`Package::was_normalized`] is already `true`
    /// by the time parsing starts. The `TransitionalNotSupported` arm is kept
    /// anyway, defensively, for a normalizer that chose not to rewrite the
    /// part it was handed; every other case — a `Strict`- or `Unknown`-by-signal
    /// package whose actual root sits in some unrelated namespace — is
    /// `InvalidXml`.
    fn root_namespace_error(&self) -> StrictError {
        if !self.package.was_normalized() && self.package.conformance() == Conformance::Transitional
        {
            StrictError::TransitionalNotSupported {
                location: self.location(),
            }
        } else {
            self.invalid("root element is not in the WordprocessingML Strict namespace")
        }
    }

    /// Confirms that the part ended where its root element ended.
    ///
    /// Every root parser stops at the root's closing tag by design, which leaves
    /// everything after it unread. An unread tail is where a second root
    /// element, a truncated one and a stray end tag all live, and the reader
    /// cannot judge what nobody asks it for. Draining the reader puts the whole
    /// part under the same well-formedness check as its content.
    ///
    /// lint-eof: this arm is the success case. This is the one function in the
    /// crate whose `Eof` arm does not fail - reaching the end of a well-formed
    /// part is what it is asking for.
    pub(crate) fn expect_end_of_part(&mut self) -> Result<()> {
        loop {
            match self.next_event()? {
                XmlEvent::Eof => return Ok(()),
                XmlEvent::Text(text) | XmlEvent::CData(text) if text.trim().is_empty() => {}
                _ => return Err(self.invalid("content after the root element")),
            }
        }
    }

    /// Interns a string, returning a shared handle.
    pub(crate) fn intern(&mut self, value: &str) -> Arc<str> {
        self.interner.intern(value)
    }

    /// Records a feature usage in the support model.
    pub(crate) fn record(
        &mut self,
        feature_id: &str,
        status: SupportStatus,
        message: Option<String>,
        location: Option<SourceLocation>,
    ) {
        let feature: Arc<str> = self.intern(feature_id);
        self.support.record(feature, status, message, location);
    }

    /// Builds a malformed-input error at the current location.
    pub(crate) fn invalid(&self, detail: impl Into<String>) -> StrictError {
        StrictError::InvalidXml {
            location: self.location(),
            detail: detail.into(),
        }
    }

    /// Records an invalid enumerated value as a support entry.
    pub(crate) fn record_enum(&mut self, element: &str, value: &str, location: &SourceLocation) {
        self.record(
            element,
            SupportStatus::Partial,
            Some(format!("invalid value '{value}'; schema default applied")),
            Some(location.clone()),
        );
    }

    /// Consumes the current element and all of its descendants.
    ///
    /// The caller must have already consumed the element's `StartElement`.
    pub(crate) fn skip_element(&mut self) -> Result<()> {
        let mut depth = 1u32;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { .. } => depth = depth.saturating_add(1),
                XmlEvent::EndElement { .. } => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                XmlEvent::Eof => return Err(self.invalid("unexpected end of document")),
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
            }
        }
    }

    /// Runs `f` inside one level of block nesting.
    ///
    /// The counter is left alone on every path out, so a part that fails
    /// half-way through does not leave the parser deeper than it found it — the
    /// same property [`enter`/`leave`](Self::nested) has to earn by hand.
    ///
    /// The check is on entry and the error is for the whole document rather than
    /// a skipped subtree: a document nested past the limit is hostile input, and
    /// silently flattening its tables would be a report nobody reads.
    pub(crate) fn nested_block<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let depth = self.block_depth.saturating_add(1);
        if depth > self.max_block_nesting {
            return Err(StrictError::LimitExceeded {
                kind: strict_ooxml_core::error::LimitKind::BlockNesting,
                limit: u64::from(self.max_block_nesting),
                actual: u64::from(depth),
            });
        }
        self.block_depth = depth;
        let out = f(self);
        self.block_depth = self.block_depth.saturating_sub(1);
        out
    }

    /// Runs `f` inside one level of text-box nesting.
    ///
    /// The text-box counterpart of [`nested_block`](Self::nested_block), with its
    /// own counter for the measured reason: a text box is a paragraph, a run, a
    /// drawing, an inline, a graphic, a graphic-data, a shape, the box and its
    /// block children, and one of those costs a seventh of the stack a table does.
    ///
    /// Past [`max_text_box_nesting`](crate::parse::PartParser::max_text_box_nesting)
    /// the container is skipped and the result is `Ok(None)`: the box costs its
    /// content, not the document, the same trade AUD-06 makes for a formula.
    /// Skipping is iterative, so it spends no stack however deep the rest goes.
    pub(crate) fn nested_text_box<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<Option<T>> {
        let depth = self.text_box_depth.saturating_add(1);
        if depth > self.max_text_box_nesting {
            self.skip_element()?;
            return Ok(None);
        }
        self.text_box_depth = depth;
        let out = f(self);
        self.text_box_depth = self.text_box_depth.saturating_sub(1);
        out.map(Some)
    }

    /// Whether `name` opens a container whose children are themselves blocks.
    ///
    /// The list is what [`PartParser::nested_block`] counts. `w:sdt` is here
    /// rather than `w:sdtContent` because the count is taken at block dispatch,
    /// where the two are the same element; a row-level or cell-level `w:sdt`
    /// holds rows and cells, not blocks, and is dispatched elsewhere.
    ///
    /// `w:txbxContent` is not here: a text box is counted by
    /// [`nested_text_box`](Self::nested_text_box) against its own budget, which
    /// is a seventh of this one.
    ///
    /// `w:comment` is listed by the plan and absent from the table because no
    /// `comments.xml` is read yet (`CORE-QUEUE.md`): there is no recursion to
    /// count. `v:textbox` is absent for the same reason from the other side -
    /// the normalizer rewrites VML to DrawingML, and an unnormalized VML shape
    /// is skipped whole.
    pub(crate) fn counts_block_nesting(name: &QName) -> bool {
        matches!(
            name.local(),
            "tbl" | "sdt" | "customXml" | "footnote" | "endnote"
        )
    }

    /// The current recursion depth; `#[cfg(test)]` so the property test can read
    /// what the parser thinks it is holding.
    #[cfg(test)]
    pub(crate) fn debug_depth(&self) -> u32 {
        self.recursion.current()
    }
    /// Parses `w:start` into the model's range, recording a clamp.
    ///
    /// `w:start` is `ST_DecimalNumber`: the schema allows a negative value and the
    /// model stores a `u32`, because the renderer's counter is one. A producer that
    /// wrote `-1`, or one that wrote a decimal fraction, gets the nearest number the
    /// model can hold, and the report says so - the alternative was a silent
    /// substitution, or a `u32` counter that wraps to zero mid-document (AUD-09).
    pub(crate) fn clamped_start(&mut self, raw: &str) -> u32 {
        /// The model's counter is a `u32` that the renderer increments per item, so
        /// a value it cannot hold is a value that would wrap mid-document.
        const CEILING: i64 = i32::MAX as i64;

        let Some(parsed) = raw.trim().parse::<i64>().ok() else {
            self.record(
                "w:start",
                SupportStatus::Partial,
                Some(format!(
                    "w:start is {raw:?}, which is not an integer; the level starts at 0"
                )),
                Some(self.location()),
            );
            return 0;
        };
        let clamped = parsed.clamp(0, CEILING);
        if clamped != parsed {
            self.record(
                "w:start",
                SupportStatus::Partial,
                Some(format!(
                    "w:start {parsed} is outside 0..={CEILING}; it was clamped to {clamped}"
                )),
                Some(self.location()),
            );
        }
        u32::try_from(clamped).unwrap_or(0)
    }

    /// Parses `w:ilvl` into `0..=8`, recording a clamp when the value is higher.
    ///
    /// The schema accepts any `ST_DecimalNumber`, but only nine levels exist.
    /// A silent clamp would make `ilvl="12"` indistinguishable from a real level 8
    /// (AUD-47).
    pub(crate) fn clamped_ilvl(&mut self, raw: Option<u32>) -> crate::model::ids::Ilvl {
        use crate::model::ids::Ilvl;
        let Some(value) = raw else {
            return Ilvl(0);
        };
        if value > u32::from(Ilvl::MAX) {
            self.record(
                "w:ilvl",
                SupportStatus::Partial,
                Some(format!(
                    "w:ilvl {value} exceeds {}; it was clamped to {}",
                    Ilvl::MAX,
                    Ilvl::MAX
                )),
                Some(self.location()),
            );
            return Ilvl(Ilvl::MAX);
        }
        Ilvl(u8::try_from(value).unwrap_or(Ilvl::MAX))
    }

    /// Resolves a relationship declared by the current part to a target part.
    pub(crate) fn resolve_relationship_target(&self, rel_id: &str) -> Option<PartId> {
        self.package
            .resolve_relationship(&self.part, rel_id)
            .ok()
            .and_then(|rel| rel.resolved.clone())
    }

    /// Returns the content type of a package part.
    pub(crate) fn content_type(&self, part: &PartId) -> Option<Arc<str>> {
        self.package.content_type(part).map(Arc::from)
    }
}

/// Returns `true` if `text` consists only of XML whitespace.
///
/// XML's `S` production is exactly space, tab, carriage return and line feed;
/// these are the only characters legal in the prolog before the root.
fn is_prolog_whitespace(text: &str) -> bool {
    text.bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Returns `true` if a field instruction is computed by the renderer
/// (PAGE/NUMPAGES/SECTIONPAGES). Every other field is rendered from its cache.
pub(crate) fn field_is_computed(instruction: &str) -> bool {
    matches!(
        instruction
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
            .as_str(),
        "PAGE" | "NUMPAGES" | "SECTIONPAGES" | "SECTIONPAGE"
    )
}

/// Returns `true` if a qualified name is in the WML Strict namespace.
pub(crate) fn is_wml(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::WML_STRICT_NS)
}

/// Returns `true` if a qualified name is in the DrawingML Strict namespace.
pub(crate) fn is_drawingml(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::DRAWINGML_STRICT_NS)
}

/// Returns `true` if a qualified name is in the OMML Strict namespace
/// (`STAGE-5C-TASK.md` §5.1). OMML is part of ISO/IEC 29500-1, so the Strict
/// namespace is the only one accepted for formulas.
pub(crate) fn is_math(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::MATH_STRICT_NS)
}

/// Builds a stable feature identifier from a qualified name.
/// Builds a support feature id as `{namespace-key}:{local}` (AUD-51).
///
/// The key comes from the resolved namespace URI, never from the document's
/// prefix — a `w:` bound to a foreign URI becomes `ext:…`, not `w:…`.
pub(crate) fn feature_id_for(name: &QName) -> String {
    let key = namespace_feature_key(
        name.ns
            .as_ref()
            .map(strict_ooxml_core::xml::qname::NsUri::as_str),
    );
    format!("{key}:{}", name.local())
}

/// Short registry key for a namespace URI, or `ext:<uri>` when unknown.
fn namespace_feature_key(uri: Option<&str>) -> String {
    match uri {
        None => "ext".to_owned(),
        Some(uri) => match uri {
            crate::WML_STRICT_NS
            | "http://schemas.openxmlformats.org/wordprocessingml/2006/main" => "w".to_owned(),
            crate::RELS_STRICT_NS
            | "http://schemas.openxmlformats.org/officeDocument/2006/relationships" => {
                "r".to_owned()
            }
            crate::DRAWINGML_STRICT_NS
            | "http://schemas.openxmlformats.org/drawingml/2006/main" => "a".to_owned(),
            crate::WORDPROCESSING_DRAWING_STRICT_NS
            | "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" => {
                "wp".to_owned()
            }
            crate::PICTURE_STRICT_NS
            | "http://schemas.openxmlformats.org/drawingml/2006/picture" => "pic".to_owned(),
            crate::MATH_STRICT_NS
            | "http://schemas.openxmlformats.org/officeDocument/2006/math" => "m".to_owned(),
            crate::WORD_PROCESSING_SHAPE_STRICT_NS
            | "http://schemas.microsoft.com/office/word/2010/wordprocessingShape" => {
                "wps".to_owned()
            }
            crate::WORD_PROCESSING_GROUP_STRICT_NS
            | "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" => {
                "wpg".to_owned()
            }
            "http://schemas.openxmlformats.org/markup-compatibility/2006" => "mc".to_owned(),
            other => format!("ext:{other}"),
        },
    }
}

/// Looks up an unprefixed-or-WML attribute by local name in the WML namespace.
pub(crate) fn wml_attr<'a>(attrs: &'a [Attr], local: &str) -> Option<&'a str> {
    attr_in_ns(attrs, crate::WML_STRICT_NS, local)
}

/// Looks up a `w:val` attribute.
pub(crate) fn val_attr(attrs: &[Attr]) -> Option<&str> {
    wml_attr(attrs, "val")
}

/// Looks up an attribute in the given namespace by local name.
pub(crate) fn attr_in_ns<'a>(attrs: &'a [Attr], namespace: &str, local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| {
            attr.name.local() == local && attr.name.ns.as_ref().is_some_and(|ns| ns == namespace)
        })
        .map(|attr| attr.value.as_str())
}

/// Looks up an unprefixed attribute by local name.
pub(crate) fn plain_attr<'a>(attrs: &'a [Attr], local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.name.ns.is_none() && attr.name.local() == local)
        .map(|attr| attr.value.as_str())
}

/// Parses an `i32` attribute value.
pub(crate) fn parse_i32(value: &str) -> Option<i32> {
    value.trim().parse().ok()
}

/// Parses a `u32` attribute value.
pub(crate) fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse().ok()
}

/// Parses a finite decimal lexical value.
pub(crate) fn parse_decimal(value: &str) -> Option<f64> {
    let number: f64 = value.trim().parse().ok()?;
    number.is_finite().then_some(number)
}

/// Rounds a decimal to the nearest `i32`, saturating at the type bounds.
pub(crate) fn decimal_to_i32(number: f64) -> i32 {
    let rounded = number.round();
    if rounded >= f64::from(i32::MAX) {
        i32::MAX
    } else if rounded <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        rounded as i32
    }
}

/// Parses `ST_SignedTwipsMeasure`: twips, or a universal measure with a unit.
///
/// Real producer markup writes decimals (`1872.0000000000002`, `-180.0`) and,
/// notably, physical units (`545.30pt`, `72pt`) for page size/margins; the value
/// is converted to the model's whole-twip representation rather than being
/// dropped (STAGE-2-WORK-ORDER D-2, STAGE-4-RENDER-FIDELITY).
pub(crate) fn parse_signed_twips(value: &str) -> Option<i32> {
    parse_measure_twips(value).map(decimal_to_i32)
}

/// Parses a `ST_UniversalMeasure`-style value into twips as a float.
///
/// A bare number is already in twips; a unit suffix (`mm`, `cm`, `in`, `pt`,
/// `pc`, `pi`) is converted. Returns `None` for an unknown unit.
fn parse_measure_twips(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let (number, unit) = match trimmed
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_alphabetic())
    {
        Some((index, _)) => (&trimmed[..index], Some(trimmed[index..].trim())),
        None => (trimmed, None),
    };
    let number = parse_decimal(number)?;
    let Some(unit) = unit else {
        return Some(number);
    };
    let per_unit = match unit {
        "in" => 1440.0,
        "cm" => 1440.0 / 2.54,
        "mm" => 144.0 / 2.54,
        "pt" => 20.0,
        "pc" | "pi" => 240.0,
        _ => return None,
    };
    Some(number * per_unit)
}

/// Parses `ST_MeasurementOrPercent` (widths, `w:tblInd`, `w:wBefore/After`,
/// `w:gridCol`): a percentage (`50%`), a universal measure (`178.05pt`), or a
/// bare number in the unit the element's `w:type` implies.
///
/// A percent is converted to the OOXML fiftieths-of-a-percent unit.
///
/// The universal-measure branch is not a nicety. `ST_MeasurementOrPercent` is
/// `union(ST_DecimalNumberOrPercent, s:ST_UniversalMeasure)` and its first branch
/// is `s:ST_Percentage` alone, whose pattern requires the `%` - so a bare number is
/// not a value the attribute can hold, and the producers that write valid Strict
/// write points: LibreOffice and docx4j put `w:tcW w:w="178.05pt"` where the
/// Microsoft conformance fixtures put `w:w="4788"` and fail their own schema. This
/// function used to accept only the percentage and the bare number, which meant
/// every table width, table indent and before/after width in a LibreOffice or
/// docx4j document was parsed as nothing at all, and the write dropped them in
/// silence.
pub(crate) fn parse_measurement_or_percent(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return parse_decimal(percent).map(|number| decimal_to_i32(number * 50.0));
    }
    if let Some(number) = parse_decimal(trimmed) {
        return Some(decimal_to_i32(number));
    }
    parse_measure_twips(trimmed).map(decimal_to_i32)
}

/// Parses `ST_TextScale` (`w:w` in a run property bag): a percentage with the
/// `%` sign the pattern demands, or the bare integer every Transitional producer
/// writes.
///
/// Both spellings are the same number - a percentage of the normal character
/// width - and the bare one is not a Strict value, so it is accepted on the way
/// in and never written back out. The range is the schema's: the pattern is
/// `0*(600|([0-5]?[0-9]?[0-9]))%`, so 601% is not a value the attribute can hold
/// and a document claiming it is recorded rather than silently clamped.
///
/// `Q-E5` is the same shape of question one level down, and is closed in
/// [`crate::model::values::percent_from_fiftieths`].
pub(crate) fn parse_text_scale(value: &str) -> Option<u16> {
    let trimmed = value.trim();
    let percent = match trimmed.strip_suffix('%') {
        Some(number) => parse_u32(number)?,
        None => parse_u32(trimmed)?,
    };
    u16::try_from(percent)
        .ok()
        .filter(|scale| *scale <= crate::model::values::TEXT_SCALE_MAX)
}

/// Parses a `CT_OnOff` value (AUD-44).
///
/// - missing `w:val`, or `true`/`1`/`on` → `Some(true)`
/// - `false`/`0`/`off` → `Some(false)`
/// - anything else → `None` (caller records the defect)
pub(crate) fn parse_on_off(attrs: &[Attr]) -> Option<bool> {
    match val_attr(attrs) {
        None => Some(true),
        Some(value) => match value {
            "true" | "on" | "1" => Some(true),
            "false" | "off" | "0" => Some(false),
            _ => None,
        },
    }
}

/// Parses a `CT_OnOff` into [`TriState`], recording invalid values.
pub(crate) fn parse_on_off_tristate(
    parser: &mut PartParser<'_>,
    attrs: &[Attr],
    feature: &str,
) -> crate::model::values::TriState {
    use crate::model::values::TriState;
    match parse_on_off(attrs) {
        Some(true) => TriState::On,
        Some(false) => TriState::Off,
        None => {
            if let Some(value) = val_attr(attrs) {
                parser.record(
                    feature,
                    crate::model::support::SupportStatus::Partial,
                    Some(format!("invalid on/off value {value:?}")),
                    Some(parser.location()),
                );
            }
            TriState::Absent
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod measure_tests {
    use super::parse_signed_twips;

    #[test]
    fn universal_measures_convert_to_twips() {
        assert_eq!(parse_signed_twips("360"), Some(360));
        assert_eq!(parse_signed_twips("545.30pt"), Some(10_906));
        assert_eq!(parse_signed_twips("72pt"), Some(1440));
        assert_eq!(parse_signed_twips("1in"), Some(1440));
        assert_eq!(parse_signed_twips("2.54cm"), Some(1440));
        assert_eq!(parse_signed_twips("25.4mm"), Some(1440));
        assert_eq!(parse_signed_twips("-180.0"), Some(-180));
        assert_eq!(parse_signed_twips("12zz"), None);
        assert_eq!(parse_signed_twips("abc"), None);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use strict_ooxml_core::limits::ResourceLimits;
    use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
    use strict_ooxml_core::part::PartId;

    use super::PartParser;

    /// Every `.docx` of the committed Strict corpus.
    fn corpus() -> Vec<std::path::PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn the_recursion_guard_is_back_at_zero_after_every_part_of_the_corpus() {
        // The property AUD-07 is about. A leaked `enter()` does not fail where it
        // happens: it makes the *next* part fail, or the hundredth sibling, with
        // a depth error that says nothing about the input that caused it. Two
        // hundred and fifty-six is not a number any corpus document reaches, so
        // the only way to see it is to read the counter back.
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let limits = ResourceLimits::default();
        let files = corpus();
        assert!(!files.is_empty(), "the Strict corpus is not there");

        let mut parts = 0usize;
        for path in &files {
            let Ok(package) = Package::open_path(path, &options) else {
                continue;
            };
            let Ok(main) = package.main_document_part().cloned() else {
                continue;
            };
            let Ok(bytes) = package.read_part(&main) else {
                continue;
            };
            let Ok(mut parser) = PartParser::new(&package, main.clone(), bytes, &limits) else {
                continue;
            };
            // Any outcome is fine; the counter is what is being checked.
            let _ = parser.parse_document_root();
            assert_eq!(
                parser.debug_depth(),
                0,
                "{} left the recursion guard open in {}",
                main,
                path.display()
            );
            parts += 1;
        }
        assert!(parts > 0, "no part of the corpus was parsed");
    }

    #[test]
    fn the_guard_does_not_leak_across_repeated_parses_of_one_part() {
        // The same observation from the outside: a part that parses once must
        // parse the next hundred times too. With a leak, the second parse of a
        // part whose siblings are counted would already be refused.
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let limits = ResourceLimits::default();
        let Some(path) = corpus().into_iter().next() else {
            return;
        };
        let Ok(package) = Package::open_path(&path, &options) else {
            return;
        };
        let Ok(main) = package.main_document_part().cloned() else {
            return;
        };
        let Ok(bytes) = package.read_part(&main) else {
            return;
        };
        for round in 0..100 {
            let mut parser = PartParser::new(&package, PartId::new(MAIN), bytes.clone(), &limits)
                .expect("reader");
            let _ = parser.parse_document_root();
            assert_eq!(
                parser.debug_depth(),
                0,
                "round {round} of {} left the guard open",
                path.display()
            );
        }
    }

    const MAIN: &str = "/word/document.xml";

    /// One paragraph holding one formula of every construct `math.rs` parses.
    ///
    /// The corpus does not exercise all of them in one part, and the property
    /// above only sees the parsers a corpus document happens to reach. This is
    /// the fixture that makes the guard's accounting visible over `math.rs`'s own
    /// twenty-one functions.
    fn every_formula() -> String {
        let fraction = "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
                        <m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>";
        let radical = "<m:rad><m:deg/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>";
        let scripts = "<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>\
                       <m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>\
                       <m:sSubSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub>\
                       <m:sup><m:r><m:t>j</m:t></m:r></m:sup></m:sSubSup>\
                       <m:sPre><m:sub><m:r><m:t>n</m:t></m:r></m:sub><m:sup><m:r><m:t>1</m:t></m:r></m:sup>\
                       <m:e><m:r><m:t>F</m:t></m:r></m:e></m:sPre>";
        let nary = "<m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr>\
                    <m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>\
                    <m:e><m:r><m:t>a</m:t></m:r></m:e></m:nary>";
        let delimited = "<m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"]\"/></m:dPr>\
                        <m:e><m:r><m:t>x</m:t></m:r></m:e><m:e><m:r><m:t>y</m:t></m:r></m:e></m:d>";
        let functions = "<m:func><m:funcPr/><m:fName><m:r><m:t>sin</m:t></m:r></m:fName>\
                        <m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>";
        let limits = "<m:limLow><m:e><m:r><m:t>x</m:t></m:r></m:e><m:lim><m:r><m:t>0</m:t></m:r></m:lim></m:limLow>\
                      <m:limUpp><m:e><m:r><m:t>n</m:t></m:r></m:e><m:lim><m:r><m:t>∞</m:t></m:r></m:lim></m:limUpp>";
        let matrix = "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:mcJc m:val=\"center\"/></m:mcPr></m:mc></m:mcs></m:mPr>\
                      <m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e></m:mr></m:m>";
        let array = "<m:eqArr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>2</m:t></m:r></m:e></m:eqArr>";
        let decorations = "<m:acc><m:accPr><m:chr m:val=\"^\"/></m:accPr><m:e><m:r><m:t>v</m:t></m:r></m:e></m:acc>\
                          <m:bar><m:barPr><m:pos m:val=\"top\"/></m:barPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:bar>\
                          <m:groupChr><m:groupChrPr><m:chr m:val=\"⏞\"/></m:groupChrPr><m:e><m:r><m:t>z</m:t></m:r></m:e></m:groupChr>\
                          <m:box><m:e><m:r><m:t>b</m:t></m:r></m:e></m:box>\
                          <m:borderBox><m:e><m:r><m:t>a</m:t></m:r></m:e></m:borderBox>\
                          <m:phant><m:phantPr><m:show m:val=\"0\"/></m:phantPr><m:e><m:r><m:t>p</m:t></m:r></m:e></m:phant>";
        let argument = "<m:argPr/><m:r><m:t>a</m:t></m:r>";
        let body = [
            fraction,
            radical,
            scripts,
            nary,
            delimited,
            functions,
            limits,
            matrix,
            array,
            decorations,
        ]
        .join("");
        let _ = argument;
        format!("<w:p><m:oMath>{body}</m:oMath></w:p>")
    }

    /// Wraps `body` in a package with the namespaces a formula needs.
    fn formula_package(body: &str) -> Vec<u8> {
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"{W_NS}\" xmlns:m=\"{M_NS}\"><w:body>{body}</w:body></w:document>"
        );
        let mut zip = strict_ooxml_core::opc::zip::write::ZipWriter::new();
        zip.add_part(
            &PartId::new("/[Content_Types].xml"),
            CONTENT_TYPES.as_bytes().to_vec(),
        )
        .expect("content types");
        zip.add_part(
            &PartId::new("/_rels/.rels"),
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" ",
                "Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument\" ",
                "Target=\"word/document.xml\"/></Relationships>"
            )
            .as_bytes()
            .to_vec(),
        )
        .expect("root rels");
        zip.add_part(&PartId::new("/word/document.xml"), document.into_bytes())
            .expect("document");
        zip.finish().expect("zip")
    }

    const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    const M_NS: &str = crate::MATH_STRICT_NS;
    const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
        <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
        <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
        <Override PartName=\"/word/document.xml\" \
        ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
        </Types>";

    #[test]
    fn every_formula_construct_leaves_the_guard_at_zero() {
        let bytes = formula_package(&every_formula());
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let package = Package::open_reader(std::io::Cursor::new(bytes), &options).expect("open");
        let main = PartId::new(MAIN);
        let limits = ResourceLimits::default();
        for round in 0..10 {
            let bytes = package.read_part(&main).expect("part");
            let mut parser =
                PartParser::new(&package, main.clone(), bytes, &limits).expect("reader");
            parser.parse_document_root().expect("the formulas parse");
            assert_eq!(
                parser.debug_depth(),
                0,
                "round {round}: a formula construct left the guard open"
            );
        }
    }

    #[test]
    fn a_document_cut_short_inside_a_formula_leaves_the_guard_at_zero() {
        // The same fixture with the last formula truncated. The parse must be an
        // error and the counter must come back to zero: this is the path where a
        // `?` used to leave the guard open.
        let full = every_formula();
        let cut = &full[..full.len() / 2];
        let bytes = formula_package(cut);
        let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
        let Ok(package) = Package::open_reader(std::io::Cursor::new(bytes), &options) else {
            return;
        };
        let main = PartId::new(MAIN);
        let limits = ResourceLimits::default();
        for round in 0..10 {
            let bytes = package.read_part(&main).expect("part");
            let mut parser =
                PartParser::new(&package, main.clone(), bytes, &limits).expect("reader");
            let _ = parser.parse_document_root();
            assert_eq!(
                parser.debug_depth(),
                0,
                "round {round}: a truncated formula left the guard open"
            );
        }
    }
}
