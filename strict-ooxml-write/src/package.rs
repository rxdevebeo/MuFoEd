//! Package assembly: parts, relationships, content types and the ZIP.
//!
//! Everything that decides *order* lives here, because order is what makes the
//! output reproducible: relationship ids are handed out in a fixed sequence,
//! media parts are named by index, and the ZIP keeps insertion order with a
//! fixed timestamp (SC-1). Nothing depends on a hash map's iteration order.

use std::collections::BTreeMap;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::content_types::ContentTypeIndex;
use strict_ooxml_core::opc::rels::{
    strict_type_uri, write_relationships, RelType, Relationship, TargetMode,
};
use strict_ooxml_core::opc::zip::write::ZipWriter;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::MediaKind;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Document;

use crate::body::blocks;
use crate::ctx::Ctx;
use crate::drawing::namespaces as drawing_namespaces;
use crate::parts;
use crate::passthrough;
use crate::props::section_properties;
use crate::xml::{XmlWriter, NS_A, NS_M, NS_PIC, NS_R, NS_W, NS_WP};

/// `/word/document.xml`.
pub const MAIN_DOCUMENT: &str = "/word/document.xml";
/// `/word/styles.xml`.
pub const STYLES_PART: &str = "/word/styles.xml";
/// `/word/numbering.xml`.
pub const NUMBERING_PART: &str = "/word/numbering.xml";
/// `/word/settings.xml`.
pub const SETTINGS_PART: &str = "/word/settings.xml";
/// `/word/fontTable.xml`.
pub const FONT_TABLE_PART: &str = "/word/fontTable.xml";
/// `/word/theme/theme1.xml`.
pub const THEME_PART: &str = "/word/theme/theme1.xml";
/// `/word/footnotes.xml`.
pub const FOOTNOTES_PART: &str = "/word/footnotes.xml";
/// `/word/endnotes.xml`.
pub const ENDNOTES_PART: &str = "/word/endnotes.xml";

const CONTENT_TYPE_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CONTENT_TYPE_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CONTENT_TYPE_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CONTENT_TYPE_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CONTENT_TYPE_FONT_TABLE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";
const CONTENT_TYPE_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CONTENT_TYPE_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CONTENT_TYPE_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const CONTENT_TYPE_HEADER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CONTENT_TYPE_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const CONTENT_TYPE_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CONTENT_TYPE_XML: &str = "application/xml";

/// A relationship target the source package knows and the model does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationshipInfo {
    /// The relationship id, as the source's `.rels` part spells it.
    pub id: String,
    /// Raw target as written in the `.rels` part.
    pub target: String,
    /// Normalized relationship type.
    pub rel_type: RelType,
    /// Whether the target is outside the package.
    pub external: bool,
}

impl RelationshipInfo {
    /// Reads a relationship of a source package.
    #[must_use]
    fn from_relationship(relationship: &Relationship) -> Self {
        Self {
            id: relationship.id.clone(),
            target: relationship.target.clone(),
            rel_type: relationship.rel_type.clone(),
            external: relationship.target_mode == TargetMode::External,
        }
    }
}

/// Where a write gets the bytes and the relationships the model does not carry.
///
/// A [`Document`] is self-contained for structure but not for payload: image
/// bytes live in the package, a hyperlink's target is only in the `.rels` part,
/// and everything behind a `c:chart` or a `dgm:relIds` reference is a producer's
/// own XML that the model deliberately does not carry. A caller that built a
/// document from scratch passes [`NoSource`] and gets a report entry for every
/// reference that cannot be resolved.
pub trait Source {
    /// Reads a part fully.
    ///
    /// Named for the part, not for its role: the same call serves an image, a
    /// chart part and an embedded workbook.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`](strict_ooxml_core::error::StrictError) when the
    /// part cannot be read.
    fn read_part(&self, part: &PartId) -> Result<Vec<u8>>;

    /// Looks up a relationship declared by `from`.
    fn relationship(&self, from: &PartId, rel_id: &str) -> Option<RelationshipInfo>;

    /// Every relationship `from` declares, in the order the `.rels` part lists
    /// them.
    ///
    /// This is what lets a write notice a part the source had and the model has
    /// no reference to — the pass-through audit (W7) is exactly that question.
    fn relationships(&self, from: &PartId) -> Vec<RelationshipInfo> {
        let _ = from;
        Vec::new()
    }

    /// The content type the source declares for a part, if any.
    fn content_type(&self, part: &PartId) -> Option<String> {
        let _ = part;
        None
    }

    /// The relationship ids `from` declares, in order.
    ///
    /// # Errors
    ///
    /// Never; the method is total.
    #[must_use]
    fn relationship_ids(&self, from: &PartId) -> Vec<String> {
        self.relationships(from)
            .into_iter()
            .map(|info| info.id)
            .collect()
    }
}

impl Source for strict_ooxml_core::opc::Package {
    fn read_part(&self, part: &PartId) -> Result<Vec<u8>> {
        self.read_part(part)
    }

    fn relationship(&self, from: &PartId, rel_id: &str) -> Option<RelationshipInfo> {
        let relationship = self
            .relationships(from)
            .iter()
            .find(|rel| rel.id == rel_id)?;
        Some(RelationshipInfo::from_relationship(relationship))
    }

    fn relationships(&self, from: &PartId) -> Vec<RelationshipInfo> {
        self.relationships(from)
            .iter()
            .map(RelationshipInfo::from_relationship)
            .collect()
    }

    fn content_type(&self, part: &PartId) -> Option<String> {
        self.content_type(part).map(ToOwned::to_owned)
    }
}

/// Media bytes, ready to be written.
///
/// The natural home for this is the writer rather than each caller: a
/// conversion produces a document whose `MediaIndex` names parts the *PDF* holds,
/// and a caller should not have to know that to close the loop.
#[derive(Clone, Debug, Default)]
pub struct MediaBag {
    bytes: BTreeMap<PartId, Vec<u8>>,
}

impl MediaBag {
    /// An empty bag.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the bytes of one part.
    pub fn insert(&mut self, part: PartId, bytes: impl Into<Vec<u8>>) {
        self.bytes.insert(part, bytes.into());
    }

    /// How many parts the bag holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the bag is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Source for MediaBag {
    fn read_part(&self, part: &PartId) -> Result<Vec<u8>> {
        self.bytes
            .get(part)
            .cloned()
            .ok_or_else(|| strict_ooxml_core::error::StrictError::MissingPart(part.clone()))
    }

    fn relationship(&self, _from: &PartId, _rel_id: &str) -> Option<RelationshipInfo> {
        None
    }
}

/// A source that has nothing: the model must be self-sufficient.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSource;

impl Source for NoSource {
    fn read_part(&self, part: &PartId) -> Result<Vec<u8>> {
        Err(strict_ooxml_core::error::StrictError::MissingPart(
            part.clone(),
        ))
    }

    fn relationship(&self, _from: &PartId, _rel_id: &str) -> Option<RelationshipInfo> {
        None
    }
}

/// Options controlling a write.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    /// Resource budget for the produced package.
    pub limits: ResourceLimits,
    /// Write `word/styles.xml` even when the style table is empty.
    ///
    /// Off by default: a document that had no styles part does not acquire one.
    pub always_write_styles: bool,
    /// Write `word/fontTable.xml` (default `true`).
    pub write_font_table: bool,
    /// Write `word/settings.xml` even when the model recorded no setting.
    pub always_write_settings: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            limits: ResourceLimits::default(),
            always_write_styles: false,
            write_font_table: true,
            always_write_settings: false,
        }
    }
}

/// The result of a write.
#[derive(Clone, Debug)]
pub struct WriteOutput {
    /// The `.docx` bytes.
    pub bytes: Vec<u8>,
    /// What the write could not express, and what it rewrote.
    pub report: crate::WriteReport,
    /// Number of parts in the package.
    pub part_count: usize,
}

/// Hands out relationship ids in a fixed order.
#[derive(Debug, Default)]
pub(crate) struct RelBuilder {
    next: u32,
    relationships: Vec<Relationship>,
}

impl RelBuilder {
    /// A builder that starts at `rId1`.
    pub(crate) fn new() -> Self {
        Self {
            next: 1,
            relationships: Vec::new(),
        }
    }

    /// Adds a relationship and returns its id.
    pub(crate) fn add(&mut self, rel_type: &RelType, target: String, external: bool) -> String {
        let id = format!("rId{}", self.next);
        self.next += 1;
        self.relationships.push(Relationship {
            id: id.clone(),
            rel_type: rel_type.clone(),
            raw_type: strict_type_uri(rel_type),
            target,
            target_mode: if external {
                TargetMode::External
            } else {
                TargetMode::Internal
            },
            resolved: None,
        });
        id
    }

    /// The relationships collected, in the order they were added.
    pub(crate) fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }
}

/// Serializes `document` into a `.docx` package.
///
/// # Errors
///
/// Returns a [`StrictError`](strict_ooxml_core::error::StrictError) when a part
/// exceeds a resource limit, when a media part cannot be read, or when the
/// writer's own depth budget is exceeded.
pub fn write_package(
    document: &Document,
    source: Option<&dyn Source>,
    options: &WriteOptions,
) -> Result<WriteOutput> {
    let source = source.unwrap_or(&NoSource);
    let mut report = crate::WriteReport::new();
    let mut ctx = Ctx::new(&mut report);

    let mut rels = RelBuilder::new();
    let mut content_types = ContentTypeIndex::new();
    content_types.insert_default("rels", CONTENT_TYPE_RELS);
    content_types.insert_default("xml", CONTENT_TYPE_XML);
    content_types.insert_override(PartId::new(MAIN_DOCUMENT), CONTENT_TYPE_MAIN);

    // Auxiliary parts first, in a fixed order, so `rId1` always names the
    // styles part when there is one.
    if !document.styles.is_empty() || options.always_write_styles {
        rels.add(&RelType::Styles, "styles.xml".to_owned(), false);
        content_types.insert_override(PartId::new(STYLES_PART), CONTENT_TYPE_STYLES);
    }
    if !document.numbering.is_empty() {
        rels.add(&RelType::Numbering, "numbering.xml".to_owned(), false);
        content_types.insert_override(PartId::new(NUMBERING_PART), CONTENT_TYPE_NUMBERING);
    }
    if document.settings != Default::default() || options.always_write_settings {
        rels.add(&RelType::Settings, "settings.xml".to_owned(), false);
        content_types.insert_override(PartId::new(SETTINGS_PART), CONTENT_TYPE_SETTINGS);
    }
    if document.theme.is_some() {
        rels.add(&RelType::Theme, "theme/theme1.xml".to_owned(), false);
        content_types.insert_override(PartId::new(THEME_PART), CONTENT_TYPE_THEME);
    }
    if !document.footnotes.is_empty() {
        rels.add(&RelType::Footnotes, "footnotes.xml".to_owned(), false);
        content_types.insert_override(PartId::new(FOOTNOTES_PART), CONTENT_TYPE_FOOTNOTES);
    }
    if !document.endnotes.is_empty() {
        rels.add(&RelType::Endnotes, "endnotes.xml".to_owned(), false);
        content_types.insert_override(PartId::new(ENDNOTES_PART), CONTENT_TYPE_ENDNOTES);
    }
    if options.write_font_table {
        rels.add(&RelType::FontTable, "fontTable.xml".to_owned(), false);
        content_types.insert_override(PartId::new(FONT_TABLE_PART), CONTENT_TYPE_FONT_TABLE);
    }

    // Headers and footers, in the order the document lists them, numbered per
    // role so a document with two headers and one footer gets header1,
    // header2 and footer1.
    let mut header_footer_map: BTreeMap<String, String> = BTreeMap::new();
    let mut header_footer_parts: Vec<String> = Vec::new();
    for header_footer in &document.headers_footers {
        let role = if header_footer.is_header {
            "header"
        } else {
            "footer"
        };
        let index = header_footer_parts
            .iter()
            .filter(|part| {
                part.rsplit('/')
                    .next()
                    .is_some_and(|name| name.starts_with(role))
            })
            .count()
            + 1;
        let name = format!("{role}{index}.xml");
        let rel_type = if header_footer.is_header {
            RelType::Header
        } else {
            RelType::Footer
        };
        let id = rels.add(&rel_type, name.clone(), false);
        header_footer_map.insert(header_footer.part.as_str().to_owned(), id);
        header_footer_parts.push(format!("/word/{name}"));
        content_types.insert_override(
            PartId::new(format!("/word/{name}").as_str()),
            if header_footer.is_header {
                CONTENT_TYPE_HEADER
            } else {
                CONTENT_TYPE_FOOTER
            },
        );
    }

    // Media, in `MediaIndex` order, which is the order the parser resolved them
    // in and is therefore stable.
    let mut media_map: BTreeMap<String, String> = BTreeMap::new();
    let mut media_parts: Vec<(String, PartId)> = Vec::new();
    for (index, item) in document.media.iter().enumerate() {
        let extension = media_extension(item.kind);
        let name = format!("media/image{}.{extension}", index + 1);
        let id = rels.add(&RelType::Image, name.clone(), false);
        media_map.insert(item.part.as_str().to_owned(), id);
        media_parts.push((format!("/word/{name}"), item.part.clone()));
        content_types.insert_default(extension, media_content_type(item.kind));
    }

    // Hyperlinks: the target only exists in the source package.
    let main = PartId::new(MAIN_DOCUMENT);
    let mut hyperlink_map: BTreeMap<String, String> = BTreeMap::new();
    for old_id in collect_hyperlink_ids(&document.body.blocks) {
        match source.relationship(&main, &old_id) {
            Some(info) => {
                let id = rels.add(&info.rel_type, info.target, info.external);
                hyperlink_map.insert(old_id, id);
            }
            None => ctx.report_unsupported(
                "w:hyperlink/@r:id",
                "hyperlink target is not available without the source package",
                &strict_ooxml_core::error::SourceLocation::unknown(),
            ),
        }
    }

    // W7: the parts this project does not model - a chart, a SmartArt diagram, a
    // workbook behind either, custom XML - are copied from the source and the
    // body's references are re-pointed at them. Without a source package the
    // references cannot be written at all, and `drawing.rs` says so per object.
    let pass = passthrough::plan(
        &mut ctx,
        source,
        &mut rels,
        &main,
        &passthrough::referenced_ids(&document.body.blocks),
    );
    for part in pass.parts() {
        if let Some(content_type) = &part.content_type {
            content_types.insert_override(PartId::new(part.name.as_str()), content_type);
        }
    }

    ctx = ctx
        .with_relationships(hyperlink_map, media_map, header_footer_map)
        .with_passthrough(&pass);

    // The parts themselves. Each is written only when the model carries the
    // content for it, so a document does not acquire parts it did not have.
    let mut zip = ZipWriter::with_limits(options.limits);
    add_part(
        &mut zip,
        MAIN_DOCUMENT,
        document_part(&mut ctx, document).into_bytes(),
    )?;
    if content_types.content_type_for(&PartId::new(STYLES_PART)) == Some(CONTENT_TYPE_STYLES) {
        add_part(
            &mut zip,
            STYLES_PART,
            parts::styles_part(&mut ctx, &document.styles).into_bytes(),
        )?;
    }
    if !document.numbering.is_empty() {
        add_part(
            &mut zip,
            NUMBERING_PART,
            parts::numbering_part(&mut ctx, &document.numbering).into_bytes(),
        )?;
    }
    if document.settings != Default::default() || options.always_write_settings {
        add_part(
            &mut zip,
            SETTINGS_PART,
            parts::settings_part(&mut ctx, &document.settings).into_bytes(),
        )?;
    }
    if let Some(theme) = &document.theme {
        add_part(&mut zip, THEME_PART, parts::theme_part(theme).into_bytes())?;
    }
    if !document.footnotes.is_empty() {
        add_part(
            &mut zip,
            FOOTNOTES_PART,
            parts::notes_part(&mut ctx, &document.footnotes, true).into_bytes(),
        )?;
    }
    if !document.endnotes.is_empty() {
        add_part(
            &mut zip,
            ENDNOTES_PART,
            parts::notes_part(&mut ctx, &document.endnotes, false).into_bytes(),
        )?;
    }
    if options.write_font_table {
        let families = parts::font_families(&document.styles);
        add_part(
            &mut zip,
            FONT_TABLE_PART,
            parts::font_table_part(&mut ctx, &families).into_bytes(),
        )?;
    }
    for part in &header_footer_parts {
        let name = part.rsplit('/').next().unwrap_or(part.as_str()).to_owned();
        if let Some(header_footer) = name_header_footer(document, &name) {
            add_part(
                &mut zip,
                part.as_str(),
                parts::header_footer_part(&mut ctx, header_footer).into_bytes(),
            )?;
        }
    }
    for (part, source_part) in &media_parts {
        add_part(&mut zip, part, source.read_part(source_part)?)?;
    }
    // W7: the copied parts, each next to its own `.rels`, in name order.
    for part in pass.parts() {
        add_part(&mut zip, part.name.as_str(), part.bytes.clone())?;
    }

    add_part(
        &mut zip,
        "/word/_rels/document.xml.rels",
        write_relationships(rels.relationships()).into_bytes(),
    )?;
    add_part(
        &mut zip,
        "/_rels/.rels",
        write_relationships(
            &std::iter::once(Relationship {
                id: "rId1".to_owned(),
                rel_type: RelType::OfficeDocument,
                raw_type: strict_type_uri(&RelType::OfficeDocument),
                target: "word/document.xml".to_owned(),
                target_mode: TargetMode::Internal,
                resolved: None,
            })
            .chain(pass.root_relationships().iter().map(|rel| Relationship {
                id: rel.id.clone(),
                rel_type: RelType::Other(rel.raw_type.clone()),
                raw_type: rel.raw_type.clone(),
                target: rel.target.clone(),
                target_mode: TargetMode::Internal,
                resolved: None,
            }))
            .collect::<Vec<_>>(),
        )
        .into_bytes(),
    )?;
    add_part(
        &mut zip,
        "/[Content_Types].xml",
        content_types.write_xml().into_bytes(),
    )?;

    let part_count = zip.len();
    let bytes = zip.finish()?;
    Ok(WriteOutput {
        bytes,
        report: ctx.into_report(),
        part_count,
    })
}

/// Finds the header/footer a written part name refers to.
fn name_header_footer<'a>(
    document: &'a Document,
    name: &str,
) -> Option<&'a strict_ooxml_wml::model::document::HeaderFooter> {
    let role = name
        .trim_start_matches("header")
        .trim_start_matches("footer");
    let index: usize = role.trim_end_matches(".xml").parse().ok()?;
    let want_header = name.starts_with("header");
    document
        .headers_footers
        .iter()
        .filter(|candidate| candidate.is_header == want_header)
        .nth(index.saturating_sub(1))
}

fn add_part(zip: &mut ZipWriter, part: &str, bytes: Vec<u8>) -> Result<()> {
    zip.add_part(&PartId::new(part), bytes)
}

/// Collects the relationship ids of every hyperlink in the body, in order.
fn collect_hyperlink_ids(blocks: &[Block]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_hyperlink_ids_in(blocks, &mut out);
    out
}

fn collect_hyperlink_ids_in(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => collect_hyperlink_ids_inline(&paragraph.inlines, out),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_hyperlink_ids_in(&cell.blocks, out);
                    }
                }
            }
            Block::SdtBlock(sdt) => collect_hyperlink_ids_in(&sdt.blocks, out),
            _ => {}
        }
    }
}

fn collect_hyperlink_ids_inline(inlines: &[Inline], out: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Hyperlink(link) => {
                if let Some(id) = &link.rel_id {
                    let id = id.as_str().to_owned();
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
                collect_hyperlink_ids_inline(&link.inlines, out);
            }
            Inline::Field(field) => collect_hyperlink_ids_inline(&field.inlines, out),
            Inline::SdtInline(sdt) => collect_hyperlink_ids_inline(&sdt.inlines, out),
            _ => {}
        }
    }
}

/// Serializes `word/document.xml`.
fn document_part(ctx: &mut Ctx<'_>, document: &Document) -> String {
    let mut xml = XmlWriter::new();
    let declared = namespaces();
    xml.start_root(
        "w:document",
        &declared
            .iter()
            .map(|(prefix, uri)| (*prefix, *uri))
            .collect::<Vec<_>>(),
    );
    xml.start("w:body");
    blocks(ctx, &mut xml, &document.body.blocks);
    if let Some(section) = document.sections.last() {
        section_properties(ctx, &mut xml, &section.properties);
    }
    xml.end(); // w:body
    xml.end(); // w:document
    xml.finish().expect("balanced")
}

/// The namespace declarations every WML part carries.
///
/// Order is fixed and duplicates are dropped keeping the first, because a
/// repeated `xmlns:a` is a hard XML error rather than a redundant declaration.
///
/// `mc`, `w14` and `w15` used to be declared here for the extension attributes
/// the writer wrote, and they are gone with them (ADR-0014): a Strict part that
/// declares the extension namespace is one invitation away from using it again,
/// and nothing in `purl.oclc.org` needs any of the three.
fn namespaces() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for (prefix, uri) in [
        ("w", NS_W),
        ("r", NS_R),
        ("wp", NS_WP),
        ("a", NS_A),
        ("pic", NS_PIC),
        ("m", NS_M),
    ]
    .into_iter()
    .chain(drawing_namespaces())
    {
        if !out.iter().any(|(existing, _)| *existing == prefix) {
            out.push((prefix, uri));
        }
    }
    out
}

/// The file extension for a media kind.
#[must_use]
pub fn media_extension(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Png => "png",
        MediaKind::Jpeg => "jpeg",
        MediaKind::Gif => "gif",
        MediaKind::Bmp => "bmp",
        MediaKind::Tiff => "tiff",
        MediaKind::Emf => "emf",
        MediaKind::Wmf => "wmf",
        MediaKind::Svg => "svg",
        MediaKind::Other => "bin",
    }
}

/// The content type for a media kind.
#[must_use]
pub fn media_content_type(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Png => "image/png",
        MediaKind::Jpeg => "image/jpeg",
        MediaKind::Gif => "image/gif",
        MediaKind::Bmp => "image/bmp",
        MediaKind::Tiff => "image/tiff",
        MediaKind::Emf => "image/x-emf",
        MediaKind::Wmf => "image/x-wmf",
        MediaKind::Svg => "image/svg+xml",
        MediaKind::Other => "application/octet-stream",
    }
}
