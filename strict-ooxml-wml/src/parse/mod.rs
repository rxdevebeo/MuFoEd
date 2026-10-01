//! Event-driven WordprocessingML Strict parser.
//!
//! The parser is a recursive-descent consumer of [`XmlReader`] events
//! (ADR-0004). It is split into small modules by element family; each module
//! adds inherent methods to the internal `PartParser`.
//!
//! Entry point: [`parse_document`].

pub mod dispatch;
pub mod document;
pub mod drawing;
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
use strict_ooxml_core::opc::{ConformancePolicy, Package};
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
use crate::model::{Body, NoteTable, NumberingTable, Settings};

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
#[derive(Clone, Copy, Debug)]
pub struct ParseOptions {
    /// Conformance policy; only `Strict` input is accepted (Stage 2).
    pub conformance: ConformancePolicy,
    /// Resource limits applied while reading parts.
    pub limits: ResourceLimits,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            conformance: ConformancePolicy::StrictOnly,
            limits: ResourceLimits::default(),
        }
    }
}

/// Parses the WordprocessingML Strict parts of `package` into an immutable
/// [`Document`].
///
/// Runs both phases (parse + resolve). A Transitional package is rejected
/// unless a normalizer is installed, in which case the parts arrive already
/// normalized and the same Strict parser handles them (Stage 6).
///
/// `Mixed` is not a contradiction here. It is what stage T0 sees *before* any
/// transformation: a Transitional document whose parts have been rewritten to
/// different degrees, or a producer that mixed a Strict-native part into a
/// Transitional package. Under a normalizing policy the normalizer maps every
/// registered URI into one family, so refusing the input would make the policy
/// unable to open a single real document — which is the only reason it exists.
///
/// # Errors
///
/// Returns a [`StrictError`] for conformance mismatch, malformed XML, a
/// resource-limit violation or an unresolved required reference.
pub fn parse_document(package: &Package, options: &ParseOptions) -> Result<Document> {
    let main = package.main_document_part()?.clone();
    let normalizing = matches!(
        options.conformance,
        ConformancePolicy::Normalize | ConformancePolicy::Permissive
    );
    match package.conformance() {
        Conformance::Transitional if !normalizing => {
            return Err(StrictError::TransitionalNotSupported {
                location: SourceLocation::new(main, 1, 1, 0),
            });
        }
        Conformance::Mixed if !normalizing => {
            return Err(StrictError::MixedConformance {
                detail: "both Strict and Transitional signals were detected".to_owned(),
            });
        }
        Conformance::Strict
        | Conformance::Transitional
        | Conformance::Mixed
        | Conformance::Unknown => {}
    }

    let styles_part = find_related_part(package, &main, &RelType::Styles);
    let numbering_part = find_related_part(package, &main, &RelType::Numbering);
    let settings_part = find_related_part(package, &main, &RelType::Settings);
    let footnotes_part = find_related_part(package, &main, &RelType::Footnotes);
    let endnotes_part = find_related_part(package, &main, &RelType::Endnotes);
    let theme_part = find_related_part(package, &main, &RelType::Theme);

    let mut parser = PartParser::new(
        package,
        main.clone(),
        package.read_part(&main)?,
        &options.limits,
    )?;
    let (mut body, mut sections) = parser.parse_document_root()?;
    let mut media = std::mem::take(&mut parser.media);
    let mut support = std::mem::take(&mut parser.support);
    drop(parser);

    let headers_footers = parse_decoration_parts(
        package,
        &main,
        &mut sections,
        &mut body,
        options,
        &mut support,
        &mut media,
    )?;

    let (styles, numbering, settings, aux_support) = parse_auxiliary(
        package,
        styles_part.as_ref(),
        numbering_part.as_ref(),
        settings_part.as_ref(),
        options,
    )?;
    support.merge(aux_support);

    let (footnotes, endnotes, note_support) = parse_notes_parts(
        package,
        footnotes_part.as_ref(),
        endnotes_part.as_ref(),
        options,
    )?;
    support.merge(note_support);

    let theme = parse_theme_part(package, theme_part.as_ref(), options, &mut support)?;

    let mut document = Document {
        body,
        styles: styles.unwrap_or_default(),
        numbering: numbering.unwrap_or_default(),
        footnotes,
        endnotes,
        settings: settings.unwrap_or_default(),
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
    SupportModel,
);

/// Parses the independent `styles`/`numbering`/`settings` parts.
///
/// Under `feature = "parallel"` the styles and numbering parts are parsed
/// concurrently with `rayon` (STAGE-2 §4.4).
fn parse_auxiliary(
    package: &Package,
    styles_part: Option<&PartId>,
    numbering_part: Option<&PartId>,
    settings_part: Option<&PartId>,
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
        let mut support = styles_support;
        support.merge(numbering_support);
        support.merge(settings_support);
        Ok((styles, numbering, settings, support))
    }
    #[cfg(not(feature = "parallel"))]
    {
        let (styles, styles_support) = parse_aux_styles(package, styles_part, options)?;
        let (numbering, numbering_support) = parse_aux_numbering(package, numbering_part, options)?;
        let (settings, settings_support) = parse_aux_settings(package, settings_part, options)?;
        let mut support = styles_support;
        support.merge(numbering_support);
        support.merge(settings_support);
        Ok((styles, numbering, settings, support))
    }
}

/// Parses the independent `footnotes`/`endnotes` parts and returns their tables.
fn parse_notes_parts(
    package: &Package,
    footnotes_part: Option<&PartId>,
    endnotes_part: Option<&PartId>,
    options: &ParseOptions,
) -> Result<(NoteTable, NoteTable, SupportModel)> {
    let (footnotes, footnotes_support) = match footnotes_part {
        Some(part) => parse_part_with(package, part, options, |parser| {
            parser.parse_footnotes_root().map(|(table, _)| table)
        })?,
        None => (NoteTable::new(), SupportModel::new()),
    };
    let (endnotes, endnotes_support) = match endnotes_part {
        Some(part) => parse_part_with(package, part, options, |parser| {
            parser.parse_endnotes_root().map(|(table, _)| table)
        })?,
        None => (NoteTable::new(), SupportModel::new()),
    };
    let mut support = footnotes_support;
    support.merge(endnotes_support);
    Ok((footnotes, endnotes, support))
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

/// Copies resolved section properties back onto the paragraphs that carry them.
///
/// `sections` is built in document order from the paragraph-level `w:sectPr`
/// first and the body-level one last, so the *n*-th paragraph-level `sectPr`
/// is `sections[n]`.
fn sync_resolved_sections_into_body(body: &mut Body, sections: &[Section]) {
    fn walk(blocks: &mut [Block], sections: &[Section], next: &mut usize) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    if paragraph.props.section.is_some() {
                        if let Some(section) = sections.get(*next) {
                            paragraph.props.section = Some(section.properties.clone());
                        }
                        *next += 1;
                    }
                }
                Block::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            walk(&mut cell.blocks, sections, next);
                        }
                    }
                }
                Block::SdtBlock(sdt) => walk(&mut sdt.blocks, sections, next),
                _ => {}
            }
        }
    }
    let mut next = 0;
    walk(&mut body.blocks, sections, &mut next);
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
    pub(crate) max_depth: u32,
    pub(crate) depth: u32,
}

impl<'a> PartParser<'a> {
    /// Creates a parser over one part's bytes.
    pub(crate) fn new(
        package: &'a Package,
        part: PartId,
        bytes: Vec<u8>,
        limits: &'a ResourceLimits,
    ) -> Result<Self> {
        let max_depth = limits.max_xml_depth;
        let reader = XmlReader::from_vec(bytes, part.clone(), limits)?;
        Ok(Self {
            reader,
            package,
            part,
            interner: Interner::new(),
            support: SupportModel::new(),
            media: MediaIndex::new(),
            max_depth,
            depth: 0,
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
    /// For a package detected as Strict the root must also be in the WML Strict
    /// namespace; packages of undetermined conformance (`Unknown`) keep matching
    /// by local name so the CLI can still report the missing signal. The root
    /// occurs once, so this is not recursive.
    pub(crate) fn expect_root(&mut self, expected_local: &str) -> Result<()> {
        self.expect_root_ns(expected_local, crate::WML_STRICT_NS)
    }

    /// Like [`expect_root`](Self::expect_root) but for another schema namespace
    /// (for example DrawingML for `theme1.xml`).
    pub(crate) fn expect_root_ns(&mut self, expected_local: &str, namespace: &str) -> Result<()> {
        let require_strict_ns = self.package.conformance() == Conformance::Strict;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. }
                    if name.local() == expected_local
                        && (!require_strict_ns
                            || name.ns.as_ref().is_some_and(|ns| ns == namespace)) =>
                {
                    return Ok(());
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

    /// Recursion guard: rejects input deeper than the configured XML limit.
    pub(crate) fn enter(&mut self) -> Result<()> {
        self.depth = self.depth.saturating_add(1);
        if self.depth > self.max_depth {
            return Err(StrictError::LimitExceeded {
                kind: strict_ooxml_core::error::LimitKind::XmlDepth,
                limit: u64::from(self.max_depth),
                actual: u64::from(self.depth),
            });
        }
        Ok(())
    }

    /// Leaves the current recursion level.
    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
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
pub(crate) fn feature_id_for(name: &QName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local()),
        None => name.local().to_owned(),
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

/// Parses an on/off attribute or a bare element (default `true`).
pub(crate) fn parse_on_off(attrs: &[Attr]) -> bool {
    match val_attr(attrs) {
        None => true,
        Some(value) => matches!(value, "true" | "on" | "1"),
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
