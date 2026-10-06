//! Per-part writing context: the loss report, the relationship-id maps and the
//! counters a part needs.

use std::collections::{BTreeMap, BTreeSet};

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::normalize::report::{LossRecord, NormalizationReport, Severity};
use strict_ooxml_core::opc::rels::RelType;
use strict_ooxml_core::part::PartId;

use crate::package::RelAllocator;
use crate::passthrough::PassThrough;

/// Stable id of the writer's own loss class.
///
/// It is namespaced with `W` so it can never collide with a normalization
/// stage id (`T1`…`T8`) in a report that merges both.
pub const WRITE_LOSS_ID: &str = "W1.unserializable";

/// Stable id of the writer's "written only in part" class.
pub const WRITE_PARTIAL_ID: &str = "W2.partial";

/// Which notes part is being written.
///
/// [`RunContent::NoteRef`](strict_ooxml_wml::model::inline::RunContent::NoteRef)
/// covers both `w:footnoteRef` and `w:endnoteRef` — the two differ only in the
/// part they live in, and the model deliberately does not record which. The
/// writer does know, because it is writing one part at a time, so it says so
/// here rather than guessing from the element name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteRole {
    /// `/word/footnotes.xml` — the element is `w:footnoteRef`.
    Footnote,
    /// `/word/endnotes.xml` — the element is `w:endnoteRef`.
    Endnote,
}

impl NoteRole {
    /// The reference element this role writes.
    #[must_use]
    pub fn reference_element(self) -> &'static str {
        match self {
            Self::Footnote => "w:footnoteRef",
            Self::Endnote => "w:endnoteRef",
        }
    }
}

/// State shared by every part writer.
#[derive(Debug)]
pub struct Ctx<'a> {
    report: &'a mut NormalizationReport,
    next_doc_pr_id: u32,
    /// `wp:docPr/@id` values already written into the current part.
    used_doc_pr_ids: BTreeSet<u32>,
    /// Old hyperlink relationship id → the id this write emits (document part).
    hyperlinks: BTreeMap<String, String>,
    /// Media source part → document-part relationship id.
    media: BTreeMap<String, String>,
    /// Media source part → written relative target (`media/imageN.ext`).
    media_targets: BTreeMap<String, String>,
    /// Header/footer part id → the relationship id this write emits.
    header_footers: BTreeMap<String, String>,
    /// The pass-through of unmodelled parts, when the package has one.
    passthrough: Option<PassThrough>,
    /// Which notes part is being written, when writing one.
    note_role: Option<NoteRole>,
    /// The part currently being written (AUD-61).
    current_part: Option<PartId>,
    /// Fresh relationship allocator for the current non-document content part.
    part_rels: Option<RelAllocator>,
    /// Foreign (chart/diagram) old id → new id for the current part.
    part_foreign: BTreeMap<String, String>,
    /// The font table's own relationships: media part id → the id
    /// `word/_rels/fontTable.xml.rels` will carry.
    fonts: BTreeMap<String, String>,
    /// Characters removed from the part being written because XML 1.0 cannot
    /// carry them; drained into the report by [`Self::report_invalid_chars`].
    invalid_chars: usize,
}

/// Stable id of the loss "a character XML cannot carry was removed".
pub const INVALID_XML_CHAR_ID: &str = "W.invalid-xml-char";

/// Stable id: a `w:customXml` / `w:smartTag` wrapper was dropped on read (AUD-42).
pub const CUSTOM_XML_WRAPPER_ID: &str = "W.custom-xml-wrapper";

/// Stable id: an embedded font's bytes could not be read (AUD-61).
pub const FONT_LOSS_ID: &str = "W7.font";

/// Stable id: a generated relationship kept a Transitional type URI (AUD-63).
pub const RELTYPE_TRANSITIONAL_ID: &str = "W.reltype-transitional";

/// Stable id: a passthrough part still carries a Transitional signal (AUD-63).
pub const NON_STRICT_PART_ID: &str = "W7.non-strict-part";

impl<'a> Ctx<'a> {
    /// Creates a context writing into `report`.
    #[must_use]
    pub fn new(report: &'a mut NormalizationReport) -> Self {
        Self {
            report,
            // `wp:docPr/@id` must be unique within a part and non-zero; 1 is the
            // first value Word uses.
            next_doc_pr_id: 1,
            used_doc_pr_ids: BTreeSet::new(),
            hyperlinks: BTreeMap::new(),
            media: BTreeMap::new(),
            media_targets: BTreeMap::new(),
            header_footers: BTreeMap::new(),
            passthrough: None,
            note_role: None,
            current_part: None,
            part_rels: None,
            part_foreign: BTreeMap::new(),
            fonts: BTreeMap::new(),
            invalid_chars: 0,
        }
    }

    /// Finishes a part's XML, keeping count of the characters it had to drop.
    ///
    /// # Errors
    ///
    /// As [`XmlWriter::finish`](crate::xml::XmlWriter::finish).
    pub fn finish_xml(
        &mut self,
        xml: crate::xml::XmlWriter,
    ) -> Result<String, crate::xml::WriteError> {
        let (text, removed) = xml.finish_counted()?;
        self.invalid_chars += removed;
        Ok(text)
    }

    /// Records one loss for `part` if any character was dropped since the last
    /// call, and resets the count.
    pub fn report_invalid_chars(&mut self, part: &str) {
        let removed = std::mem::take(&mut self.invalid_chars);
        if removed == 0 {
            return;
        }
        let mut location = SourceLocation::unknown();
        location.part = PartId::new(part);
        self.report.record_loss(LossRecord {
            transform_id: WRITE_LOSS_ID,
            feature_id: INVALID_XML_CHAR_ID.to_owned(),
            reason: format!(
                "{removed} character(s) that XML 1.0 cannot carry (C0 controls other than \
                 tab, line feed and carriage return, U+FFFE, U+FFFF) were removed from text \
                 and attribute values"
            ),
            severity: Severity::Lossy,
            locations: vec![location],
        });
    }

    /// Declares which notes part is being written.
    pub fn set_note_role(&mut self, role: NoteRole) {
        self.note_role = Some(role);
    }

    /// Returns the notes role, when writing a notes part.
    #[must_use]
    pub fn note_role(&self) -> Option<NoteRole> {
        self.note_role
    }

    /// Records the relationship-id maps a package write computed.
    #[must_use]
    pub fn with_relationships(
        mut self,
        hyperlinks: BTreeMap<String, String>,
        media: BTreeMap<String, String>,
        media_targets: BTreeMap<String, String>,
        header_footers: BTreeMap<String, String>,
    ) -> Self {
        self.hyperlinks = hyperlinks;
        self.media = media;
        self.media_targets = media_targets;
        self.header_footers = header_footers;
        self
    }

    /// Attaches the pass-through of unmodelled parts (W7).
    pub(crate) fn with_passthrough(mut self, passthrough: &PassThrough) -> Self {
        self.passthrough = Some(passthrough.clone());
        self
    }

    /// Returns the relationship id to write for a reference into an unmodelled
    /// part, when the pass-through resolved it.
    ///
    /// Inside a decoration or notes part the map is that part's own (AUD-61);
    /// otherwise it is the document part's.
    #[must_use]
    pub fn foreign_rel(&self, old_id: &str) -> Option<&str> {
        if self.part_rels.is_some() {
            return self.part_foreign.get(old_id).map(String::as_str);
        }
        self.passthrough
            .as_ref()
            .and_then(|pass| pass.document_rel(old_id))
    }

    /// Begins writing a content part that owns its own `.rels` (AUD-61).
    pub fn begin_part_relationships(&mut self, part: &str) {
        self.current_part = Some(PartId::new(part));
        self.part_rels = Some(RelAllocator::new());
        self.part_foreign.clear();
    }

    /// Records a foreign (chart/diagram) id for the current part.
    pub fn set_part_foreign(&mut self, old_id: String, new_id: String) {
        self.part_foreign.insert(old_id, new_id);
    }

    /// Mutable access to the current part's relationship allocator.
    pub(crate) fn part_rels_mut(&mut self) -> Option<&mut RelAllocator> {
        self.part_rels.as_mut()
    }

    /// Ends the part-local relationship scope and returns what was allocated.
    pub(crate) fn take_part_relationships(&mut self) -> RelAllocator {
        self.current_part = None;
        self.part_foreign.clear();
        self.part_rels.take().unwrap_or_default()
    }

    /// Returns the relationship id to write for a hyperlink that referenced
    /// `old_id` in the parsed document.
    ///
    /// Inside a decoration/notes part the document's ids are not the answer
    /// (AUD-61): they name the document part's relationships.
    #[must_use]
    pub fn hyperlink_rel(&self, old_id: &str) -> Option<&str> {
        if self.part_rels.is_some() {
            return None;
        }
        self.hyperlinks.get(old_id).map(String::as_str)
    }

    /// Returns the relationship id to write for a picture that referenced
    /// `part`.
    ///
    /// Inside a decoration or notes part a fresh id is allocated in that
    /// part's [`RelAllocator`] against the **written** media target (AUD-61).
    pub fn media_rel(&mut self, part: Option<&PartId>) -> Option<String> {
        let part = part?;
        let target = self.media_targets.get(part.as_str())?.clone();
        if let Some(alloc) = self.part_rels.as_mut() {
            return Some(alloc.ensure(&RelType::Image, target, false));
        }
        self.media.get(part.as_str()).cloned()
    }

    /// Returns the relationship id to write for a header/footer part.
    #[must_use]
    pub fn header_footer_rel(&self, part: &PartId) -> Option<&str> {
        self.header_footers.get(part.as_str()).map(String::as_str)
    }

    /// Declares the ids `word/_rels/fontTable.xml.rels` will carry.
    #[must_use]
    pub fn with_font_relationships(mut self, fonts: BTreeMap<String, String>) -> Self {
        self.fonts = fonts;
        self
    }

    /// Replaces the font-table relationship map (AUD-61: after bytes are read).
    pub fn set_font_relationships(&mut self, fonts: BTreeMap<String, String>) {
        self.fonts = fonts;
    }

    /// Returns the relationship id to write into `w:embed*`.
    #[must_use]
    pub fn font_rel(&self, part: &PartId) -> Option<&str> {
        self.fonts.get(part.as_str()).map(String::as_str)
    }

    /// Returns the mutable report.
    pub fn report(&mut self) -> &mut NormalizationReport {
        self.report
    }

    /// Allocates the next free `wp:docPr/@id`.
    pub fn next_doc_pr_id(&mut self) -> u32 {
        loop {
            let id = self.next_doc_pr_id;
            self.next_doc_pr_id = self.next_doc_pr_id.saturating_add(1);
            if self.used_doc_pr_ids.insert(id) {
                return id;
            }
        }
    }

    /// Keeps a parsed `wp:docPr/@id` when it is still free.
    ///
    /// The id is only a uniqueness key. Reusing the source value keeps the
    /// drawing identity; a collision takes the next free id instead.
    pub fn reserve_doc_pr_id(&mut self, id: u32) -> u32 {
        if id == 0 || !self.used_doc_pr_ids.insert(id) {
            return self.next_doc_pr_id();
        }
        if self.next_doc_pr_id <= id {
            self.next_doc_pr_id = id.saturating_add(1);
        }
        id
    }

    /// Records a construct that could not be written at all.
    pub fn report_unsupported(
        &mut self,
        feature_id: &str,
        reason: &str,
        location: &SourceLocation,
    ) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_LOSS_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
        self.report.count_reported_removal(1);
    }

    /// Records a Lossy write fact that is not a removed node (AUD-63).
    pub fn report_lossy(&mut self, feature_id: &str, reason: &str, location: &SourceLocation) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_LOSS_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
    }

    /// Records a construct that was written only in part.
    pub fn report_partial(&mut self, feature_id: &str, reason: &str, location: &SourceLocation) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_PARTIAL_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Ignorable,
            locations: vec![location.clone()],
        });
    }

    /// Records an informational rewrite that changes no meaning.
    pub fn report_info(&mut self, feature_id: &str, reason: &str, location: &SourceLocation) {
        self.report.record_loss(LossRecord {
            transform_id: WRITE_PARTIAL_ID,
            feature_id: feature_id.to_owned(),
            reason: reason.to_owned(),
            severity: Severity::Info,
            locations: vec![location.clone()],
        });
    }

    /// Consumes the context, returning the report it wrote into.
    #[must_use]
    pub fn into_report(self) -> NormalizationReport {
        self.report.clone()
    }
}
