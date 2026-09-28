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
pub mod interner;
pub mod numbering;
pub mod props;
pub mod settings;
pub mod styles;
pub mod table;

use std::sync::Arc;

use strict_ooxml_core::error::{Result, SourceLocation, StrictError};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::rels::{RelType, Relationship};
use strict_ooxml_core::opc::{ConformancePolicy, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent, XmlReader};

use crate::model::document::{Document, DocumentSource};
use crate::model::drawing::MediaIndex;
use crate::model::styles::StyleTable;
use crate::model::support::{SupportModel, SupportStatus};
use crate::model::{NumberingTable, Settings};

use self::interner::Interner;

/// Namespace of `w14` extensions (`w14:paraId`, `w14:textId`).
pub(crate) const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

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
/// Runs both phases (parse + resolve). Only Strict input is accepted: a
/// Transitional or Mixed package yields [`StrictError::TransitionalNotSupported`]
/// / [`StrictError::MixedConformance`] regardless of the policy (normalization
/// is Stage 6).
///
/// # Errors
///
/// Returns a [`StrictError`] for conformance mismatch, malformed XML, a
/// resource-limit violation or an unresolved required reference.
pub fn parse_document(package: &Package, options: &ParseOptions) -> Result<Document> {
    let main = package.main_document_part()?.clone();
    match package.conformance() {
        Conformance::Transitional => {
            return Err(StrictError::TransitionalNotSupported {
                location: SourceLocation::new(main, 1, 1, 0),
            });
        }
        Conformance::Mixed => {
            return Err(StrictError::MixedConformance {
                detail: "both Strict and Transitional signals were detected".to_owned(),
            });
        }
        Conformance::Strict | Conformance::Unknown => {}
    }

    let styles_part = find_related_part(package, &main, &RelType::Styles);
    let numbering_part = find_related_part(package, &main, &RelType::Numbering);
    let settings_part = find_related_part(package, &main, &RelType::Settings);

    let mut parser = PartParser::new(
        package,
        main.clone(),
        package.read_part(&main)?,
        &options.limits,
    )?;
    let (body, sections) = parser.parse_document_root()?;
    let media = std::mem::take(&mut parser.media);
    let mut support = std::mem::take(&mut parser.support);
    drop(parser);

    let (styles, numbering, settings, aux_support) = parse_auxiliary(
        package,
        styles_part.as_ref(),
        numbering_part.as_ref(),
        settings_part.as_ref(),
        options,
    )?;
    support.merge(aux_support);

    let mut document = Document {
        body,
        styles: styles.unwrap_or_default(),
        numbering: numbering.unwrap_or_default(),
        settings: settings.unwrap_or_default(),
        sections,
        media,
        support,
        source: DocumentSource {
            main_document: main,
            styles: styles_part,
            numbering: numbering_part,
            settings: settings_part,
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
        let require_strict_ns = self.package.conformance() == Conformance::Strict;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. }
                    if name.local() == expected_local && (!require_strict_ns || is_wml(&name)) =>
                {
                    return Ok(());
                }
                XmlEvent::StartElement { name, .. } => {
                    return Err(self.invalid(format!(
                        "expected 'w:{expected_local}' root element, found '{}'",
                        name.local()
                    )));
                }
                XmlEvent::Text(text) | XmlEvent::CData(text) if is_prolog_whitespace(&text) => {}
                XmlEvent::Text(_) | XmlEvent::CData(_) => {
                    return Err(self.invalid(format!(
                        "unexpected character data before 'w:{expected_local}' root element"
                    )));
                }
                XmlEvent::EndElement { .. } | XmlEvent::Eof => {
                    return Err(self.invalid(format!("expected 'w:{expected_local}' root element")));
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

/// Returns `true` if a qualified name is in the WML Strict namespace.
pub(crate) fn is_wml(name: &QName) -> bool {
    name.ns
        .as_ref()
        .is_some_and(|ns| ns == crate::WML_STRICT_NS)
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

/// Parses `ST_SignedTwipsMeasure`: an integer or decimal twip value.
///
/// Real producer markup writes decimals (`1872.0000000000002`, `-180.0`); the
/// value is rounded to the model's whole-twip representation rather than being
/// dropped (STAGE-2-WORK-ORDER D-2).
pub(crate) fn parse_signed_twips(value: &str) -> Option<i32> {
    parse_decimal(value).map(decimal_to_i32)
}

/// Parses `ST_MeasurementOrPercent` (widths, `w:tblInd`, `w:wBefore/After`,
/// `w:gridCol`): a decimal measurement, or a percentage (`50%`).
///
/// A percent is converted to the OOXML fiftieths-of-a-percent unit; other forms
/// are rounded to the nearest integer in the unit implied by the element's
/// `w:type`.
pub(crate) fn parse_measurement_or_percent(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return parse_decimal(percent).map(|number| decimal_to_i32(number * 50.0));
    }
    parse_decimal(trimmed).map(decimal_to_i32)
}

/// Parses an on/off attribute or a bare element (default `true`).
pub(crate) fn parse_on_off(attrs: &[Attr]) -> bool {
    match val_attr(attrs) {
        None => true,
        Some(value) => matches!(value, "true" | "on" | "1"),
    }
}
