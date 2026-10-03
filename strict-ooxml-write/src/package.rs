//! Package assembly: parts, relationships, content types and the ZIP.
//!
//! Everything that decides *order* lives here, because order is what makes the
//! output reproducible: relationship ids are handed out in a fixed sequence,
//! media parts are named by index, and the ZIP keeps insertion order with a
//! fixed timestamp (SC-1). Nothing depends on a hash map's iteration order.

use std::collections::{BTreeMap, BTreeSet};

use strict_ooxml_core::error::{Result, StrictError};
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::content_types::ContentTypeIndex;
use strict_ooxml_core::opc::rels::{
    strict_type_uri, write_relationships, RelType, Relationship, TargetMode,
};
use strict_ooxml_core::opc::zip::write::ZipWriter;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::MediaKind;
use strict_ooxml_wml::model::fonts::FontTable;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::parse::LOST_FONT_PART;

use crate::body::blocks;
use crate::ctx::Ctx;
use crate::drawing::namespaces as drawing_namespaces;
use crate::parts;
use crate::passthrough;
use crate::props::section_properties;
use crate::xml::{WriteError, XmlWriter, NS_A, NS_M, NS_PIC, NS_R, NS_W, NS_WP};

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

/// Allocates part names so generated media never collides with a passthrough
/// part (AUD-62).
///
/// Passthrough keeps source spellings; generated images take the lowest free
/// `word/media/image{N}.{ext}`. Re-reserving the same name is a no-op so a
/// thumbnail referenced twice still becomes one part.
#[derive(Debug, Default)]
pub(crate) struct PartNameAllocator {
    reserved: BTreeSet<PartId>,
}

impl PartNameAllocator {
    /// Empty allocator.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserves an absolute part name. A second reservation of the same name
    /// (ASCII case-insensitive, AUD-24) is a no-op.
    pub(crate) fn reserve(&mut self, name: &str) {
        self.reserved.insert(PartId::new(name));
    }

    /// Whether `name` is already reserved.
    #[must_use]
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.reserved.contains(&PartId::new(name))
    }

    /// Next free `/word/media/image{N}.{ext}`; returns `(absolute, relative)`.
    pub(crate) fn allocate_media(&mut self, extension: &str) -> (String, String) {
        let mut n = 1u32;
        loop {
            let relative = format!("media/image{n}.{extension}");
            let absolute = format!("/word/{relative}");
            let id = PartId::new(absolute.as_str());
            if !self.reserved.contains(&id) {
                self.reserved.insert(id);
                return (absolute, relative);
            }
            n = n.saturating_add(1);
        }
    }
}

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

    /// Every part the source package holds.
    ///
    /// Needed for one question only, and it is the question SC-10 exists for:
    /// **which parts did this write drop without saying so?** A part that is
    /// absent validates perfectly and draws nothing, so nothing else in the
    /// system can notice it — the pass-through reaches what a reference reaches,
    /// and an orphan (no `.rels` points at it) is by definition unreached. A
    /// source that cannot enumerate its parts reports none, and the accounting
    /// is then simply silent, which is the pre-existing behaviour.
    fn parts(&self) -> Vec<PartId> {
        Vec::new()
    }

    /// Parts reachable from `from` by following internal relationships
    /// (AUD-25), not including `from` itself.
    ///
    /// The default walks [`Source::relationships`] without a depth bound (for
    /// test doubles that are not a real package). The
    /// [`Package`](strict_ooxml_core::opc::Package) impl enforces
    /// `max_rel_depth`.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`](strict_ooxml_core::error::StrictError) when a
    /// bound is exceeded.
    fn reachable_parts(&self, from: &PartId) -> Result<Vec<PartId>> {
        use std::collections::{HashSet, VecDeque};
        let mut visited = HashSet::new();
        let mut out = Vec::new();
        let mut queue = VecDeque::new();
        visited.insert(from.clone());
        queue.push_back(from.clone());
        while let Some(part) = queue.pop_front() {
            for info in self.relationships(&part) {
                if info.external {
                    continue;
                }
                let Ok(Some(target)) =
                    strict_ooxml_core::opc::path::resolve_target(&part, &info.target, false)
                else {
                    continue;
                };
                if !visited.insert(target.clone()) {
                    continue;
                }
                out.push(target.clone());
                queue.push_back(target);
            }
        }
        Ok(out)
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

    fn parts(&self) -> Vec<PartId> {
        strict_ooxml_core::opc::Package::parts(self)
            .map(|part| part.id.clone())
            .collect()
    }

    fn reachable_parts(&self, from: &PartId) -> Result<Vec<PartId>> {
        strict_ooxml_core::opc::Package::reachable_parts(self, from)
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

/// Per-part relationship allocator (AUD-61).
///
/// Deduplicates by `(external, target)` so two pictures of the same media part
/// share one id inside the part being written.
#[derive(Debug, Default)]
pub(crate) struct RelAllocator {
    builder: RelBuilder,
    by_target: BTreeMap<(bool, String), String>,
}

impl RelAllocator {
    /// Empty allocator starting at `rId1`.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Ensures a relationship for `target` and returns its id.
    pub(crate) fn ensure(&mut self, rel_type: &RelType, target: String, external: bool) -> String {
        let key = (external, target.clone());
        if let Some(id) = self.by_target.get(&key) {
            return id.clone();
        }
        let id = self.builder.add(rel_type, target, external);
        self.by_target.insert(key, id.clone());
        id
    }

    /// Relationships in allocation order.
    pub(crate) fn relationships(&self) -> &[Relationship] {
        self.builder.relationships()
    }

    /// Whether nothing was allocated.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.builder.relationships().is_empty()
    }
}

/// Serialises one part, naming the part if the writer gives up.
///
/// The writer can fail on its own terms - an element left open, or a document
/// nested past the budget - and that is a reportable failure about *this* part
/// rather than about the input. Every part writer returns `Result` for that
/// reason (`STAGE-10-TASK.md` E34); before it, these call sites turned it into a
/// panic via `.expect("balanced")`.
///
/// It is also where a part's dropped characters become a loss: the count is
/// kept by the part's [`XmlWriter`] and drained here, where the part name is
/// known.
fn part_xml<'a>(
    ctx: &mut Ctx<'a>,
    part: &str,
    write: impl FnOnce(&mut Ctx<'a>) -> std::result::Result<String, WriteError>,
) -> Result<String> {
    let xml = write(ctx).map_err(|error| StrictError::Write {
        part: PartId::new(part),
        detail: error.to_string(),
    })?;
    ctx.report_invalid_chars(part);
    Ok(xml)
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
    // Before anything is serialized: a model nested past the block budget would
    // recurse the serializer as deep as the model goes, and the reader that
    // produced it would never have let it exist. A model built in code has had no
    // such reader, so this is the first place that can say no (AUD-05 п.3).
    crate::body::check_block_nesting(&document.body.blocks, &options.limits).map_err(|error| {
        StrictError::Write {
            part: PartId::new(MAIN_DOCUMENT),
            detail: error.to_string(),
        }
    })?;
    let mut report = crate::WriteReport::new();
    // AUD-42: the parser drops customXml/smartTag wrappers (content kept). The
    // writer cannot restore them — surface the Ignorable loss once per feature.
    for feature in ["w:customXml", "w:smartTag"] {
        if let Some(entry) = document.support.get(feature) {
            if entry.status == strict_ooxml_wml::model::support::SupportStatus::Partial {
                use strict_ooxml_core::normalize::report::{LossRecord, Severity};
                report.record_loss(LossRecord {
                    transform_id: crate::ctx::WRITE_PARTIAL_ID,
                    feature_id: crate::ctx::CUSTOM_XML_WRAPPER_ID.to_owned(),
                    reason: format!("{feature}: wrapper dropped, content kept"),
                    severity: Severity::Ignorable,
                    locations: entry.locations.clone(),
                });
            }
        }
    }
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

    // Embedded-font candidates. AUD-61: relationships are allocated only after
    // the bytes are read successfully — see the font-table write below.
    let font_candidates: Vec<PartId> = document
        .font_table
        .as_ref()
        .map(|table| {
            table
                .embedded_parts()
                .iter()
                .filter(|font| font.part.as_str() != LOST_FONT_PART)
                .map(|font| font.part.clone())
                .collect()
        })
        .unwrap_or_default();

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

    // AUD-62: reserve every passthrough name first, then give generated media
    // the lowest free `image{N}`. Media relationships are added after
    // passthrough so a chart thumbnail at `word/media/image1.png` does not
    // collide with the body's first picture.
    let mut names = PartNameAllocator::new();
    for part in pass.parts() {
        names.reserve(part.name.as_str());
    }
    let mut media_map: BTreeMap<String, String> = BTreeMap::new();
    let mut media_targets: BTreeMap<String, String> = BTreeMap::new();
    let mut media_parts: Vec<(String, PartId)> = Vec::new();
    for item in document.media.iter() {
        let source_key = item.part.as_str().to_owned();
        if media_targets.contains_key(&source_key) {
            // Same source part referenced twice (e.g. a shared thumbnail): one
            // written part, one document relationship.
            continue;
        }
        let extension = media_extension(item.kind);
        // If passthrough already reserved the source spelling (chart thumbnail),
        // reuse that part instead of writing a second copy under a new name.
        let reused = names.contains(&source_key);
        let (absolute, relative) = if reused {
            let relative = source_key
                .strip_prefix("/word/")
                .unwrap_or(source_key.as_str())
                .to_owned();
            (source_key.clone(), relative)
        } else {
            names.allocate_media(extension)
        };
        let id = rels.add(&RelType::Image, relative.clone(), false);
        media_map.insert(source_key.clone(), id);
        media_targets.insert(source_key, relative);
        if !reused {
            media_parts.push((absolute, item.part.clone()));
        }
        content_types.insert_default(extension, media_content_type(item.kind));
    }

    ctx = ctx
        .with_relationships(
            hyperlink_map,
            media_map.clone(),
            media_targets.clone(),
            header_footer_map,
        )
        .with_passthrough(&pass);

    // The parts themselves. Each is written only when the model carries the
    // content for it, so a document does not acquire parts it did not have.
    let mut zip = ZipWriter::with_limits(options.limits);
    add_part(
        &mut zip,
        MAIN_DOCUMENT,
        part_xml(&mut ctx, MAIN_DOCUMENT, |ctx| document_part(ctx, document))?.into_bytes(),
    )?;
    if content_types.content_type_for(&PartId::new(STYLES_PART)) == Some(CONTENT_TYPE_STYLES) {
        add_part(
            &mut zip,
            STYLES_PART,
            part_xml(&mut ctx, STYLES_PART, |ctx| {
                parts::styles_part(ctx, &document.styles)
            })?
            .into_bytes(),
        )?;
    }
    if !document.numbering.is_empty() {
        add_part(
            &mut zip,
            NUMBERING_PART,
            part_xml(&mut ctx, NUMBERING_PART, |ctx| {
                parts::numbering_part(ctx, &document.numbering)
            })?
            .into_bytes(),
        )?;
    }
    if document.settings != Default::default() || options.always_write_settings {
        add_part(
            &mut zip,
            SETTINGS_PART,
            part_xml(&mut ctx, SETTINGS_PART, |ctx| {
                parts::settings_part(ctx, &document.settings)
            })?
            .into_bytes(),
        )?;
    }
    if let Some(theme) = &document.theme {
        add_part(
            &mut zip,
            THEME_PART,
            part_xml(&mut ctx, THEME_PART, |ctx| parts::theme_part(ctx, theme))?.into_bytes(),
        )?;
    }
    // Extra parts copied because a header/footer/notes part referenced them
    // (AUD-61); merged with the document passthrough by name when writing.
    let mut part_extra_copies: Vec<passthrough::CopiedPart> = Vec::new();

    if !document.footnotes.is_empty() {
        let foreign = note_foreign_ids(&document.footnotes);
        write_content_part_with_rels(
            &mut zip,
            &mut ctx,
            source,
            FOOTNOTES_PART,
            "footnotes.xml",
            document.source.footnotes.as_ref(),
            &foreign,
            &mut part_extra_copies,
            |ctx| parts::notes_part(ctx, &document.footnotes, true),
        )?;
    }
    if !document.endnotes.is_empty() {
        let foreign = note_foreign_ids(&document.endnotes);
        write_content_part_with_rels(
            &mut zip,
            &mut ctx,
            source,
            ENDNOTES_PART,
            "endnotes.xml",
            document.source.endnotes.as_ref(),
            &foreign,
            &mut part_extra_copies,
            |ctx| parts::notes_part(ctx, &document.endnotes, false),
        )?;
    }
    if options.write_font_table {
        // AUD-61: read every font first; only successful reads enter
        // `fontTable.xml.rels` (loss `W7.font` otherwise).
        let mut font_rels = RelBuilder::new();
        let mut font_map: BTreeMap<String, String> = BTreeMap::new();
        let mut font_parts: Vec<(String, PartId, Vec<u8>)> = Vec::new();
        for (index, source_part) in font_candidates.iter().enumerate() {
            match source.read_part(source_part) {
                Ok(bytes) => {
                    let extension = font_extension(source_part.as_str());
                    let name = format!("fonts/font{index}.{extension}");
                    let id = font_rels.add(&RelType::Font, name.clone(), false);
                    font_map.insert(source_part.as_str().to_owned(), id);
                    content_types.insert_default(extension, font_content_type(extension));
                    font_parts.push((format!("/word/{name}"), source_part.clone(), bytes));
                }
                Err(error) => ctx.report_unsupported(
                    crate::ctx::FONT_LOSS_ID,
                    &format!(
                        "{source_part} is an embedded font whose bytes could not be read ({error}), \
                         so the face is lost and the relationship is dropped rather than left \
                         pointing at nothing"
                    ),
                    &strict_ooxml_core::error::SourceLocation::unknown(),
                ),
            }
        }
        ctx.set_font_relationships(font_map);
        let families = parts::font_families(&document.styles);
        add_part(
            &mut zip,
            FONT_TABLE_PART,
            part_xml(&mut ctx, FONT_TABLE_PART, |ctx| {
                parts::font_table_part(
                    ctx,
                    document
                        .font_table
                        .as_ref()
                        .unwrap_or(&FontTable::default()),
                    &families,
                )
            })?
            .into_bytes(),
        )?;
        if !font_rels.relationships().is_empty() {
            add_part(
                &mut zip,
                "/word/_rels/fontTable.xml.rels",
                write_relationships(font_rels.relationships()).into_bytes(),
            )?;
        }
        for (part, _, bytes) in font_parts {
            add_part(&mut zip, &part, bytes)?;
        }
    }
    for part in &header_footer_parts {
        let name = part.rsplit('/').next().unwrap_or(part.as_str()).to_owned();
        if let Some(header_footer) = name_header_footer(document, &name) {
            let foreign = passthrough::referenced_ids(header_footer.blocks.as_slice());
            write_content_part_with_rels(
                &mut zip,
                &mut ctx,
                source,
                part.as_str(),
                &name,
                Some(&header_footer.part),
                &foreign,
                &mut part_extra_copies,
                |ctx| parts::header_footer_part(ctx, header_footer),
            )?;
        }
    }
    for (part, source_part) in &media_parts {
        add_part(&mut zip, part, source.read_part(source_part)?)?;
    }
    // W7: the copied parts, each next to its own `.rels`, in name order.
    for part in &part_extra_copies {
        if let Some(content_type) = &part.content_type {
            content_types.insert_override(PartId::new(part.name.as_str()), content_type);
        }
    }
    let mut written_pass: BTreeSet<String> = BTreeSet::new();
    for part in pass.parts().iter().chain(part_extra_copies.iter()) {
        if !written_pass.insert(part.name.clone()) {
            continue;
        }
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
    // W7, and the last thing the write does: name every part the source had that
    // this package does not. It runs HERE rather than in the pass-through plan
    // because "which parts did the write produce" is a question about the result
    // and several of them are conditional — `word/numbering.xml` is written only
    // when the model carries a numbering table, and a plan that assumed it was
    // produced would call that document's numbering a non-loss.
    passthrough::report_what_was_dropped(&mut ctx, source, &zip.part_names());
    let bytes = zip.finish()?;
    // AUD-61: every r:* in every written part must resolve through that part's
    // .rels to an existing part or External. A target named in a W7.* loss was
    // intentionally not copied (unreadable bytes) — that is a reported loss,
    // not a silent dangling id.
    let known_gaps: Vec<String> = ctx
        .report()
        .losses()
        .iter()
        .filter(|loss| {
            loss.feature_id.starts_with("W7.") || loss.feature_id == crate::ctx::FONT_LOSS_ID
        })
        .map(|loss| loss.reason.clone())
        .collect();
    verify_no_dangling_relationships(&bytes, &known_gaps).map_err(|error| StrictError::Write {
        part: PartId::new(MAIN_DOCUMENT),
        detail: error.to_string(),
    })?;
    Ok(WriteOutput {
        bytes,
        report: ctx.into_report(),
        part_count,
    })
}

/// Writes a content part that owns its own `.rels` (AUD-61).
#[allow(clippy::too_many_arguments)]
fn write_content_part_with_rels(
    zip: &mut ZipWriter,
    ctx: &mut Ctx<'_>,
    source: &dyn Source,
    part: &str,
    file_name: &str,
    source_part: Option<&PartId>,
    foreign_ids: &[String],
    extra: &mut Vec<passthrough::CopiedPart>,
    write: impl FnOnce(&mut Ctx<'_>) -> std::result::Result<String, WriteError>,
) -> Result<()> {
    ctx.begin_part_relationships(part);
    if let Some(from) = source_part {
        bind_part_foreign(ctx, source, from, foreign_ids, extra);
    }
    let xml = part_xml(ctx, part, write)?.into_bytes();
    let alloc = ctx.take_part_relationships();
    add_part(zip, part, xml)?;
    if !alloc.is_empty() {
        add_part(
            zip,
            &format!("/word/_rels/{file_name}.rels"),
            write_relationships(alloc.relationships()).into_bytes(),
        )?;
    }
    Ok(())
}

/// Resolves chart/diagram ids of a non-document part into that part's allocator.
fn bind_part_foreign(
    ctx: &mut Ctx<'_>,
    source: &dyn Source,
    from: &PartId,
    foreign_ids: &[String],
    extra: &mut Vec<passthrough::CopiedPart>,
) {
    for old_id in foreign_ids {
        let Some(info) = source.relationship(from, old_id) else {
            continue;
        };
        let Some(alloc) = ctx.part_rels_mut() else {
            continue;
        };
        let new_id = alloc.ensure(&info.rel_type, info.target.clone(), info.external);
        ctx.set_part_foreign(old_id.clone(), new_id);
        if info.external {
            continue;
        }
        let Ok(Some(target)) =
            strict_ooxml_core::opc::path::resolve_target(from, &info.target, false)
        else {
            continue;
        };
        let mut seeds = vec![target];
        if let Ok(reached) = source.reachable_parts(&seeds[0]) {
            seeds.extend(reached);
        }
        for seed in seeds {
            if extra.iter().any(|part| part.name == seed.as_str()) {
                continue;
            }
            let Ok(bytes) = source.read_part(&seed) else {
                continue;
            };
            // Also copy the part's own .rels so its internal ids keep working.
            extra.push(passthrough::CopiedPart {
                name: seed.as_str().to_owned(),
                bytes,
                content_type: source.content_type(&seed),
            });
            if let Some(rels_part) = passthrough::rels_part_of(&seed) {
                if let Ok(rels_bytes) = source.read_part(&rels_part) {
                    let rels_name = rels_part.as_str().to_owned();
                    if !extra.iter().any(|part| part.name == rels_name) {
                        extra.push(passthrough::CopiedPart {
                            name: rels_name,
                            bytes: rels_bytes,
                            content_type: Some(
                                "application/vnd.openxmlformats-package.relationships+xml"
                                    .to_owned(),
                            ),
                        });
                    }
                }
            }
        }
    }
}

fn note_foreign_ids(table: &strict_ooxml_wml::model::notes::NoteTable) -> Vec<String> {
    let mut out = Vec::new();
    for note in table.iter() {
        for id in passthrough::referenced_ids(&note.blocks) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// AUD-61 invariant: every `r:*` attribute resolves through its part's `.rels`.
fn verify_no_dangling_relationships(
    bytes: &[u8],
    known_gaps: &[String],
) -> std::result::Result<(), WriteError> {
    let package = strict_ooxml_core::opc::Package::open_reader(
        bytes,
        &strict_ooxml_core::opc::OpenOptions::default(),
    )
    .map_err(|error| WriteError::DanglingRelationship {
        part: MAIN_DOCUMENT.to_owned(),
        id: format!("package open failed: {error}"),
    })?;
    for part in package.parts() {
        let name = part.id.as_str();
        if !std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
            || name.contains("/_rels/")
        {
            continue;
        }
        let Ok(xml_bytes) = package.read_part(&part.id) else {
            continue;
        };
        let Ok(xml) = std::str::from_utf8(&xml_bytes) else {
            continue;
        };
        let rels = package.relationships(&part.id);
        let declared: BTreeSet<&str> = rels.iter().map(|rel| rel.id.as_str()).collect();
        for id in office_relationship_ids(xml) {
            if !declared.contains(id.as_str()) {
                return Err(WriteError::DanglingRelationship {
                    part: name.to_owned(),
                    id,
                });
            }
            let Some(rel) = rels.iter().find(|rel| rel.id == id) else {
                return Err(WriteError::DanglingRelationship {
                    part: name.to_owned(),
                    id,
                });
            };
            if rel.target_mode == TargetMode::External {
                continue;
            }
            let Ok(Some(resolved)) =
                strict_ooxml_core::opc::path::resolve_target(&part.id, &rel.target, false)
            else {
                return Err(WriteError::DanglingRelationship {
                    part: name.to_owned(),
                    id,
                });
            };
            if package.read_part(&resolved).is_err() {
                let path = resolved.as_str();
                let reported = known_gaps.iter().any(|reason| reason.contains(path));
                if !reported {
                    return Err(WriteError::DanglingRelationship {
                        part: name.to_owned(),
                        id,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Collects relationship-bearing `r:*` attribute values from a Strict part.
fn office_relationship_ids(xml: &str) -> Vec<String> {
    const NAMES: &[&str] = &["embed", "id", "link", "dm", "lo", "qs", "cs"];
    let mut out = Vec::new();
    for attr in NAMES {
        let needle = format!(" r:{attr}=\"");
        let mut rest = xml;
        while let Some(at) = rest.find(&needle) {
            let value_start = &rest[at + needle.len()..];
            let Some(end) = value_start.find('"') else {
                break;
            };
            let value = value_start[..end].to_owned();
            if !value.is_empty() && !out.contains(&value) {
                out.push(value);
            }
            rest = &value_start[end + 1..];
        }
    }
    out
}

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
            Inline::Directional(dir) => collect_hyperlink_ids_inline(&dir.inlines, out),
            _ => {}
        }
    }
}

/// Serializes `word/document.xml`.
fn document_part(
    ctx: &mut Ctx<'_>,
    document: &Document,
) -> std::result::Result<String, WriteError> {
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
    ctx.finish_xml(xml)
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

/// The file extension of an embedded font part, from its own name.
///
/// `ttf` and `odttf` are the two a producer writes, and they mean different
/// things: an `odttf` is an **obfuscated** font, and the `w:fontKey` on the
/// `w:embed*` that points at it is what a consumer de-obfuscates with. Copying
/// the bytes under the wrong extension would hand a consumer a font it
/// de-obfuscates when it should not, and the content type is the other half of
/// the same statement.
#[must_use]
pub fn font_extension(name: &str) -> &'static str {
    let extension = name.rsplit('.').next().unwrap_or_default();
    if extension.eq_ignore_ascii_case("odttf") {
        "odttf"
    } else if extension.eq_ignore_ascii_case("otf") {
        "otf"
    } else {
        "ttf"
    }
}

/// The content type an embedded font of this extension is declared with.
///
/// `otf` gets the obfuscated-font type on purpose rather than by accident of a
/// copied arm: an `.otf` in `word/fonts` is what Word writes when it obfuscates,
/// and OPC has no separate declared type for a plain OpenType font in this
/// position — `application/x-font-ttf` is the type every producer uses for
/// everything that is not `odttf`, and a mismatch here is a part a consumer
/// refuses to load.
#[must_use]
pub fn font_content_type(extension: &str) -> &'static str {
    if extension.eq_ignore_ascii_case("odttf") || extension.eq_ignore_ascii_case("otf") {
        "application/vnd.openxmlformats-officedocument.obfuscatedFont"
    } else {
        "application/x-font-ttf"
    }
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

#[cfg(test)]
mod tests {
    use super::PartNameAllocator;

    #[test]
    fn allocator_skips_reserved_media_names() {
        let mut names = PartNameAllocator::new();
        names.reserve("/word/media/image1.png");
        names.reserve("/word/media/IMAGE1.PNG"); // no-op (AUD-24)
        assert!(names.contains("/word/media/image1.png"));
        let (absolute, relative) = names.allocate_media("png");
        assert_eq!(relative, "media/image2.png");
        assert_eq!(absolute, "/word/media/image2.png");
        let (_, relative) = names.allocate_media("jpeg");
        assert_eq!(relative, "media/image1.jpeg");
    }

    #[test]
    fn allocator_re_reserve_is_noop() {
        let mut names = PartNameAllocator::new();
        names.reserve("/word/media/image1.png");
        names.reserve("/word/media/image1.png");
        assert_eq!(names.reserved.len(), 1);
    }
}
