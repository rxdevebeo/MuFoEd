//! Per-part writing context: the loss report, the relationship-id maps and the
//! counters a part needs.

use std::collections::BTreeMap;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::normalize::report::{LossRecord, NormalizationReport, Severity};
use strict_ooxml_core::part::PartId;

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
    /// Old hyperlink relationship id → the id this write emits.
    hyperlinks: BTreeMap<String, String>,
    /// Media part id → the relationship id this write emits.
    media: BTreeMap<String, String>,
    /// Header/footer part id → the relationship id this write emits.
    header_footers: BTreeMap<String, String>,
    /// The pass-through of unmodelled parts, when the package has one.
    passthrough: Option<PassThrough>,
    /// Which notes part is being written, when writing one.
    note_role: Option<NoteRole>,
    /// The decoration part being written, when writing one.
    ///
    /// Relationship ids are **per part**: `rId3` in `word/header1.xml` is a
    /// different relationship from `rId3` in `word/document.xml`, and a reference
    /// only resolves against its own part's `.rels`. A header that holds a picture
    /// therefore needs its own relationship part, and the ids inside it are this
    /// write's — the same reason the header's rels cannot simply be copied from
    /// the source's.
    decoration: Option<(String, BTreeMap<String, String>)>,
}

impl<'a> Ctx<'a> {
    /// Creates a context writing into `report`.
    #[must_use]
    pub fn new(report: &'a mut NormalizationReport) -> Self {
        Self {
            report,
            // `wp:docPr/@id` must be unique within a part and non-zero; 1 is the
            // first value Word uses.
            next_doc_pr_id: 1,
            hyperlinks: BTreeMap::new(),
            media: BTreeMap::new(),
            header_footers: BTreeMap::new(),
            passthrough: None,
            note_role: None,
            decoration: None,
        }
    }

    /// Declares which notes part is being written.
    ///
    /// The role is what tells [`NoteRole::reference_element`] which of the two
    /// reference elements to emit; outside a notes part it is unset and a
    /// `NoteRef` cannot be written at all, which is reported rather than
    /// guessed.
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
        header_footers: BTreeMap<String, String>,
    ) -> Self {
        self.hyperlinks = hyperlinks;
        self.media = media;
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
    /// `None` is the answer that matters: the part is not in the written package,
    /// so the reference cannot be written and the caller records the loss.
    #[must_use]
    pub fn foreign_rel(&self, old_id: &str) -> Option<&str> {
        self.passthrough
            .as_ref()
            .and_then(|pass| pass.document_rel(old_id))
    }

    /// Declares the relationship ids the decoration part being written will carry.
    ///
    /// `media` maps a media **part** to the id this part's own `.rels` will use.
    /// Inside a decoration the reference the model holds is a resolved part rather
    /// than a relationship id, because the ids belong to the source part and are
    /// not ours to reuse — so the map is keyed by part, exactly as
    /// [`Ctx::media_rel`] expects when a decoration is current.
    pub fn set_decoration_relationships(&mut self, part: &str, media: BTreeMap<String, String>) {
        self.decoration = Some((part.to_owned(), media));
    }

    /// Ends the decoration scope, so the next part written is the document part
    /// again.
    pub fn clear_decoration_relationships(&mut self) {
        self.decoration = None;
    }

    /// Returns the relationship id to write for a hyperlink that referenced
    /// `old_id` in the parsed document.
    ///
    /// **Inside a decoration part the document's ids are not the answer**: they
    /// name the document part's relationships, and the reference resolves against
    /// the header's own `.rels`. A header with a hyperlink therefore gets `None`
    /// here, which the writer records rather than writing a dangling id.
    #[must_use]
    pub fn hyperlink_rel(&self, old_id: &str) -> Option<&str> {
        if self.decoration.is_some() {
            return None;
        }
        self.hyperlinks.get(old_id).map(String::as_str)
    }

    /// Returns the relationship id to write for a picture that referenced
    /// `part`.
    ///
    /// Inside a decoration part the id comes from **that part's** map, because the
    /// relationship a picture needs is declared in the header's own `.rels` and
    /// nowhere else. That is why both maps exist rather than one of them being
    /// wrong.
    #[must_use]
    pub fn media_rel(&self, part: Option<&PartId>) -> Option<&str> {
        let part = part?;
        if let Some((_, media)) = &self.decoration {
            return media.get(part.as_str()).map(String::as_str);
        }
        self.media.get(part.as_str()).map(String::as_str)
    }

    /// Returns the relationship id to write for a header/footer part.
    #[must_use]
    pub fn header_footer_rel(&self, part: &PartId) -> Option<&str> {
        self.header_footers.get(part.as_str()).map(String::as_str)
    }

    /// Returns the mutable report.
    pub fn report(&mut self) -> &mut NormalizationReport {
        self.report
    }

    /// Allocates the next `wp:docPr/@id`.
    pub fn next_doc_pr_id(&mut self) -> u32 {
        let id = self.next_doc_pr_id;
        self.next_doc_pr_id += 1;
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
