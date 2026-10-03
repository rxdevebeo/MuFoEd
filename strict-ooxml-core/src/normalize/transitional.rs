//! Transitional → Strict normalization (`TZ-STRICT-OOXML-RUST.md` §10).
//!
//! The pipeline runs over a streaming XML rewriter so that a part is
//! transformed without ever holding a DOM, which matters because the
//! documents this has to survive are real files, not fixtures: the largest
//! in the corpus is over a megabyte of XML.
//!
//! Stage order follows §10.0 and is not negotiable — names are canonicalized
//! (T1–T3) before values are interpreted (T4) and before anything is removed
//! (T5), so every later table is keyed on Strict `QName`s:
//!
//! ```text
//! T0 conformance · T1 namespaces · T2 relationship/content types
//! T3 element and attribute names · T4 enumerated values
//! T5 Transitional-only removal · T6 MCE resolution · T7 legacy graphics
//! T8 invariants
//! ```
//!
//! Invariants (§10.10) and where each is enforced:
//!
//! * **determinism** — no hash-ordered iteration reaches the output, and the
//!   report is sorted on read, so two runs agree even though parts are read
//!   in whatever order the package walk produces;
//! * **idempotence on Strict input** — [`NormalizationReport::is_noop`],
//!   asserted by criterion SC-1;
//! * **no silent loss** — [`NormalizationReport::verify_no_silent_loss`],
//!   SC-4;
//! * **no panics** — every fallible step returns [`Result`].

use std::borrow::Cow;
use std::sync::Mutex;

use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};

use crate::error::{Result, SourceLocation, StrictError};
use crate::normalize::mce;
use crate::normalize::report::{LossRecord, NormalizationReport, Severity};
use crate::normalize::tables::{self, is_ignorable_extension, VML_NAMESPACES};
use crate::normalize::vml;
use crate::ns::registry::NamespaceRegistry;
use crate::opc::rels::{
    PACKAGE_CORE_PROPERTIES_REL, PACKAGE_THUMBNAIL_REL, REL_TYPES, TRANSITIONAL_OFFICE_BASE,
};
use crate::part::PartId;

/// Markup Compatibility and Extensibility. The namespace is the same in
/// Transitional and Strict, so `map_uri` leaves it alone.
const MC_NAMESPACE: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Standard OPC package-relationships namespace (both families; ADR-0015).
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// Standard OPC core-properties namespace (both families; ADR-0015).
const PACKAGE_CORE_PROPERTIES_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
/// Legacy purl OPC URIs written by earlier versions of this project (AUD-20).
const LEGACY_PURL_PACKAGE_REL_NS: &str = "http://purl.oclc.org/ooxml/package/relationships";
const LEGACY_PURL_CORE_PROPERTIES_NS: &str =
    "http://purl.oclc.org/ooxml/package/metadata/coreProperties";
const LEGACY_PURL_CORE_PROPERTIES_REL: &str =
    "http://purl.oclc.org/ooxml/package/relationships/metadata/core-properties";
const LEGACY_PURL_THUMBNAIL_REL: &str =
    "http://purl.oclc.org/ooxml/package/relationships/metadata/thumbnail";

/// Which markup-compatibility branch to keep (`TZ` §10.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McePolicy {
    /// Take the first `mc:Choice` whose `Requires` prefixes we all support.
    #[default]
    ProcessChoice,
    /// Always take `mc:Fallback`.
    PreferFallback,
    /// Resolve nothing; record it and leave the block alone.
    Report,
}

/// How strictly to treat a broken Strict invariant (`TZ` §10.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvariantMode {
    /// Record violations as warnings and keep the document.
    #[default]
    Lenient,
    /// Fail the part.
    Strict,
}

/// Configuration for [`TransitionalNormalizer`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NormalizerOptions {
    /// Markup-compatibility branch policy.
    ///
    /// **Read, since 2026-10-02.** It was declared, documented and default-valued
    /// from the start and read nowhere, which made `with_mce` a constructor for a
    /// setting with no behaviour behind it — the same category of defect as the
    /// XSD harness version that caught `XMLSchemaParseError` and passed. See
    /// [`mce`](crate::normalize::mce) for what each policy does and
    /// [`McePolicy`] for what "understood" means here.
    pub mce: McePolicy,
    /// What to do about a broken Strict invariant (`TZ` §10.9).
    ///
    /// **Read, since 2026-10-02**, and the reason the doc comment on
    /// [`TransitionalNormalizer::normalize`] can promise an error under
    /// [`Strict`](InvariantMode::Strict). Before that the promise was
    /// unimplementable: `verify_no_silent_loss` is the only invariant the pipeline
    /// could check, and it is unconditional.
    pub invariants: InvariantMode,
    /// A cap on the bytes one part may expand to while being rewritten.
    ///
    /// Rewriting can grow a part slightly; without a cap a crafted input
    /// could push a part past the point where the rest of the pipeline can
    /// hold it. Zero means "no extra allowance".
    pub max_expansion_bytes: usize,
}

impl NormalizerOptions {
    /// The default expansion allowance, as a multiple of the input.
    const DEFAULT_EXPANSION: usize = 4;

    /// Returns options with an explicit markup-compatibility policy.
    #[must_use]
    pub fn with_mce(mce: McePolicy) -> Self {
        Self {
            mce,
            ..Self::default()
        }
    }

    /// Returns options with an explicit invariant mode.
    ///
    /// The counterpart to [`with_mce`](Self::with_mce), and it exists for the
    /// same reason: a caller that wants `Strict` semantics should be able to say
    /// so without building the whole struct by hand — which is how the option
    /// ended up unreadable in the first place.
    #[must_use]
    pub fn with_invariants(invariants: InvariantMode) -> Self {
        Self {
            invariants,
            ..Self::default()
        }
    }

    fn expansion_limit(&self, input: usize) -> usize {
        if self.max_expansion_bytes == 0 {
            input
                .saturating_mul(Self::DEFAULT_EXPANSION)
                .max(input + 1024)
        } else {
            input.saturating_add(self.max_expansion_bytes)
        }
    }
}

/// Normalizes Transitional OOXML parts to Strict at the raw-bytes seam.
///
/// Implements [`RawNormalizer`]. The report is accumulated behind a mutex
/// because the seam hands out a shared `&self` and because a package may be
/// walked in any order; [`Self::report`] returns it sorted.
#[derive(Debug, Default)]
pub struct TransitionalNormalizer {
    options: NormalizerOptions,
    report: Mutex<NormalizationReport>,
}

impl TransitionalNormalizer {
    /// Creates a normalizer with the default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a normalizer with explicit options.
    #[must_use]
    pub fn with_options(options: NormalizerOptions) -> Self {
        Self {
            options,
            report: Mutex::new(NormalizationReport::new()),
        }
    }

    /// The accumulated report, ordered.
    ///
    /// # Panics
    ///
    /// Never in practice: the mutex is only held for the duration of a field
    /// update, and nothing inside that scope can panic. A poisoned lock would
    /// mean a previous thread panicked mid-report, which is already a bug.
    pub fn report(&self) -> NormalizationReport {
        self.report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Runs the pipeline over one part.
    ///
    /// Returns [`Cow::Borrowed`] when nothing changed, which is the
    /// already-Strict case and the property SC-1 asserts.
    ///
    /// # Errors
    ///
    /// Returns an error when the part is not well-formed XML, when rewriting
    /// would exceed [`NormalizerOptions::max_expansion_bytes`], or when
    /// [`InvariantMode::Strict`] is set and an invariant is violated.
    pub fn normalize<'a>(&self, part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        // Cheap pre-check: a part with no Transitional signal at all is
        // returned untouched without being parsed. This is what makes SC-1
        // cheap as well as true.
        if !part_needs_normalization(bytes) {
            return Ok(Cow::Borrowed(bytes));
        }
        let limit = self.options.expansion_limit(bytes.len());
        let mut report = self
            .report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut reader = Reader::from_reader(bytes);
        let config = reader.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = false;
        let mut writer = Writer::new(Vec::with_capacity(bytes.len()));
        let mut context = PartContext::new(part.clone());
        context.mce = self.options.mce;
        context.invariants = self.options.invariants;
        context.used_prefixes = used_prefixes(bytes);
        if declares_a_vml_picture(bytes) {
            context.vml_picture_prefixes = vml::REQUIRED_NAMESPACES
                .iter()
                .map(|(prefix, strict, _)| (*prefix, *strict))
                .collect();
        }

        // One queue for both sources, because a resolved `mc:AlternateContent`
        // branch has to meet the same dispatch as the rest of the part: a
        // `mc:Fallback` in six corpus documents is a `w:pict`, and a branch that
        // went around the loop would have its picture dropped by T7 instead of
        // converted by it.
        let mut buffered: std::collections::VecDeque<Event<'static>> =
            std::collections::VecDeque::new();

        loop {
            // The seam hands the normalizer a *prefix* of a part as well as
            // whole parts: conformance detection scans the first 64 KiB for the
            // root namespace before deciding whether to read the rest. A
            // truncated prefix is not valid XML, and re-serializing it would
            // unbalance the tags — so the namespace URIs it does contain are
            // rewritten textually instead.
            //
            // Returning the prefix untouched would be worse than broken XML:
            // the scan would then report a Transitional root next to the
            // Strict roots the whole parts produce, and every such document
            // would be rejected as Mixed.
            // A part that does not parse is left exactly as it is. Two cases
            // reach here, and neither is the normalizer's to reject: damaged
            // input, which the Strict parser reports later with a real
            // location, and — historically — a truncated prefix, which no
            // longer happens because conformance detection scans the prefix
            // raw and asks the registry what the URI will become.
            let event = if let Some(event) = buffered.pop_front() {
                event
            } else {
                let Ok(event) = reader.read_event().map(into_owned_event) else {
                    return Ok(Cow::Borrowed(bytes));
                };
                event
            };
            let Event::Eof = event else {
                // ---- T6 and T7: the two elements judged whole ---------------
                //
                // Both need their subtree before they can be judged —
                // `w:pict` because what makes a `v:shape` a picture is a
                // `v:imagedata` on a descendant, and `mc:AlternateContent`
                // because what it resolves to is in the `Requires` of a
                // descendant `mc:Choice`. A streaming pass sees the start tag
                // and not the thing that decides.
                let mut source = EventSource::new(&mut reader, &mut buffered);
                if context.skip_depth == 0 && is_legacy_graphics(&event, &context) {
                    let subtree = collect_subtree(&mut source, event);
                    Self::rewrite_legacy_graphics(
                        &mut writer,
                        &subtree,
                        &mut context,
                        &mut report,
                        &mut buffered,
                    )?;
                } else if context.skip_depth == 0 && is_alternate_content(&event, &context) {
                    let subtree = collect_subtree(&mut source, event);
                    rewrite_alternate_content(
                        &mut writer,
                        &subtree,
                        &mut context,
                        &mut report,
                        &mut buffered,
                    )?;
                } else {
                    Self::rewrite_event(&mut writer, event, &mut context, &mut report)?;
                }
                if writer.get_ref().len() > limit {
                    return Err(StrictError::LimitExceeded {
                        kind: crate::error::LimitKind::TextLen,
                        limit: limit as u64,
                        actual: writer.get_ref().len() as u64,
                    });
                }
                continue;
            };
            break;
        }

        let output = writer.into_inner();
        if part.as_str().ends_with("footer1.xml") {
            let _ = std::fs::write(
                "C:/Users/gamer/AppData/Local/Temp/opencode/norm-footer1.xml",
                &output,
            );
        }
        // T8. `verify_no_silent_loss` is the one invariant that was always checked,
        // and `InvariantMode::Strict` adds the other two the pipeline can
        // actually decide on its own — see [`check_invariants`]. The promise on
        // this function's doc comment ("an error when `Strict` is set and an
        // invariant is violated") was unimplementable until now, because nothing
        // else read the field.
        let transformed = report.applied().iter().any(|record| record.count > 0);
        check_invariants(part, &output, transformed, context.invariants, &mut report).map_err(
            |detail| StrictError::NormalizationInvariantViolation {
                location: SourceLocation::new(part.clone(), 1, 1, 0),
                detail,
            },
        )?;
        report.verify_no_silent_loss().map_err(|reason| {
            StrictError::NormalizationInvariantViolation {
                location: SourceLocation::new(part.clone(), 1, 1, 0),
                detail: reason,
            }
        })?;
        Ok(Cow::Owned(output))
    }

    /// Rewrites a buffered `w:pict` or `w:object` subtree: into a `DrawingML` picture
    /// when the shape is one, and into nothing (recorded) when it is not.
    ///
    /// The events in `subtree` are **raw** — nothing in them has been rewritten
    /// yet, because the subtree was collected before T1–T7 ran on it — so the
    /// extraction resolves them against the prefix table as it stands. That is
    /// what makes `v:imagedata/@r:id` findable, and it is also why the subtree is
    /// buffered at all: a streaming pass sees the `v:shape` start tag and not the
    /// `v:imagedata` that decides what it is.
    ///
    /// `w:object` comes through the same door for a different reason. Strict
    /// declares `w:object`, so it is **not** a removal — but everything inside it
    /// is, `o:OLEObject` and the VML shape alike, and an empty `<w:object/>` is a
    /// schema-valid element with nothing in it. What the reader saw before the
    /// conversion was the `v:imagedata` **preview raster**, and the audit §12 is
    /// explicit that this is the only part of an OLE object that is recoverable:
    /// "OLE is an executable object; Strict does not have it, and no substitute
    /// exists". So the preview is kept as a picture, the object itself is named as
    /// lost, and the page shows what it showed before. That is a different claim
    /// from "the picture was converted", and the report makes it separately.
    fn rewrite_legacy_graphics(
        writer: &mut Writer<Vec<u8>>,
        subtree: &[Event<'static>],
        context: &mut PartContext,
        report: &mut NormalizationReport,
        _buffered: &mut std::collections::VecDeque<Event<'static>>,
    ) -> Result<()> {
        let location = context.location();
        let element = legacy_graphics_element(subtree);

        let Some((shape, wrap)) = vml::classify(subtree, context) else {
            // Not a shape this converts. Everything under `w:pict` is VML, and
            // Strict declares none of it, so the subtree goes and the loss is
            // named - the same treatment `T7.vml` gave it, and for the same
            // reason: a dropped node that the report does not name is the failure
            // mode this whole module is written against.
            report.record_loss(LossRecord {
                transform_id: "T7.vml",
                feature_id: element.to_owned(),
                reason: "VML is not part of Strict; the node is dropped, not rendered, and \
                         nothing in it is a shape this conversion knows how to draw"
                    .to_owned(),
                severity: Severity::Lossy,
                locations: vec![location.clone()],
            });
            report.count_reported_removal(1);
            return Ok(());
        };

        if let vml::Shape::Freeform(frame) = &shape {
            // The frame is kept and the **geometry is dropped**, and the two are
            // named separately. A frame with a guessed path is a shape in the right
            // place drawn wrong, which is worse than a frame with no path - and
            // `a:custGeom` has no arc command for the `r` command both freeform
            // shapes in the corpus use.
            report.record_loss(LossRecord {
                transform_id: "T7.vml-freeform",
                feature_id: "v:shape/@path".to_owned(),
                reason: format!(
                    "the VML shape {:?} is a freeform path that uses a command a:custGeom has no \
                     equivalent for, so its frame is kept and its outline is dropped",
                    frame.name
                ),
                severity: Severity::Lossy,
                locations: vec![location.clone()],
            });
        }

        let text_box = matches!(shape, vml::Shape::TextBox(_));
        let doc_pr_id = context.next_doc_pr_id();
        // The content of a text box is written **in place**, through the same
        // `rewrite_event` as the rest of the part, which is what makes it ordinary
        // WML by the time it lands. The alternative - queue the content and queue
        // a tail behind it - needs three orderings right at once, and when one of
        // them was wrong the part came out with the shape's body properties
        // outside the shape and the paragraph one level too deep.
        let (head, tail) = if text_box {
            vml::text_box_shape_events(&shape, wrap, doc_pr_id, report, &location)
        } else {
            vml::shape_events(&shape, wrap, doc_pr_id, report, &location)
        };
        for event in head {
            writer
                .write_event(event)
                .map_err(|error| xml_error(&context.part, error.to_string()))?;
        }
        if text_box {
            for content in Self::drain_textbox_content(subtree) {
                Self::rewrite_event(writer, content, context, report)?;
            }
            for event in vml::text_box_close() {
                writer
                    .write_event(event)
                    .map_err(|error| xml_error(&context.part, error.to_string()))?;
            }
        }
        for event in tail {
            writer
                .write_event(event)
                .map_err(|error| xml_error(&context.part, error.to_string()))?;
        }
        report.record("T7.vml-shape", 1);

        if element == "w:object" {
            // The preview survives; the object does not, and that is a **second**
            // claim from the conversion above. Recorded separately so a reader of
            // the report can tell "the picture is here" from "the thing the
            // picture stood for is gone" - they are different and only one of them
            // is recoverable.
            report.record_loss(LossRecord {
                transform_id: "T7.ole",
                feature_id: "w:object".to_owned(),
                reason: "an OLE object is an executable object Strict has no substitute for; \
                         its preview raster was kept as a picture and the object itself is gone"
                    .to_owned(),
                severity: Severity::Lossy,
                locations: vec![location.clone()],
            });
            // One node removed: the `w:object` the preview replaced. Its VML
            // children went with it and are inside that node.
            report.count_reported_removal(1);
        }
        // A converted shape is a **mapping**, not a loss: the image relationship is
        // the same one, the part behind it is carried by the pass-through, and
        // nothing was dropped - so `count_reported_removal` is deliberately NOT
        // called for the `w:pict` case. Booking it as one would put the two
        // counters `verify_no_silent_loss` compares out of step.
        Ok(())
    }

    /// The events of a `w:txbxContent`'s **children**, and nothing else.
    ///
    /// Two things have to be right here, and the first version got the second
    /// wrong in a way that did not converge:
    ///
    /// - the `v:textbox`'s wrapper is stripped, because the conversion writes its
    ///   own `wps:txbx/w:txbxContent` pair and a second wrapper would nest them:
    ///   valid XML, and a text box whose every paragraph is one level too deep;
    /// - **the walk stops at the wrapper's end tag.** Carrying on collects the
    ///   rest of the `w:pict` - the `v:shape` and the `w:pict` itself - and
    ///   feeding those back through the event loop makes it meet the same `w:pict`
    ///   again and convert it again, producing twice as much markup each time. That
    ///   is the difference between a document that grows and a normalizer that
    ///   does not terminate.
    ///
    /// The **text** events are carried: a text box whose paragraphs arrive with
    /// their runs emptied is a frame around nothing, and it passes every
    /// structural assertion.
    fn drain_textbox_content(subtree: &[Event<'static>]) -> Vec<Event<'static>> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        for event in subtree {
            match event {
                Event::Start(start) => {
                    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                    if depth == 0 {
                        if !name.ends_with(":txbxContent") {
                            continue;
                        }
                        depth = 1;
                        continue;
                    }
                    depth += 1;
                    out.push(event.clone());
                }
                Event::End(_) => {
                    if depth == 0 {
                        continue;
                    }
                    depth -= 1;
                    if depth == 0 {
                        return out;
                    }
                    out.push(event.clone());
                }
                Event::Empty(_) | Event::Text(_) if depth > 0 => out.push(event.clone()),
                _ => {}
            }
        }
        out
    }

    /// Rewrites one event into the writer. Dropping is what makes this more than a
    /// rename pass: a removed element is a whole subtree, so the reader has to keep
    /// consuming until its matching end tag without writing anything.
    /// `skip_depth` counts the open elements still inside the dropped subtree.
    fn rewrite_event(
        writer: &mut Writer<Vec<u8>>,
        event: Event<'_>,
        context: &mut PartContext,
        report: &mut NormalizationReport,
    ) -> Result<()> {
        match event {
            // The declaration is the only place the XML version appears, and
            // it changes how attribute values are unescaped, so it is read
            // before any element is touched.
            Event::Decl(decl) => {
                if let Ok(text) = std::str::from_utf8(&decl) {
                    if text.contains("1.1") {
                        context.xml_version = quick_xml::XmlVersion::Explicit1_1;
                    }
                }
                write_passthrough(writer, &Event::Decl(decl))
                    .map_err(|error| xml_error(&context.part, error.to_string()))
            }
            Event::Start(start) => {
                if context.skip_depth > 0 {
                    context.skip_depth += 1;
                    return Ok(());
                }
                // The element is pushed onto the open-element stack by
                // `rewrite_start` itself, once it has agreed to keep it, so the
                // stack is exactly the tree being written and a dropped subtree
                // leaves nothing behind for the matching `End` to pop.
                match Self::rewrite_start(&start, context, report) {
                    Rewritten::Keep(rewritten) => write_start(writer, &rewritten)
                        .map_err(|error| xml_error(&context.part, error.to_string())),
                    Rewritten::Drop => {
                        context.skip_depth = 1;
                        Ok(())
                    }
                }
            }
            Event::Empty(start) => {
                if context.skip_depth > 0 {
                    return Ok(());
                }
                match Self::rewrite_start(&start, context, report) {
                    // `rewrite_start` pushes onto the open-element stack, and for
                    // an `Event::Empty` there is no `Event::End` to pop it - a
                    // self-closing tag is one event, not two. Without this the
                    // stack grew by one per empty tag and `parent_local()` returned
                    // the PREVIOUS SIBLING for every element after the first, which
                    // is what made the container-keyed rules fire on nothing: a
                    // `w:left` following a `w:top` inside `w:tblCellMar` was asked
                    // whether `w:top` renames `w:left`, and no table has that entry.
                    //
                    // The pop is after the write so a failed write leaves the stack
                    // as it was rather than unwinding a caller that will bail out.
                    Rewritten::Keep(rewritten) => {
                        let written = write_empty(writer, &rewritten)
                            .map_err(|error| xml_error(&context.part, error.to_string()));
                        context.pop_element();
                        written
                    }
                    // A self-closing tag has no subtree: the decision is the
                    // whole removal.
                    Rewritten::Drop => {
                        context.pop_element();
                        Ok(())
                    }
                }
            }
            Event::End(end) => {
                if context.skip_depth > 0 {
                    context.skip_depth -= 1;
                    return Ok(());
                }
                context.pop_element();
                write_end(writer, &end).map_err(|error| xml_error(&context.part, error.to_string()))
            }
            other => write_passthrough(writer, &other)
                .map_err(|error| xml_error(&context.part, error.to_string())),
        }
    }

    /// Applies T1–T5 and T7 to a start/empty tag.
    fn rewrite_start(
        start: &BytesStart<'_>,
        context: &mut PartContext,
        report: &mut NormalizationReport,
    ) -> Rewritten {
        let location = context.location();
        let (local, uri) = resolve(start, context);
        let qualified = qualify(&uri, &local);

        // ---- T7: legacy graphics -------------------------------------
        if VML_NAMESPACES.contains(&uri.as_str()) {
            report.record_loss(LossRecord {
                transform_id: "T7.vml",
                feature_id: qualified,
                reason: "VML is not part of Strict; the node is dropped, not rendered".to_owned(),
                severity: Severity::Lossy,
                locations: vec![location],
            });
            // The whole subtree goes with the root, and the record above is
            // what accounts for it (SC-4).
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }
        if let Some(removal) = tables::removal_for(&local) {
            // ---- T5: Transitional-only element -------------------------
            report.record_loss(LossRecord {
                transform_id: "T5.removal",
                feature_id: qualified,
                reason: removal.reason.to_owned(),
                severity: removal.severity,
                locations: vec![location],
            });
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }
        if tables::is_ignorable_extension(&uri) {
            // ---- T5: a producer extension we do not implement -----------
            report.record_loss(LossRecord {
                transform_id: "T5.extension",
                feature_id: qualified,
                reason: "producer extension outside the standard; Markup \
                         Compatibility permits skipping it"
                    .to_owned(),
                severity: Severity::Ignorable,
                locations: vec![location],
            });
            report.count_reported_removal(1);
            return Rewritten::Drop;
        }

        // ---- T3: element name -----------------------------------------
        // Keyed by the PARENT, because "is this `left` an edge or a page
        // margin?" is a question about the container: Strict's `CT_TblBorders`,
        // `CT_TcBorders`, `CT_TblCellMar` and `CT_TcMar` declare `start`/`end`
        // and its `CT_PBdr`, `CT_PageBorders` and `CT_PageMar` still declare
        // `left`/`right`. A global rename would zero every paragraph border,
        // every page border and all four page margins on every page. See the
        // `tables` module docs, and the negative test that holds the parents
        // with no entry.
        //
        // **The namespace test is on the REWRITTEN uri, and that is load-bearing.**
        // `is_wml` compares against `.../wordprocessingml/main`, which is the
        // Strict spelling; the Transitional one is `.../wordprocessingml/2006/main`
        // and does not end with it. The attribute path has always tested
        // `effective_uri` and therefore always worked — and the element path
        // tested the original, so on a Transitional part it was always false and
        // the T3 element table was dead. It looked alive: the table was
        // populated, the call was made, and the census reported `TZ-04 = 0`
        // because on this corpus the constructs only appear in parts the writer
        // *regenerates* (and the writer already emits `start`/`end`), so there
        // was nothing for the rule to fail on. The gate that caught it is
        // `the_direction_neutral_rename_follows_the_container_and_not_the_name`,
        // which is a unit test on purpose: **a corpus gate cannot see a rule
        // that the corpus never exercises.**
        let element_uri = map_uri(&uri).unwrap_or_else(|| uri.clone());
        let parent = context.parent_local().unwrap_or_default();
        let renamed = is_wml(&element_uri)
            .then(|| tables::rename_element(parent, &local))
            .flatten();
        let new_local = renamed.unwrap_or(local.as_str());
        if renamed.is_some() {
            report.record("T3.rename", 1);
        }

        // ---- T1: namespace declarations -------------------------------
        let mut buffer = start.to_owned().into_owned();
        buffer.clear_attributes();
        // T4 and T3: both keys are about the element this tag is, and neither
        // the element nor its parent is on the stack yet - the element is pushed
        // only after the pipeline has agreed to keep it, so the two are parked
        // here for the attribute loop.
        context.pending_parent = parent.to_owned();
        context.pending_element.clone_from(&local);
        // ---- T4: the one attribute Strict spells as six ----------------
        //
        // `w:tblLook` is the only such case: Strict's `CT_TblLook` declares six
        // `s:ST_OnOff` attributes and no `@w:val`. Six attributes cannot come back
        // through the attribute loop, which can only rewrite what the producer
        // wrote, so the decoding happens here and the results are pushed onto
        // the buffer afterwards. 838 occurrences in 14 corpus documents — and,
        // as with the T3 element table, none of them in a part this write
        // passes through, so the unit test is what holds this and the census
        // only says the corpus is clean.
        let tbl_look = if is_wml(&element_uri) && local == "tblLook" {
            decode_and_report_tbl_look(start, context, report, &location)
        } else {
            None
        };
        let drop_w_val = tbl_look.is_some();
        copy_attributes(start, drop_w_val, &mut buffer, context, report);
        // ---- T4: `CT_PageMar`'s three required attributes ----------------
        if is_wml(&element_uri) && local == "pgMar" {
            synthesize_page_margin(context, start, &element_uri, &mut buffer, report);
        }
        // ---- T4: the six attributes the bit mask becomes ---------------
        //
        // Only the ones the producer did not already write. Word emits
        // `w:val="04A0"` *and* `w:firstRow="1" w:lastRow="0" ...` in the same
        // tag, for consumers that understand either spelling, and six of this
        // corpus's tables do exactly that; pushing six more would have produced
        // a tag with a repeated `w:firstRow`, which is a hard XML error rather
        // than a tolerated one.
        if let Some(decoded) = tbl_look {
            let prefix = context.prefix_for(&element_uri_of(&uri));
            let already: Vec<&str> = tables::TBL_LOOK_BITS
                .iter()
                .map(|(_, attribute)| *attribute)
                .filter(|attribute| {
                    start
                        .attributes()
                        .flatten()
                        .any(|present| present.key.as_ref().ends_with(attribute.as_bytes()))
                })
                .collect();
            for (on, (_, attribute)) in decoded.flags.iter().zip(tables::TBL_LOOK_BITS) {
                if already.contains(attribute) {
                    continue;
                }
                let key = PartContext::qualified_name(&prefix, attribute);
                buffer.push_attribute((key.as_str(), if *on { "true" } else { "false" }));
            }
        }
        // ---- T7: the DrawingML prefixes a converted picture will need ----
        declare_vml_picture_prefixes(&mut buffer, context);
        // The element name is not an attribute: it has to be set through
        // `set_name`, or the name survives *and* a bogus copy of it is
        // emitted as one.
        //
        // The prefix is looked up under the *rewritten* namespace. Using the
        // original here is what made every element pick up a freshly minted
        // `n0` prefix while its declaration still named `w`.
        let new_local = new_local.to_string();
        let prefix = context.prefix_for(&element_uri);
        let qualified = PartContext::qualified_name(&prefix, &new_local);
        buffer.set_name(qualified.as_bytes());
        // The element joins the open-element stack here rather than in the event
        // loop, because here is the only place its final name is known: the loop
        // holds a `BytesStart` and the name it carries is the *producer's*, and
        // an earlier version pushed that — through a helper that returned the
        // stack's own last entry, which is the parent's name — so every entry
        // was the wrong string and the four direction-neutral renames stayed
        // unreachable while the table said they were populated.
        //
        // The **Strict** name is what is recorded, because that is what the table
        // is keyed on: `CT_TcBorders` and friends are containers the rename never
        // touches, and a key spelled either way would be a second way to be wrong.
        context.push_element(&new_local);
        Rewritten::Keep(buffer.into_owned())
    }
}

/// The Strict namespace a WML element's URI maps to, or the URI itself.
fn element_uri_of(uri: &str) -> String {
    map_uri(uri).unwrap_or_else(|| uri.to_owned())
}

/// Whether a part might emit a converted VML picture, so its root needs the three
/// `DrawingML` prefixes.
///
/// Textual, like [`part_needs_normalization`] and for the same reason: the question
/// is asked before the part is parsed, and the answer only has to be *safe*. It
/// looks for `w:pict` and `v:shape` together because a part with neither cannot
/// convert anything, and a false positive costs three unused namespace
/// declarations — which the census's `extension` signal does not count and the
/// schema does not reject.
///
/// A false **negative** would cost a part that cannot be parsed, which is why the
/// needle is the element name `w:pict` rather than the shape: `w:pict` is the one
/// element the conversion replaces, and every VML picture in a document arrives
/// inside one.
fn declares_a_vml_picture(bytes: &[u8]) -> bool {
    // Both elements, and the second is not optional: `w:object` carries an OLE
    // preview raster in exactly the shape `w:pict` does, so a pre-scan that only
    // knew about `w:pict` left `word/document.xml` with no `xmlns:a` and the part
    // stopped being well-formed — which is what `Интегралы (2).docx` did, and the
    // first version of this needle is why.
    [":pict", ":object"]
        .iter()
        .any(|needle| memchr::memmem::find(bytes, needle.as_bytes()).is_some())
}

/// Runs every attribute of a start tag through [`rewrite_attribute`] and copies
/// the results onto `buffer`.
///
/// `drop_w_val` is the `w:tblLook` case: its `@w:val` is the Transitional spelling
/// of six attributes Strict declares, and Strict declares no `@w:val`. The check
/// lives here rather than in [`rewrite_attribute`] because it is **the element's**
/// and the attribute path has no way to know which element it is on — dropping
/// every `@w:val` in the part would delete the entire vocabulary.
fn copy_attributes(
    start: &BytesStart<'_>,
    drop_w_val: bool,
    buffer: &mut BytesStart<'static>,
    context: &mut PartContext,
    report: &mut NormalizationReport,
) {
    for attribute in start.attributes().flatten() {
        if drop_w_val && attribute.key.as_ref().ends_with(b":val") {
            continue;
        }
        match rewrite_attribute(&attribute, context, report) {
            RewrittenAttribute::Keep(key, value) => {
                // `value` is already in its final escaped form: a stage that
                // rewrote it escaped it, and a stage that did not left the
                // producer's own bytes alone.
                buffer.push_attribute((key.as_str(), value.as_str()));
            }
            // A declaration that must not be emitted: the namespace is going away
            // with its nodes, and leaving it would only advertise a namespace
            // nothing uses.
            RewrittenAttribute::Drop => {
                report.record("T1.namespace-decl", 1);
            }
        }
    }
}

/// The T8 checks this pipeline can decide on its own (`TZ` §10.9), and the ones
/// it cannot.
///
/// §10.9 lists five invariant classes. Three of them are **not decidable here** and
/// it is worth saying which, because a claim of coverage would be false:
///
/// - "an element or attribute the schema does not declare" — that is the XSD
///   gate's job, and it needs the schemas, which are deliberately not in the crate
///   graph (the ECMA set carries no licence grant; see `xtool/xsd-gate/`);
/// - "an order that violates the schema's `xsd:sequence`" — the writer's `order`
///   table owns that, and `tests/schema_order.rs` plus `xsd_gate.py`'s G21
///   comparison check it;
/// - "a relationship that is neither internal nor `External`" — a `.rels` part is
///   not XML the normalizer resolves namespaces in, and OPC owns it.
///
/// Two it **can**, and this does:
///
/// 1. a namespace URI that is neither the Strict form of a registered family nor
///    ignorable;
/// 2. an `mc:Ignorable` that survived T6 — conformance is defined on the post-MCE
///    part (ECMA-376 Part 1 §2.1(ii)), so MCE markup still in the output is a
///    conformance defect whatever the schema says.
///
/// **Only a part the pipeline actually changed is checked**, and that is the
/// boundary that makes the check mean something. The seam offers every part of the
/// package, including the ones the writer copies verbatim — a `SmartArt` diagram
/// part declaring `ds`, `dgm`, `a14` and half a dozen more vendor namespaces we
/// carry on purpose (ADR-0007). Those bytes are the producer's, the pipeline
/// applied no stage to them, and calling their namespaces a violation of *our*
/// output would be a false positive on the first document with a diagram in it. A
/// part whose bytes are ours is the only part an invariant about our output can be
/// about.
///
/// Under [`InvariantMode::Lenient`] each violation is a record and the part is
/// kept; under [`InvariantMode::Strict`] the first one is an error. The asymmetry is
/// deliberate and is what §10.9 asks for.
fn check_invariants(
    part: &PartId,
    output: &[u8],
    transformed: bool,
    mode: InvariantMode,
    report: &mut NormalizationReport,
) -> std::result::Result<(), String> {
    if !transformed {
        return Ok(());
    }
    let location = SourceLocation::new(part.clone(), 1, 1, 0);
    let mut violations: Vec<String> = Vec::new();

    for attribute in namespace_declarations(output) {
        let (prefix, uri) = attribute;
        let known = crate::ns::registry::strict_form(&uri).is_some()
            || uri == mce::MC_NS
            || tables::is_ignorable_extension(&uri)
            || VML_NAMESPACES.contains(&uri.as_str())
            || uri == XML_NAMESPACE;
        if !known {
            violations.push(format!(
                "namespace prefix {prefix} is bound to {uri}, which is neither the Strict form \
                 of a registered family nor an extension this pipeline removes"
            ));
        }
    }

    if output.windows(4).any(|window| window == b"mc:I") {
        violations.push(
            "an mc:Ignorable survived T6; Strict conformance is defined on the post-MCE part \
             (ECMA-376 Part 1 §2.1 clause (ii))"
                .to_owned(),
        );
    }

    if violations.is_empty() {
        return Ok(());
    }
    let detail = violations.join("; ");
    if matches!(mode, InvariantMode::Strict) {
        return Err(format!(
            "{part}: {} Strict invariant violation(s): {detail}",
            violations.len()
        ));
    }
    for violation in violations {
        report.record_loss(LossRecord {
            transform_id: "T8.invariant",
            feature_id: part.as_str().to_owned(),
            reason: violation,
            severity: Severity::Ignorable,
            locations: vec![location.clone()],
        });
    }
    Ok(())
}

/// The `xml:` namespace, which XML binds and no document declares.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Every `xmlns[:prefix]="uri"` in `output`, as `(prefix, uri)`.
fn namespace_declarations(output: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let text = String::from_utf8_lossy(output);
    let mut rest = text.as_ref();
    while let Some(at) = rest.find("xmlns") {
        rest = &rest[at + 5..];
        let (prefix, tail) = match rest.strip_prefix(':') {
            Some(tail) => match tail.find('=') {
                Some(equals) => (
                    rest[..equals].trim_end_matches('"').to_owned(),
                    &tail[equals + 1..],
                ),
                None => continue,
            },
            None => (String::new(), rest),
        };
        let Some(quoted) = tail.strip_prefix('"') else {
            continue;
        };
        let Some(end) = quoted.find('"') else {
            continue;
        };
        out.push((prefix, quoted[..end].to_owned()));
        rest = &quoted[end + 1..];
    }
    out
}
/// will need.
///
/// On the **root**, once, and only for a part that has a `w:pict` to convert.
/// Declaring them anywhere else is not an option — the root is the only element
/// that is an ancestor of every use — and declaring them for a part that emits
/// nothing is the unused-declaration debt
/// `strict-ooxml-write/tests/strict_conformance.rs` already refuses.
///
/// The prefix is **remembered** as well as declared, because `prefix_for` reads the
/// table to decide what to call a namespace: a declaration the table does not know
/// about would make every converted element pick up a freshly minted `n0` while
/// the root still says `a`.
fn declare_vml_picture_prefixes(buffer: &mut BytesStart<'static>, context: &mut PartContext) {
    if context.root_written {
        return;
    }
    context.root_written = true;
    for (prefix, strict) in context.missing_vml_picture_prefixes() {
        let key = format!("xmlns:{prefix}");
        buffer.push_attribute((key.as_str(), strict));
        context.remember_prefix(prefix.as_bytes().to_vec(), strict.to_owned());
    }
}

/// The elements whose subtree T7 judges whole rather than dropping.
///
/// `w:pict` and `w:object` are both `EG_RunInnerContent` members, so both only
/// appear inside a run. The namespace test is on the element's **own** prefix
/// rather than on the literal `w`: a producer that binds the WML namespace to
/// something else still produces a part this has to read, and a `v:pict`-shaped
/// local name in another vocabulary must not be collected as one.
const LEGACY_GRAPHICS: [&str; 2] = ["pict", "object"];

/// The local name of a `w:pict` or `w:object` start tag, if that is what it is.
fn legacy_graphics_local<'a>(name: &'a str, context: &PartContext) -> Option<&'a str> {
    let (local, bound) = match name.split_once(':') {
        Some((prefix, local)) => (local, context.uri_for(prefix.as_bytes())),
        None => (name, context.uri_for(&[])),
    };
    (LEGACY_GRAPHICS.contains(&local) && bound.is_some_and(is_wml)).then_some(local)
}

/// Whether an event opens an `mc:AlternateContent` — the T6 element.
fn is_alternate_content(event: &Event<'_>, context: &PartContext) -> bool {
    let Event::Start(start) = event else {
        return false;
    };
    let raw = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    match raw.split_once(':') {
        Some((prefix, local)) => {
            local == "AlternateContent" && context.uri_for(prefix.as_bytes()) == Some(mce::MC_NS)
        }
        None => false,
    }
}

/// Rewrites a buffered `mc:AlternateContent`: resolved per
/// [`NormalizerOptions::mce`], and the resolved branch queued for the event loop.
///
/// Three outcomes and three report severities, and the pairing is the point:
///
/// | Resolution | Severity | Why |
/// |---|---|---|
/// | a `Choice` was taken | `Info` | a decision, nothing removed |
/// | the `mc:Fallback` was taken | `Ignorable` | a decision, nothing removed |
/// | nothing was understood and there is no fallback | `Lossy` | **a node is gone** |
/// | [`McePolicy::Report`] | `Ignorable` | nothing done, on purpose |
///
/// The third is the one that needs the report. Three corpus documents carry
/// `<mc:Choice Requires="wpsCustomData"/>` in `word/settings.xml` with no
/// fallback, and under MCE that block resolves to nothing — the element goes.
///
/// The `buffered` hand-off is not an optimisation, it is the correctness of
/// §10.7 step 4: the chosen content is ordinary markup that has not met T1–T5
/// yet, and six of the corpus's nine blocks have a `mc:Fallback` that is a
/// `w:pict` — so a branch written straight out would have had its picture
/// **dropped** by T7 instead of converted, which is the regression this queue
/// exists to prevent.
fn rewrite_alternate_content(
    writer: &mut Writer<Vec<u8>>,
    subtree: &[Event<'static>],
    context: &mut PartContext,
    report: &mut NormalizationReport,
    buffered: &mut std::collections::VecDeque<Event<'static>>,
) -> Result<()> {
    let location = context.location();
    let policy = context.mce;
    let branches = mce::branches(subtree, context);
    let (resolution, chosen) = mce::resolve(policy, &branches, context);

    let record = mce::record(&resolution, policy, &location);
    let removed = record.severity == Severity::Lossy;
    report.record_loss(record);
    if removed {
        report.count_reported_removal(1);
    }

    if matches!(resolution, mce::Resolution::Reported) {
        // Left exactly as it was, which means the raw events - the block has not
        // met T1 and so its namespace declarations are the producer's. A report
        // says so, and `McePolicy::Report` is the policy that asks for it.
        for event in subtree {
            writer
                .write_event(event.clone())
                .map_err(|error| xml_error(&context.part, error.to_string()))?;
        }
        return Ok(());
    }

    mce::queue(chosen, buffered);
    let _ = writer;
    Ok(())
}

/// Whether an event opens a subtree T7 judges whole.
fn is_legacy_graphics(event: &Event<'_>, context: &PartContext) -> bool {
    let Event::Start(start) = event else {
        return false;
    };
    let raw = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    legacy_graphics_local(&raw, context).is_some()
}

/// The qualified name a buffered subtree is reported under.
fn legacy_graphics_element(subtree: &[Event<'static>]) -> &'static str {
    let raw = match subtree.first() {
        Some(Event::Start(start) | Event::Empty(start)) => {
            String::from_utf8_lossy(start.name().as_ref()).into_owned()
        }
        _ => String::new(),
    };
    match raw.split_once(':').map(|(_, local)| local) {
        Some("object") => "w:object",
        _ => "w:pict",
    }
}

/// Where [`collect_subtree`] reads from.
///
/// Two sources because two elements are judged as a whole: the reader for the
/// part itself, and a queue for a resolved `mc:AlternateContent` branch whose
/// events are **already in hand** and must not be read past. Without the queue a
/// branch's subtree would be collected from the wrong stream, and the events after
/// it would be consumed twice.
struct EventSource<'a, 'r> {
    reader: &'a mut Reader<&'r [u8]>,
    buffered: &'a mut std::collections::VecDeque<Event<'static>>,
}

impl<'a, 'r> EventSource<'a, 'r> {
    fn new(
        reader: &'a mut Reader<&'r [u8]>,
        buffered: &'a mut std::collections::VecDeque<Event<'static>>,
    ) -> Self {
        Self { reader, buffered }
    }

    fn next(&mut self) -> Option<Event<'static>> {
        if let Some(event) = self.buffered.pop_front() {
            return Some(event);
        }
        self.reader.read_event().ok().map(into_owned_event)
    }
}

/// Reads a subtree into memory, starting from the event that opened it.
///
/// Returns the opening event followed by everything down to its matching end tag.
/// A document that ends inside the subtree — damaged input — returns what it has,
/// and the stages above then decline it, which is the safe direction: a truncated
/// subtree is not a picture anybody should draw and not a branch anybody should
/// resolve.
fn collect_subtree(source: &mut EventSource<'_, '_>, open: Event<'static>) -> Vec<Event<'static>> {
    let mut out = vec![open];
    let mut depth = 1usize;
    while depth > 0 {
        let Some(event) = source.next() else {
            break;
        };
        match &event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            Event::Eof => break,
            _ => {}
        }
        out.push(event);
    }
    out
}

/// Detaches an event from the reader's buffer.
fn into_owned_event(event: Event<'_>) -> Event<'static> {
    match event {
        Event::Start(start) => Event::Start(start.into_owned()),
        Event::End(end) => Event::End(end.into_owned()),
        Event::Empty(start) => Event::Empty(start.into_owned()),
        Event::Text(text) => Event::Text(text.into_owned()),
        Event::CData(data) => Event::CData(data.into_owned()),
        Event::Comment(comment) => Event::Comment(comment.into_owned()),
        Event::Decl(decl) => Event::Decl(decl.into_owned()),
        Event::PI(pi) => Event::PI(pi.into_owned()),
        Event::DocType(doc) => Event::DocType(doc.into_owned()),
        other => other.into_owned(),
    }
}

/// What T5 and T7 decided about a start tag.
enum Rewritten {
    /// The tag, rewritten.
    Keep(BytesStart<'static>),
    /// The whole subtree is to be dropped.
    Drop,
}

/// Per-part mutable state.
///
/// Visible to the [`vml`] module, which resolves a buffered `w:pict` subtree
/// against the prefix table. The field itself stays private: what is shared is
/// the one question `vml` asks, [`PartContext::uri_for`].
pub(crate) struct PartContext {
    part: PartId,
    prefixes: Vec<(Vec<u8>, String)>,
    /// Prefixes whose `xmlns:` declaration the pipeline did not emit.
    ///
    /// The binding is still in `prefixes` - `resolve` needs it to recognise a
    /// node in a namespace it removes - so this is the separate record that the
    /// declaration itself is gone. See [`map_namespace_declaration`].
    dropped_declarations: std::collections::BTreeSet<Vec<u8>>,
    /// Every prefix the part's bytes use, scanned before the streaming pass.
    ///
    /// `mc:Ignorable` names prefixes, and whether a name is dead depends on
    /// whether anything in the part uses it - a question about the whole part,
    /// asked at the root tag where the answer is not yet available. See
    /// [`used_prefixes`].
    used_prefixes: std::collections::BTreeSet<Vec<u8>>,
    line: u32,
    column: u32,
    offset: u64,
    /// The XML version the part declares.
    ///
    /// XML 1.1 changes entity handling, so an attribute value is unescaped
    /// under the version the document actually declares rather than under a
    /// version we assume.
    xml_version: quick_xml::XmlVersion,
    /// Depth of the subtree a removed element left open; everything read while
    /// this is non-zero is consumed and discarded.
    skip_depth: usize,
    /// The open WML elements, outermost first, as the output tree stands.
    ///
    /// T3 needs the **parent**, not the element: Strict renamed the edges of
    /// `CT_TblBorders`, `CT_TcBorders`, `CT_TblCellMar` and `CT_TcMar` to
    /// `start`/`end` and left `CT_PBdr`, `CT_PageBorders` and `CT_PageMar`
    /// spelling `left`/`right`, so `w:left` means two different things
    /// depending on where it sits and a global rename would zero every paragraph
    /// border, every page border and all four page margins.
    ///
    /// T4 needs the element, which is the last entry. An element is pushed only
    /// once the pipeline has decided to keep it, so a dropped subtree leaves the
    /// stack exactly as it found it and the matching `End` has nothing to pop.
    open_elements: Vec<String>,
    /// The parent of the element currently being rewritten, set by
    /// `rewrite_start` because that element is not on the stack yet.
    pending_parent: String,
    /// The element currently being rewritten, for the same reason.
    ///
    /// T4 is keyed by it because one `w:jc` carries two different simple types:
    /// `ST_Jc` inside `w:pPr` and `ST_JcTable` inside `w:tblPr`, and keying on
    /// the attribute alone - which is what `JC_VALUES` did - made every rule
    /// unreachable. A third reason, and the one that bites hardest: `left` is a
    /// *legal Strict value* of `ST_PTabAlignment` (`w:ptab/@w:alignment`), so a
    /// rule keyed without the element would destroy a `w:ptab` alignment.
    pending_element: String,
    /// A VML picture becomes a `w:drawing` whose payload lives in three
    /// namespaces the producer's part may never have declared, because a part
    /// that drew its pictures with VML has no reason to know `DrawingML`.
    ///
    /// Declared on the **root**, once, and only when the part really is about to
    /// emit one — see [`declares_a_vml_picture`]. An undeclared prefix is not a
    /// cosmetic problem: the part stops being well-formed, the **next read of it
    /// fails**, and so a normalization that reported itself successful has produced
    /// a document nobody can open. The failure lands one step downstream of here,
    /// which is the worst place for it to land.
    vml_picture_prefixes: Vec<(&'static str, &'static str)>,
    /// Whether the part's root element has been written yet.
    root_written: bool,
    /// Next `wp:docPr/@id` this part hands out, for a converted VML picture.
    doc_pr_id: u32,
    /// The markup-compatibility policy this write runs with.
    ///
    /// On the context rather than passed down through every signature: T6 is
    /// applied in three places (`rewrite_alternate_content`, the `mc:Ignorable`
    /// cleanup, and the namespace bookkeeping), and threading a policy through all
    /// three is how it became unreadable in the first place.
    pub(crate) mce: McePolicy,
    /// The invariant mode this write runs with.
    pub(crate) invariants: InvariantMode,
}

impl PartContext {
    /// Records a namespace binding, for a caller that knows the part's prefixes
    /// before it has read the part.
    ///
    /// Only the `vml` module's tests need it - they reach
    /// [`classify`](crate::normalize::vml::classify) without a package, and a
    /// classifier that resolves namespaces by prefix rather than by element name
    /// needs those bindings to answer anything. The real pipeline never needs it:
    /// `resolve` records them as it reads.
    #[cfg(test)]
    pub(crate) fn bind(&mut self, prefix: &str, uri: &str) {
        self.remember_prefix(prefix.as_bytes().to_vec(), uri.to_owned());
    }

    /// A context for one part, with the defaults.
    pub(crate) fn new(part: PartId) -> Self {
        Self {
            part,
            prefixes: Vec::new(),
            dropped_declarations: std::collections::BTreeSet::new(),
            used_prefixes: std::collections::BTreeSet::new(),
            line: 1,
            column: 1,
            offset: 0,
            xml_version: quick_xml::XmlVersion::Implicit1_0,
            skip_depth: 0,
            open_elements: Vec::new(),
            pending_parent: String::new(),
            pending_element: String::new(),
            doc_pr_id: 0,
            vml_picture_prefixes: Vec::new(),
            root_written: false,
            mce: McePolicy::default(),
            invariants: InvariantMode::default(),
        }
    }

    /// Records an element the pipeline has decided to keep.
    fn push_element(&mut self, local: &str) {
        self.open_elements.push(local.to_owned());
    }

    /// The next `wp:docPr/@id` for a converted VML picture.
    ///
    /// `@id` must be unique within the part and Word refuses a file that repeats
    /// one, so this is a per-part counter rather than anything derived from the
    /// shape. It counts only what **this** conversion emits: ids the part already
    /// carries are the producer's and the writer renumbers every `wp:docPr` it
    /// regenerates (`strict-ooxml-write/src/drawing.rs::document_properties`), so
    /// there is nothing to be consistent with here — only not to collide with
    /// ourselves.
    fn next_doc_pr_id(&mut self) -> u32 {
        self.doc_pr_id += 1;
        self.doc_pr_id
    }

    /// The prefixes a converted VML picture needs and this part does not declare.
    ///
    /// "Does not declare" means **the prefix is not bound**, and the test is
    /// `uri_for` rather than "does the input use it". A part may declare `xmlns:wp`
    /// and never use it — Word writes those declarations liberally — and checking
    /// usage would then add a *second* declaration for a prefix the root already
    /// binds, which is a duplicated attribute and a part that will not parse. The
    /// bug is worth naming because it is the third in this session that only shows
    /// up on the corpus and not on a hand-written fixture: every fixture that
    /// exercised this path also *used* the prefix.
    fn missing_vml_picture_prefixes(&self) -> Vec<(&'static str, &'static str)> {
        if self.vml_picture_prefixes.is_empty() {
            return Vec::new();
        }
        vml::REQUIRED_NAMESPACES
            .iter()
            .filter(|(prefix, _, _)| self.uri_for(prefix.as_bytes()).is_none())
            .map(|(prefix, strict, _)| (*prefix, *strict))
            .collect()
    }

    /// Forgets the innermost open element.
    fn pop_element(&mut self) {
        self.open_elements.pop();
    }

    /// The local name of the element that contains the one being rewritten.
    ///
    /// The element being rewritten is **not on the stack yet** — it is pushed
    /// only after the pipeline has agreed to keep it — so the parent is the last
    /// entry, not the one before it. Getting that off by one is not a subtle bug
    /// but a silent one: a `w:left` inside `w:tblBorders` looked itself up in
    /// `w:tblPr`, found no entry, and every one of the four direction-neutral
    //  renames was unreachable while the table said it was populated.
    fn parent_local(&self) -> Option<&str> {
        self.open_elements.last().map(String::as_str)
    }

    fn location(&self) -> SourceLocation {
        SourceLocation::new(self.part.clone(), self.line, self.column, self.offset)
    }

    /// Binds `prefix` to `uri`, replacing any earlier binding.
    ///
    /// Replacing rather than keeping the first binding matters: the start tag
    /// is read once to learn the original namespace and again to rewrite the
    /// declaration, and the second pass must win or every rewritten attribute
    /// ends up on a freshly minted prefix while the declaration still names
    /// the old one.
    fn remember_prefix(&mut self, prefix: Vec<u8>, uri: String) {
        if let Some(slot) = self.prefixes.iter_mut().find(|(known, _)| *known == prefix) {
            slot.1 = uri;
        } else {
            self.prefixes.push((prefix, uri));
        }
    }

    pub(crate) fn uri_for(&self, prefix: &[u8]) -> Option<&str> {
        self.prefixes
            .iter()
            .find(|(known, _)| known == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// Records that `prefix`'s `xmlns:` declaration is not being emitted.
    fn forget_declaration(&mut self, prefix: &[u8]) {
        self.dropped_declarations.insert(prefix.to_vec());
    }

    /// Whether `prefix`'s `xmlns:` declaration was dropped.
    fn declaration_dropped(&self, prefix: &[u8]) -> bool {
        self.dropped_declarations.contains(prefix)
    }

    /// Whether any name in the part uses `prefix`.
    fn prefix_used(&self, prefix: &[u8]) -> bool {
        self.used_prefixes.contains(prefix)
    }

    /// Joins a prefix and a local name, or returns the local name alone when
    /// the prefix is the default namespace.
    ///
    /// `<:Types>` is not a name, it is a syntax error, and it is what a
    /// `[Content_Types].xml` — whose root is in the default namespace —
    /// otherwise turns into.
    fn qualified_name(prefix: &str, local: &str) -> String {
        if prefix.is_empty() {
            local.to_owned()
        } else {
            format!("{prefix}:{local}")
        }
    }

    fn prefix_for(&mut self, uri: &str) -> String {
        if let Some((prefix, _)) = self.prefixes.iter().find(|(_, known)| known == uri) {
            return String::from_utf8_lossy(prefix).into_owned();
        }
        if uri.is_empty() {
            // The empty URI is "no namespace", and an unprefixed name is
            // exactly that. Minting a prefix for it would emit `<n0:tag>` with
            // no declaration, which is how a whole part stopped being XML.
            return String::new();
        }
        let prefix = format!("n{}", self.prefixes.len());
        self.remember_prefix(prefix.as_bytes().to_vec(), uri.to_owned());
        prefix
    }
}

/// Splits a tag name into `(local, uri)` using the remembered prefixes.
fn resolve(start: &BytesStart<'_>, context: &mut PartContext) -> (String, String) {
    let version = context.xml_version;
    let raw = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    // Namespace declarations have to be seen before the element that uses
    // them is resolved, so record them here rather than in the attribute loop.
    for attribute in start.attributes().flatten() {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        if let Some(uri) = key.strip_prefix("xmlns:") {
            if let Ok(value) = attribute.normalized_value(version) {
                context.remember_prefix(uri.as_bytes().to_vec(), value.into_owned());
            }
        } else if key == "xmlns" {
            if let Ok(value) = attribute.normalized_value(version) {
                context.remember_prefix(Vec::new(), value.into_owned());
            }
        }
    }
    if let Some((prefix, local)) = raw.split_once(':') {
        let uri = context
            .uri_for(prefix.as_bytes())
            .unwrap_or_default()
            .to_owned();
        (local.to_owned(), uri)
    } else {
        let uri = context.uri_for(&[]).unwrap_or_default().to_owned();
        (raw, uri)
    }
}

/// What T5 and T7 decided about an attribute.
///
/// A distinct type rather than `Option<Option<..>>`: the three outcomes —
/// keep with a value, keep with none, drop — are all meaningful, and nesting
/// `Option` to say so is exactly the kind of shape that gets misread later.
enum RewrittenAttribute {
    /// Keep, with a rewritten name and value.
    Keep(String, String),
    /// Drop: the namespace it declared is going away with its nodes.
    Drop,
}

/// Applies T1, T2, T3 and T4 to one attribute.
fn rewrite_attribute(
    attribute: &Attribute<'_>,
    context: &mut PartContext,
    report: &mut NormalizationReport,
) -> RewrittenAttribute {
    let version = context.xml_version;
    let location = context.location();
    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
    // The value is kept in its original escaped form and only decoded when a
    // stage has something to map. Re-encoding an untouched value would be a
    // fidelity loss in its own right — the producer's escaping is valid, and
    // `&quot;` written back escaped comes out `&amp;quot;`.

    if let Some(uri) = key.strip_prefix("xmlns:") {
        let Some(value) = attribute
            .normalized_value(version)
            .ok()
            .map(Cow::into_owned)
        else {
            return RewrittenAttribute::Drop;
        };
        return map_namespace_declaration(uri, &value, context, report)
            .map_or(RewrittenAttribute::Drop, |(key, value)| {
                RewrittenAttribute::Keep(key, value)
            });
    }
    if key == "xmlns" {
        let Some(value) = attribute
            .normalized_value(version)
            .ok()
            .map(Cow::into_owned)
        else {
            return RewrittenAttribute::Drop;
        };
        return map_namespace_declaration("", &value, context, report)
            .map_or(RewrittenAttribute::Drop, |(key, value)| {
                RewrittenAttribute::Keep(key, value)
            });
    }

    let local = key.rsplit(':').next().unwrap_or(&key).to_owned();
    // `xml:` is bound by the XML specification and is never declared in the
    // document. Treating it as an ordinary prefix loses the binding and turns
    // `xml:space="preserve"` into a name that is in no namespace at all.
    let is_reserved = key.starts_with("xml:");
    let uri = if is_reserved {
        String::new()
    } else {
        key.rsplit_once(':')
            .map_or_else(String::new, |(prefix, _)| {
                context
                    .uri_for(prefix.as_bytes())
                    .unwrap_or_default()
                    .to_owned()
            })
    };

    // ---- T1: a namespaced attribute in a Transitional namespace ---------
    let strict_uri = map_uri(&uri);
    if let Some(strict) = strict_uri.as_deref() {
        report.record_mapping("T1.namespace", &uri, strict);
    }
    let effective_uri = strict_uri.unwrap_or_else(|| uri.clone());

    // ---- T5: an attribute in a producer extension ----------------------
    // The *attribute* half of what `is_ignorable_extension` does for elements.
    // `w14:paraId` and `w14:textId` were carried straight through while every
    // `w14` element was dropped, which is the half no schema message names:
    // ECMA-376 Part 1 §2.1 clause (ii) defines conformance on the post-MCE part,
    // and an MCE processor removes an ignorable namespace's attributes too, so
    // the attribute was never a schema defect of ours - it was still markup we
    // claimed to have normalized away. One survived in `word/comments.xml`
    // (census `TZ-13`), and one is what the whole corpus holds, so this is the
    // entire remaining surface of `XS-17` on the pass-through path.
    //
    // Counted in the stage total rather than as a removal record: an attribute
    // is not a node, and `verify_no_silent_loss` counts nodes against reported
    // removals - booking it as one would put the two counters out of step.
    if !is_reserved && is_ignorable_extension(&uri) {
        report.record("T5.extension-attribute", 1);
        return RewrittenAttribute::Drop;
    }

    // ---- T3: attribute rename ----------------------------------------
    // The key is the element that **carries** the attribute - `w:ind` - not its
    // parent, and that is the same distinction the element rename draws the
    // other way round: for `w:left` as an ELEMENT the key is the container
    // (`w:tblBorders`), for `w:left` as an ATTRIBUTE it is the holder
    // (`w:ind`). Both are needed, and both are needed because Strict renamed
    // the two halves independently: `CT_Ind` has `@w:start` and no `@w:left`,
    // while `CT_PBdr` still has a `w:left` child and `CT_PageMar` still has an
    // `@w:left`. A `w:pgMar/@w:left` that followed the `w:ind` rule would zero
    // all four page margins on every page.
    let renamed = is_wml(&uri)
        .then(|| tables::rename_attribute(&context.pending_element, &local))
        .flatten();
    let new_local = renamed.unwrap_or(local.as_str());
    if renamed.is_some() {
        report.record("T3.rename", 1);
    }

    // ---- T6: MCE bookkeeping that has nothing left to name --------------
    // `mc:Ignorable="w14 w15"` on a part whose `w14` and `w15` nodes are all gone
    // is a declaration of nothing, and it is also what ADR-0014 counts as ours:
    // Strict conformance is defined on the post-MCE part, so shipping the
    // declaration and relying on the consumer to act on it is shipping the part
    // un-normalized. Thirty of them survived in the corpus (audit §3). Cleaning
    // them is part of T6 rather than a separate table entry because it is
    // MCE's own bookkeeping: the attribute names prefixes, and whether a prefix
    // still resolves is a question only the prefix table can answer.
    if uri == MC_NAMESPACE {
        let decoded = attribute
            .normalized_value(version)
            .ok()
            .map(Cow::into_owned)
            .unwrap_or_default();
        let Some(kept) = clean_mce_attribute(context, &local, &decoded) else {
            report.record("T6.mce", 1);
            return RewrittenAttribute::Drop;
        };
        if kept != decoded {
            report.record("T6.mce", 1);
        }
        return RewrittenAttribute::Keep(PartContext::qualified_name("mc", &local), kept);
    }

    let out_key = if is_reserved {
        key.clone()
    } else if uri.is_empty() {
        new_local.to_owned()
    } else {
        PartContext::qualified_name(&context.prefix_for(&effective_uri), new_local)
    };

    // The value is decoded once, here. `BytesStart::push_attribute` escapes on
    // the way out, so a value must never arrive already escaped: passing the
    // producer's `&quot;` through untouched comes back out `&amp;quot;`.
    let decoded = attribute
        .normalized_value(version)
        .ok()
        .map(Cow::into_owned);
    let Some(decoded) = decoded else {
        // A valueless attribute does not exist in the WML vocabulary, and
        // carrying it through would only produce a name the schema rejects.
        return RewrittenAttribute::Drop;
    };
    let element = context.pending_element.clone();
    let parent = context.pending_parent.clone();
    let value = mapped_value(
        &element,
        &parent,
        new_local,
        &effective_uri,
        &decoded,
        &location,
        report,
    )
    .unwrap_or(decoded);
    RewrittenAttribute::Keep(out_key, value)
}

/// Drops the prefixes an MCE bookkeeping attribute names but that the part no
/// longer declares, and the attribute itself once none of them resolve.
///
/// `mc:Ignorable`, `mc:PreserveElements` and `mc:PreserveAttributes` all take a
/// whitespace-separated list of **prefixes**, and the list's whole job is to name
/// namespaces a consumer must skip. Once the normalizer has removed those nodes
/// and their `xmlns:` declarations, every token in the list is dead weight, and
/// a dead `mc:Ignorable` is worse than none: it is a declaration that a Strict
/// consumer will act on by removing nodes we already removed, and ADR-0014
/// counts shipping it as shipping the part un-normalized.
///
/// Returns `None` when the attribute has nothing left to say, which is the
/// signal to drop it. Anything that is not one of the three attributes is
/// returned untouched — this is MCE's vocabulary, not ours to extend.
fn clean_mce_attribute(context: &PartContext, local: &str, value: &str) -> Option<String> {
    if !matches!(
        local,
        "Ignorable" | "PreserveElements" | "PreserveAttributes"
    ) {
        return Some(value.to_owned());
    }
    let mut kept: Vec<&str> = Vec::new();
    for token in value.split_ascii_whitespace() {
        // Three ways a token is dead, and all three have to be checked: the
        // prefix was never declared (a producer naming a prefix it does not use),
        // its declaration was dropped because the pipeline removed that
        // namespace wholesale, or it is bound to a namespace that is gone even
        // though the declaration is still on its way out.
        let Some(uri) = context.uri_for(token.as_bytes()) else {
            continue;
        };
        if context.declaration_dropped(token.as_bytes())
            || !context.prefix_used(token.as_bytes())
            || tables::is_ignorable_extension(uri)
            || VML_NAMESPACES.contains(&uri)
        {
            continue;
        }
        kept.push(token);
    }
    (!kept.is_empty()).then(|| kept.join(" "))
}

/// Decodes a `w:tblLook` bit mask, or records that it could not be decoded.
///
/// The two outcomes are different defects and get different records: a decoded
/// mask is a transformation, and an undecodable one is a loss, because the
/// attribute stays in the part and the part will fail the schema on it. A loss
/// the report does not name is the failure mode this whole module is written
/// against, so an unreadable value is named rather than carried quietly.
///
/// `None` also covers "there is no `@w:val` at all", which is a Strict `w:tblLook`
/// arriving in a Transitional part and needs no record.
/// Adds the three `CT_PageMar` attributes Strict declares `use="required"`.
///
/// `w:gutter`, `w:header` and `w:footer`, none of which eight corpus documents
/// write - usually all three at once, because the producer is a tool that emits a
/// page margin as four numbers and stops.
///
/// A missing attribute has no value to map, which is why this is not a
/// `mapped_value` rule and why it is the only one of the four value forms that
/// lives at the element.
///
/// The defaults are Word's: no gutter, and half an inch of header and footer,
/// which is 720 twips. `s:ST_TwipsMeasure` is
/// `union(ST_UnsignedDecimalNumber, ST_PositiveUniversalMeasure)`, so a bare number
/// is conformant and no unit is written - adding `twip` would be the mirror of
/// the mistake this project made once already, in the other direction.
fn synthesize_page_margin(
    context: &mut PartContext,
    start: &BytesStart<'_>,
    element_uri: &str,
    buffer: &mut BytesStart<'static>,
    report: &mut NormalizationReport,
) {
    let present: Vec<Vec<u8>> = start
        .attributes()
        .flatten()
        .map(|attribute| attribute.key.as_ref().to_vec())
        .collect();
    for (attribute, default) in [("gutter", "0"), ("header", "720"), ("footer", "720")] {
        if present
            .iter()
            .any(|key| key.ends_with(attribute.as_bytes()))
        {
            continue;
        }
        let prefix = context.prefix_for(element_uri);
        let key = PartContext::qualified_name(&prefix, attribute);
        buffer.push_attribute((key.as_str(), default));
        report.record_mapping("T4.pageMargin", attribute, default);
    }
}

fn decode_and_report_tbl_look(
    start: &BytesStart<'_>,
    context: &PartContext,
    report: &mut NormalizationReport,
    location: &SourceLocation,
) -> Option<TblLook> {
    if let Some(decoded) = decode_tbl_look(start, context) {
        report.record("T4.tblLook", 1);
        return Some(decoded);
    }
    if start
        .attributes()
        .flatten()
        .any(|attribute| attribute.key.as_ref().ends_with(b"val"))
    {
        report.record_loss(LossRecord {
            transform_id: "T4.tblLook",
            feature_id: "w:tblLook/@w:val".to_owned(),
            reason: "w:tblLook/@w:val is not a hexadecimal bit mask, so the six Strict \
                     attributes could not be derived; the attribute is carried unchanged"
                .to_owned(),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
    }
    None
}

/// What a Transitional `w:tblLook/@w:val` bit mask says, decoded.
struct TblLook {
    /// The six flags, in the order `tables::TBL_LOOK_BITS` gives them.
    flags: [bool; 6],
}

/// Decodes the Transitional `w:tblLook/@w:val` bit mask into Strict's six
/// `s:ST_OnOff` attributes.
///
/// Strict's `CT_TblLook` declares `firstRow`, `lastRow`, `firstColumn`,
/// `lastColumn`, `noHBand` and `noVBand`, all optional, and **no `w:val`**: the
/// bit mask is a Transitional spelling of those six flags, and the conversion is
/// arithmetic — each bit is documented and named after the attribute it becomes.
/// The mask is read as hexadecimal, which is what every producer writes
/// (`04A0`, `0020`, `01E0`).
///
/// **Only the attributes the producer did not already write are added.** Word
/// emits `w:val="04A0"` *and* `w:firstRow="1" w:lastRow="0" ...` in the same tag,
/// for consumers that understand either spelling; six of this document's tables
/// do exactly that, and pushing six more attributes would have produced
/// `<w:tblLook>` with a repeated `w:firstRow`, which is a hard XML error rather
/// than a tolerated one. The producer's own values are left alone — `1` and `0`
/// are `xsd:boolean`, so they are already conformant — and `@w:val` goes
/// regardless, because Strict's `CT_TblLook` has no such attribute.
///
/// A mask that is not hexadecimal is left in place and reported: a mask we
/// cannot read is a value we would otherwise zero, and a zeroed table look is a
/// page that lost its banding.
fn decode_tbl_look(start: &BytesStart<'_>, context: &PartContext) -> Option<TblLook> {
    let value = start.attributes().flatten().find_map(|attribute| {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        if key.rsplit(':').next() == Some("val") {
            attribute
                .normalized_value(context.xml_version)
                .ok()
                .map(Cow::into_owned)
        } else {
            None
        }
    })?;
    let trimmed = value.trim();
    let bits = u32::from_str_radix(trimmed.trim_start_matches("0x"), 16).ok()?;
    let mut flags = [false; 6];
    for (slot, (bit, _)) in flags.iter_mut().zip(tables::TBL_LOOK_BITS) {
        *slot = bits & bit != 0;
    }
    Some(TblLook { flags })
}

/// Applies T4 and T2 to an attribute value, returning `None` when neither
/// applies and the original must therefore be kept.
fn mapped_value(
    element: &str,
    parent: &str,
    local: &str,
    uri: &str,
    value: &str,
    location: &SourceLocation,
    report: &mut NormalizationReport,
) -> Option<String> {
    // ---- T4: an enumerated value Strict spells differently -------------
    if is_wml(uri) {
        if let Some(mapped) = tables::map_value(element, local, value) {
            report.record_mapping("T4.value", value, mapped);
            return Some(mapped.to_owned());
        }
        // ---- T4: a value whose TYPE Strict spells differently -----------
        //
        // Four forms, and all four were absent from this function, which is why
        // every one of them was closed in the WRITER rather than here: the parts
        // that carry them are regenerated, so the census measured the writer and
        // the normalizer was never asked. They are here because pass-through
        // parts are not regenerated - a table in a header this writer does not
        // model comes out byte-identical and non-Strict.
        //
        // The parent is part of the key for one of them: `w:left` inside
        // `w:tblCellMar` is `ST_MeasurementOrPercent` and `w:left` inside `w:ind`
        // is `ST_TwipsMeasure`, which a bare number satisfies. Matching on the
        // element name alone would corrupt every indentation in the document.
        if local == "w" && tables::is_measure_carrier(element, parent) {
            if let Some(mapped) = tables::twips_to_universal(value) {
                report.record_mapping("T4.measure", value, &mapped);
                return Some(mapped);
            }
        }
        // `w:rPr/w:w/@w:val` is `ST_TextScale`; `w:zoom/@w:percent` is
        // `ST_DecimalNumberOrPercent`. Same shape, different types on different
        // elements, so they stay separate rules.
        if local == "val" && element == "w" && parent == "rPr" {
            if let Some(mapped) = tables::text_scale_percent(value) {
                report.record_mapping("T4.textScale", value, &mapped);
                return Some(mapped);
            }
        }
        if local == "percent" && element == "zoom" {
            if let Some(mapped) = tables::decimal_or_percent(value) {
                report.record_mapping("T4.percent", value, &mapped);
                return Some(mapped);
            }
        }
    }
    // ---- AUD-20: repair legacy purl OPC relationship types -------------
    if let Some(repaired) = repair_legacy_package_uri(value) {
        report.record_mapping("T1.namespace-repair", value, repaired);
        return Some(repaired.to_owned());
    }
    // ---- T2: a relationship-type URI ------------------------------------
    map_rel_or_content_type(value, location, report)
}

/// Maps a legacy purl OPC URI (from earlier project versions) to the standard
/// ECMA-376 Part 2 form (AUD-20 / ADR-0015).
fn repair_legacy_package_uri(uri: &str) -> Option<&'static str> {
    match uri {
        LEGACY_PURL_PACKAGE_REL_NS => Some(PACKAGE_REL_NS),
        LEGACY_PURL_CORE_PROPERTIES_NS => Some(PACKAGE_CORE_PROPERTIES_NS),
        LEGACY_PURL_CORE_PROPERTIES_REL => Some(PACKAGE_CORE_PROPERTIES_REL),
        LEGACY_PURL_THUMBNAIL_REL => Some(PACKAGE_THUMBNAIL_REL),
        _ => None,
    }
}

/// Maps a Transitional namespace URI to its Strict form.
fn map_uri(uri: &str) -> Option<String> {
    let entry = NamespaceRegistry::global().lookup(uri)?;
    let strict = entry.strict?;
    (strict != uri).then(|| strict.to_owned())
}

/// Rewrites a namespace declaration, or drops it when the namespace is gone.
///
/// A namespace whose content the pipeline removes wholesale - VML (T7) and the
/// ignorable word extensions (T5) - has its declaration dropped with its nodes:
/// leaving `xmlns:w14` behind advertises a namespace nothing uses, which is the
/// debt `strict-ooxml-write/tests/strict_conformance.rs` already refuses in the
/// parts this writer regenerates.
///
/// **The binding is still remembered when the declaration is not emitted.** The
/// prefix table is what `resolve` uses to decide whether an element *is* in a
/// removed namespace, and a binding forgotten here would make a later
/// `<w14:glow>` resolve to no namespace at all - which is neither removed nor
/// legal, and would be written out with an undeclared prefix. What is recorded
/// instead is that the *declaration* went, which is what
/// [`clean_mce_attribute`] needs to know.
fn map_namespace_declaration(
    prefix: &str,
    value: &str,
    context: &mut PartContext,
    report: &mut NormalizationReport,
) -> Option<(String, String)> {
    if let Some(strict) = map_uri(value) {
        report.record_mapping("T1.namespace", value, &strict);
        context.remember_prefix(prefix.as_bytes().to_vec(), strict.clone());
        let key = if prefix.is_empty() {
            "xmlns".to_owned()
        } else {
            format!("xmlns:{prefix}")
        };
        return Some((key, strict));
    }
    // AUD-20: earlier writers emitted non-standard purl OPC URIs; repair them
    // to the ECMA-376 Part 2 vocabulary without treating them as Transitional.
    if let Some(repaired) = repair_legacy_package_uri(value) {
        report.record_mapping("T1.namespace-repair", value, repaired);
        context.remember_prefix(prefix.as_bytes().to_vec(), repaired.to_owned());
        let key = if prefix.is_empty() {
            "xmlns".to_owned()
        } else {
            format!("xmlns:{prefix}")
        };
        return Some((key, repaired.to_owned()));
    }
    if VML_NAMESPACES.contains(&value) || tables::is_ignorable_extension(value) {
        context.remember_prefix(prefix.as_bytes().to_vec(), value.to_owned());
        context.forget_declaration(prefix.as_bytes());
        return None;
    }
    // An unregistered namespace is left exactly as it is: a producer's private
    // extension is not ours to rewrite, and `mc:Ignorable` is the mechanism
    // for telling consumers to skip it.
    context.remember_prefix(prefix.as_bytes().to_vec(), value.to_owned());
    let key = if prefix.is_empty() {
        "xmlns".to_owned()
    } else {
        format!("xmlns:{prefix}")
    };
    Some((key, value.to_owned()))
}

/// Maps a relationship-type URI to its Strict form (AUD-22, T2).
///
/// Reads [`REL_TYPES`] by **exact URI**, never by rewriting a prefix: the bug
/// this replaces rewrote the Transitional *base* and kept the Transitional
/// *suffix*, which turned `extended-properties` into `extended-properties`
/// again instead of `extendedProperties` — the base changed, the one place the
/// two columns actually differ in spelling did not.
///
/// Three outcomes, and only the first changes the value:
///
/// * the URI is a table row whose `strict` differs from it → rewritten,
///   `T2.reltype` records the mapping;
/// * the URI is a table row with `strict: None` (`stylesWithEffects`, the only
///   such row) → kept, `T2.reltype-no-strict` records that Strict has nothing
///   to rewrite it to rather than inventing a URI;
/// * the URI is **not** a table row but sits under the Transitional
///   officeDocument base anyway → kept, `T2.reltype-unknown` names the URI so
///   the gap is visible instead of silently passed through.
///
/// Anything else (a URI already Strict, an OPC package type, a content type —
/// the function's old name notwithstanding, content types do not differ
/// between the families; AUD-26) returns `None` unchanged and unreported.
fn map_rel_or_content_type(
    value: &str,
    location: &SourceLocation,
    report: &mut NormalizationReport,
) -> Option<String> {
    if let Some(entry) = REL_TYPES.iter().find(|entry| entry.transitional == value) {
        return match entry.strict {
            Some(strict) if strict != value => {
                report.record_mapping("T2.reltype", value, strict);
                Some(strict.to_owned())
            }
            // Transitional and Strict happen to be the same URI (the two OPC
            // package rows): nothing changed, so nothing is reported.
            Some(_) => None,
            None => {
                report.record_loss(LossRecord {
                    transform_id: "T2.reltype-no-strict",
                    feature_id: value.to_owned(),
                    reason: format!(
                        "{value} has no Strict relationship type at all (Strict declares no \
                         equivalent part); the Transitional URI is kept rather than invented"
                    ),
                    severity: Severity::Lossy,
                    locations: vec![location.clone()],
                });
                None
            }
        };
    }
    if value.starts_with(TRANSITIONAL_OFFICE_BASE) {
        report.record_loss(LossRecord {
            transform_id: "T2.reltype-unknown",
            feature_id: value.to_owned(),
            reason: format!(
                "{value} is under the Transitional officeDocument relationships base but is \
                 not a relationship type this project's table recognizes; left unchanged"
            ),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
    }
    None
}

fn is_wml(uri: &str) -> bool {
    uri.ends_with("wordprocessingml/main")
}

fn qualify(uri: &str, local: &str) -> String {
    if uri.is_empty() {
        local.to_owned()
    } else {
        format!("{uri}#{local}")
    }
}

fn write_start(writer: &mut Writer<Vec<u8>>, start: &BytesStart<'_>) -> std::io::Result<()> {
    writer.write_event(Event::Start(start.to_owned()))
}

fn write_empty(writer: &mut Writer<Vec<u8>>, start: &BytesStart<'_>) -> std::io::Result<()> {
    writer.write_event(Event::Empty(start.to_owned()))
}

fn write_end(writer: &mut Writer<Vec<u8>>, end: &BytesEnd<'_>) -> std::io::Result<()> {
    writer.write_event(Event::End(end.to_owned()))
}

fn write_passthrough(writer: &mut Writer<Vec<u8>>, event: &Event<'_>) -> std::io::Result<()> {
    writer.write_event(event.to_owned())
}

fn xml_error(part: &PartId, detail: String) -> StrictError {
    StrictError::InvalidXml {
        location: SourceLocation::new(part.clone(), 1, 1, 0),
        detail,
    }
}

impl crate::normalize::RawNormalizer for TransitionalNormalizer {
    fn normalize_part<'a>(&self, part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        self.normalize(part, bytes)
    }

    fn note_unexpected_main_content_type(&self, part: &PartId, content_type: &str) {
        // AUD-26: under Normalize/Permissive the open continues; the report
        // names the MIME so the operator can see why the package is odd.
        let mut report = self.report.lock().expect("normalizer report lock");
        report.record_mapping(
            "T2.content-type",
            content_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        );
        report.record_loss(LossRecord {
            transform_id: "T2.content-type",
            feature_id: "contentTypes".to_owned(),
            reason: format!(
                "main part {} declares content type {content_type}; expected a \
                 WordprocessingML document/template (macro-enabled) main type",
                part.as_str()
            ),
            severity: Severity::Ignorable,
            locations: vec![SourceLocation::new(part.clone(), 1, 1, 0)],
        });
    }
}

/// Whether a part carries any Transitional signal at all.
///
/// The check is textual on purpose: it runs before the part is parsed, so an
/// already-Strict document is never re-serialized (criterion SC-1) and a hostile
/// input is not parsed just to be rejected.
///
/// **The MCE namespace is not a Transitional signal**, and treating it as one
/// cost a whole document its SC-1 property. `word/webSettings.xml` in a Strict
/// package carries
/// `xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"` and
/// nothing Transitional at all, so this function said "normalize me" and T6 then
/// rewrote the part — a Strict part being rewritten by a Transitional stage, which
/// is exactly what SC-1 asserts never happens. MCE is ISO/IEC 29500-3 and its
/// namespace URI is the same in both families; a `mc:Ignorable` is markup every
/// Strict document is entitled to carry.
///
/// So the signal is a Transitional family URI, decided per occurrence rather than
/// per part: a Transitional `document.xml` carries both the officeDocument
/// family and the MCE namespace, and one document must not be classified by the
/// needle that happens to come first.
#[must_use]
pub fn part_needs_normalization(bytes: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"schemas.openxmlformats.org";
    const MCE: &[u8] = b"schemas.openxmlformats.org/markup-compatibility/2006";
    // AUD-20: legacy purl OPC URIs need a pass even when no Transitional
    // openxmlformats marker is present.
    const LEGACY_PURL_PACKAGE: &[u8] = b"purl.oclc.org/ooxml/package";
    if memchr::memmem::find(bytes, LEGACY_PURL_PACKAGE).is_some() {
        return true;
    }
    let mut from = 0usize;
    while let Some(found) = memchr::memmem::find(&bytes[from..], NEEDLE) {
        let at = from + found;
        if !bytes[at..].starts_with(MCE) {
            return true;
        }
        from = at + NEEDLE.len();
    }
    false
}

/// The prefixes a part's bytes actually use, for the MCE bookkeeping cleanup.
///
/// `mc:Ignorable` names prefixes, and whether a name is dead depends on whether
/// anything in the part uses it - a question about the WHOLE part, asked at the
/// root tag where the answer is not yet available. So it is answered before the
/// streaming pass rather than during it: one linear scan of bytes we are about to
/// parse anyway, gated by [`part_needs_normalization`] so an already-Strict part
/// never pays it.
///
/// Textual and deliberately over-inclusive: a `QName` is read as "an identifier
/// immediately followed by `:`" anywhere in the byte stream, so it sees element
/// names, attribute names and `xmlns:` declarations alike. A prefix it misses
/// would be dropped from `mc:Ignorable` while still in use, which would be a real
/// defect; one it invents merely keeps a dead token alive, which is the state the
/// project was in before.
fn used_prefixes(bytes: &[u8]) -> std::collections::BTreeSet<Vec<u8>> {
    fn is_name_start(byte: u8) -> bool {
        byte.is_ascii_alphabetic() || byte == b'_'
    }
    fn is_name_char(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
    }
    let mut used = std::collections::BTreeSet::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if !is_name_start(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_name_char(bytes[at]) {
            at += 1;
        }
        // `a:b` is a qualified name; `a:b` inside text, `http://x` and `a::b` are
        // not. The last two cases are the ones a bare "identifier then colon"
        // test would get wrong, and both are common in a part's text content.
        let qualified = at + 1 < bytes.len()
            && bytes[at] == b':'
            && !matches!(bytes.get(at + 1), Some(b':' | b'/'))
            && !bytes[at + 1].is_ascii_whitespace();
        if qualified {
            used.insert(bytes[start..at].to_vec());
            at += 1;
        }
    }
    used
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{
        map_rel_or_content_type, part_needs_normalization, repair_legacy_package_uri,
        InvariantMode, McePolicy, NormalizerOptions, TransitionalNormalizer,
    };
    use crate::error::SourceLocation;
    use crate::normalize::report::{NormalizationReport, Severity};
    use crate::part::PartId;

    fn part() -> PartId {
        PartId::new("/word/document.xml")
    }

    const TRANSITIONAL: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn a_transitional_part_is_rewritten() {
        let normalizer = TransitionalNormalizer::new();
        let output = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap();
        let Cow::Owned(_) = output else {
            panic!("a Transitional part must produce new bytes")
        };
        let text = String::from_utf8(output.to_vec()).unwrap();
        assert!(
            text.contains("http://purl.oclc.org/ooxml/wordprocessingml/main"),
            "{text}"
        );
        assert!(!text.contains("schemas.openxmlformats.org"), "{text}");
    }

    #[test]
    fn a_removed_element_takes_its_whole_subtree_with_it() {
        // The first draft of this pass set a "drop this" flag and then wrote
        // the element anyway, so nothing was ever removed while the report
        // claimed otherwise. This asserts the subtree is actually gone.
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:compat><w:doNotExpandShiftReturn/></w:compat><w:useFELayout/><w:p><w:r><w:t>kept</w:t></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        // `w:compat` **survives**: Strict declares it as `CT_Settings`
        // position 89 with eight legal children, so removing it was a loss with
        // no basis, and this test is what kept the bug in place. What must go is
        // `w:useFELayout`, which Strict's `CT_Compat` does not declare.
        assert!(
            text.contains("w:compat"),
            "w:compat is declared by Strict and must survive: {text}"
        );
        assert!(
            text.contains("doNotExpandShiftReturn"),
            "a legal CT_Compat child must survive too: {text}"
        );
        assert!(!text.contains("useFELayout"), "and the other: {text}");
        assert!(
            text.contains("kept"),
            "sibling content must survive: {text}"
        );
    }

    #[test]
    fn vml_is_dropped_and_recorded_as_lossy() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="s1"><v:fill color="red"/></v:shape></w:pict><w:t>text</w:t></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("v:shape"), "VML must be gone: {text}");
        assert!(
            !text.contains("w:pict"),
            "its Transitional container too: {text}"
        );
        assert!(text.contains("text"), "the run's text must survive: {text}");
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 1, "the VML removal is a real loss");
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// The acceptance measurement for queue item 2 (`SESSION-HANDOFF-2026-10-01.md` §4).
    ///
    /// Item 2 had **two** causes, and the handoff only found one of them. The
    /// first was real: `JC_VALUES` was keyed by attribute alone while the table
    /// needs `(element, attribute)`, because one `w:jc` is `ST_Jc` inside
    /// `w:pPr` and `ST_JcTable` inside `w:tblPr` — so `map_value("val", "left")`
    /// was compared against an entry keyed `"jc"` and never matched. Fixing the
    /// key is what made the rule reachable.
    ///
    /// The "second barrier" of the handoff does not exist. The probe that was
    /// reported as never firing did fire; the acceptance assertion was wrong in
    /// its *expected string* (`w:val="start" />` with a space, where `quick_xml`
    /// writes `w:val="start"/>`), and a substring mismatch was read as a dead
    /// rule. Which is the §6 lesson in a smaller dress: the measurement was
    /// right, the conclusion drawn from it was not.
    #[test]
    fn t4_reaches_st_jc_values_and_writes_the_strict_spelling() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="right"/></w:pPr></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"<w:jc w:val="start"/>"#),
            "left must become start: {text}"
        );
        assert!(
            text.contains(r#"<w:jc w:val="end"/>"#),
            "right must become end: {text}"
        );
        assert!(
            text.contains(r#"<w:jc w:val="center"/>"#),
            "center is already Strict and must be untouched: {text}"
        );
        assert_eq!(
            normalizer.report().lossy_count(),
            0,
            "a value the schema names differently is a mapping, not a loss"
        );
        // The report has to *name* the mapping, or the gate above would still
        // pass on a normalizer that rewrote nothing and lost nothing.
        let applied = normalizer.report().applied();
        let t4 = applied
            .iter()
            .find(|record| record.id == "T4.value")
            .expect("T4 must fire on `left`/`right`");
        assert_eq!(t4.count, 2, "left and right, once each: {applied:?}");
    }

    /// Queue item 10, and the only one on the list that gives a document back
    /// **something visible**: the corpus's pictures used to be deleted along with
    /// the VML that carried them.
    ///
    /// The shape below is the corpus's own, taken from
    /// `DOCX_Watermark_691b4503e0.docx`: a watermark is `position:absolute` with a
    /// negative `z-index` and a centred position relative to the page. Asserting
    /// the *anchor* rather than an inline image is the point — the audit said
    /// "1:1 into `wp:inline`", and following that literally would move the
    /// watermark into the text flow and centre it on the line.
    #[test]
    fn a_vml_picture_becomes_a_drawingml_picture_that_keeps_its_relationship() {
        let normalizer = TransitionalNormalizer::new();
        let source = r##"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">
<w:p><w:r><w:pict><v:shapetype id="_x0000_t75" coordsize="21600,21600" o:spt="75">
<v:stroke joinstyle="miter"/></v:shapetype>
<v:shape id="WordPictureWatermark1" alt="" type="#_x0000_t75"
 style="position:absolute;width:468.0pt;height:468.0pt;rotation:0;z-index:-503316481;mso-position-horizontal-relative:margin;mso-position-horizontal:center;mso-position-vertical-relative:margin;mso-position-vertical:center;">
<v:imagedata blacklevel="22938f" cropbottom="0f" r:id="rId7" o:title="image3.png"/></v:shape></w:pict></w:r></w:p></w:hdr>"##;
        let output = normalizer
            .normalize(&PartId::new("/word/header1.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();

        assert!(!text.contains("v:shape"), "the VML is gone: {text}");
        assert!(
            !text.contains("v:imagedata"),
            "and its image reference: {text}"
        );
        assert!(
            !text.contains("w:pict"),
            "Strict declares no w:pict: {text}"
        );
        assert!(
            text.contains("<wp:anchor"),
            "an absolutely positioned shape is an anchor: {text}"
        );
        assert!(
            text.contains(r#"<wp:positionH relativeFrom="page"><wp:align>center</wp:align>"#),
            "centred on the page: {text}"
        );
        assert!(
            text.contains(r#"<wp:positionV relativeFrom="page"><wp:align>center</wp:align>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<wp:extent cx="5943600" cy="5943600"/>"#),
            "468 pt is 5 943 600 EMU at 12 700 EMU to a point: {text}"
        );
        // The single most important assertion: **the same relationship**. The image
        // part behind it is the same part with the same relationship, which is what
        // makes this affordable at the byte seam — no relationship surgery, and the
        // pass-through carries the bytes.
        assert!(
            text.contains(r#"<a:blip r:embed="rId7"/>"#),
            "the relationship id is reused unchanged: {text}"
        );
        assert!(
            text.contains("http://purl.oclc.org/ooxml/drawingml/picture"),
            "and the payload namespace is Strict: {text}"
        );
        let report = normalizer.report();
        assert_eq!(
            report.lossy_count(),
            0,
            "a converted picture is a mapping, not a loss: {report}"
        );
        assert!(
            report
                .applied()
                .iter()
                .any(|record| record.id == "T7.vml-shape" && record.count == 1),
            "and it is counted as a transformation: {report}"
        );
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// The declarations the conversion's prefixes need, on a part that has **no**
    /// `DrawingML` in it at all.
    ///
    /// This is the case the corpus does not have: `DOCX_Watermark_691b4503e0.docx`
    /// also holds real `w:drawing` pictures, so its root already declares `wp`,
    /// `a` and `pic` and the conversion borrows them. A part whose only picture is
    /// VML has no such declarations, and emitting `wp:anchor` without them
    /// produces a part that is **not well-formed** — which fails on the *next* read
    /// of the part, one step downstream of the code that broke it, which is the
    /// worst place for a failure to land.
    #[test]
    fn a_part_that_only_had_vml_gets_the_drawingml_prefixes_it_now_needs() {
        let normalizer = TransitionalNormalizer::new();
        let source = r##"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml">
<w:p><w:r><w:pict><v:shape id="s1" style="width:100pt;height:50pt" type="#_x0000_t75">
<v:imagedata r:id="rId1"/></v:shape></w:pict></w:r></w:p></w:hdr>"##;
        let output = normalizer
            .normalize(&PartId::new("/word/header1.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        for (prefix, strict) in [
            ("a", "http://purl.oclc.org/ooxml/drawingml/main"),
            (
                "wp",
                "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
            ),
            ("pic", "http://purl.oclc.org/ooxml/drawingml/picture"),
        ] {
            assert!(
                text.contains(&format!(r#"xmlns:{prefix}="{strict}""#)),
                "{prefix} must be declared on the root: {text}"
            );
        }
        // And the part has to parse afterwards, which is the property that matters
        // and the reason this is a test rather than an inspection of the string.
        let reparsed = TransitionalNormalizer::new()
            .normalize(&PartId::new("/word/header1.xml"), text.as_bytes())
            .expect("the converted part must be well-formed XML");
        let again = String::from_utf8(reparsed.into_owned()).unwrap();
        assert!(again.contains("<wp:inline"), "{again}");
        // SC-3, and the reason the idempotence question has an answer here: a second
        // pass must not add the declarations a second time.
        assert_eq!(
            again.matches("xmlns:wp=").count(),
            1,
            "one declaration, not two: {again}"
        );
    }

    /// A part with no `w:pict` gets no `DrawingML` declarations, because a
    /// declaration nobody uses is the debt
    /// `strict-ooxml-write/tests/strict_conformance.rs` already refuses in the
    /// parts this writer regenerates.
    #[test]
    fn a_part_without_vml_gains_no_drawingml_prefixes() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:p><w:r><w:t>x</w:t></w:r></w:p></w:hdr>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/header1.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        for prefix in ["xmlns:a=", "xmlns:wp=", "xmlns:pic="] {
            assert!(!text.contains(prefix), "{prefix} is unused: {text}");
        }
    }

    /// Options for a fixture that is nothing but a VML shape.
    ///
    /// **The default expansion bound is calibrated on documents, and these are not
    /// documents.** `wp:anchor` plus `wps:wsp` plus `pic:spPr` is roughly a
    /// kilobyte of markup where `<v:shape style="..."><v:imagedata r:id="..."/></v:shape>`
    /// was 120 bytes, so a 455-byte part that is *only* a shape grows several fold and
    /// trips a bound no real document comes near — the worst
    /// `word/document.xml` growth over the 58-document Transitional corpus is one
    /// per cent. The fixture therefore asks for the allowance explicitly, rather than
    /// the default being raised for a shape that is a document in name only; the knob
    /// exists for exactly this.
    fn shape_fixture_options() -> NormalizerOptions {
        NormalizerOptions {
            max_expansion_bytes: 32 * 1024,
            ..NormalizerOptions::default()
        }
    }

    /// A VML text box becomes a `wps:wsp` with its **content intact**, because a
    /// frame with the text left out is a blank rectangle drawn where a paragraph of
    /// text was.
    ///
    /// The content goes through T1–T5 like everything else — it is written through
    /// the same `rewrite_event` — and asserting the text is *still there* is what
    /// holds that. A version that emitted the frame and dropped the content would
    /// produce a perfectly valid `wps:wsp` and lose a paragraph.
    #[test]
    fn a_vml_text_box_becomes_a_shape_that_still_has_its_text() {
        let normalizer = TransitionalNormalizer::with_options(shape_fixture_options());
        let source = r##"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="tb" type="#_x0000_t202" style="width:200pt;height:60pt">
<v:textbox><w:txbxContent><w:p><w:r><w:t>inside the box</w:t></w:r></w:p></w:txbxContent></v:textbox>
</v:shape></w:pict></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("v:shape"), "and no VML: {text}");
        assert!(text.contains("<wps:wsp"), "it is a shape: {text}");
        assert!(text.contains("<wps:txbx><w:txbxContent>"), "{text}");
        assert!(
            text.contains("inside the box"),
            "and the text is still in it: {text}"
        );
        assert!(
            text.contains("<wps:bodyPr"),
            "bodyPr is required after the optional txbx: {text}"
        );
        // Exactly one `w:txbxContent`: the conversion writes its own and a second
        // wrapper would nest them, which is valid XML and a text box whose every
        // paragraph is one level too deep.
        assert_eq!(
            text.matches("<w:txbxContent>").count(),
            1,
            "one wrapper, not two: {text}"
        );
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 0, "nothing was lost: {report}");
    }

    /// A shape carrying **both** a `v:imagedata` and a `v:textbox` is an OLE
    /// object, and the picture wins.
    ///
    /// That is the corpus's own shape (`Интегралы (2).docx`, three of them), and
    /// the ranking is deliberate: what a reader saw before the conversion was the
    /// `v:imagedata` **preview raster**, and the audit §12 is explicit that the
    /// preview is the only recoverable part of an OLE object. Converting the frame
    /// instead would draw an empty box where an equation was.
    #[test]
    fn a_shape_with_both_an_image_and_a_textbox_is_treated_as_an_ole_preview() {
        let normalizer = TransitionalNormalizer::with_options(shape_fixture_options());
        let source = r##"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"
 xmlns:o="urn:schemas-microsoft-com:office:office">
<w:body><w:p><w:r><w:object><v:shape id="ob" type="#_x0000_t202" style="width:75.4pt;height:45.5pt">
<v:textbox><w:txbxContent><w:p><w:r><w:t>the editable text</w:t></w:r></w:p></w:txbxContent></v:textbox>
<v:imagedata r:id="rId5"/><o:OLEObject Type="Embed" ProgID="Equation.3" r:id="rId6"/>
</v:shape></w:object></w:r></w:p></w:body></w:document>"##;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"<a:blip r:embed="rId5"/>"#),
            "the preview raster is what the reader saw: {text}"
        );
        assert!(
            !text.contains("OLEObject"),
            "and the object is gone: {text}"
        );
        assert!(!text.contains("rId6"), "with its relationship: {text}");
        let report = normalizer.report();
        assert_eq!(
            report.lossy_count(),
            1,
            "exactly one loss, and it is the object rather than the preview: {report}"
        );
        assert!(report.to_string().contains("executable object"), "{report}");
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// A freeform `v:path` keeps its frame and loses its outline, **by name**.
    ///
    /// Both freeform shapes in the corpus are
    /// `m665994,l,,,7199r665994,l665994,xe`: an arc whose arguments are partly
    /// absent. `a:custGeom` has no arc command, so writing it as `m`/`l` would draw
    /// a straight line where the producer drew a curve — a shape in the right place
    /// drawn wrong. The frame is kept because that is right, and the geometry is
    /// dropped because that is honest.
    #[test]
    fn a_freeform_path_keeps_its_frame_and_names_the_geometry_it_dropped() {
        let normalizer = TransitionalNormalizer::with_options(shape_fixture_options());
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="ff" style="width:52.45pt;height:.6pt"
 path="m665994,l,,,7199r665994,l665994,xe"/></w:pict></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(text.contains("<wps:wsp"), "the frame is kept: {text}");
        assert!(
            !text.contains("custGeom"),
            "and no guessed path is drawn: {text}"
        );
        let report = normalizer.report();
        assert_eq!(
            report.lossy_count(),
            1,
            "one named loss, for the geometry: {report}"
        );
        assert!(report.to_string().contains("no equivalent for"), "{report}");
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// `w10:wrap/@type` and `margin-left`/`margin-top` — the two spellings a
    /// floating VML shape uses for its wrap and its position.
    ///
    /// Five of the ten floating objects in the corpus use one and five the other.
    /// Reading only the `mso-` pair gives the other five a **zero** `wp:posOffset`
    /// and puts them in the corner of the page, and **the census cannot see it**:
    /// a shape in the wrong place is not a schema violation, it is a page that looks
    /// wrong. Hence this test, which holds both spellings rather than the one that
    /// happened to be measured.
    #[test]
    fn a_floating_shape_keeps_its_wrap_and_its_offset() {
        let normalizer = TransitionalNormalizer::with_options(shape_fixture_options());
        let offset = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w10="urn:schemas-microsoft-com:office:word">
<w:body><w:p><w:r><w:pict><v:rect id="a" style="position:absolute;margin-left:68.05pt;margin-top:9.95pt;width:52.45pt;height:.6pt;z-index:-251659264"><w10:wrap type="topAndBottom" anchorx="page"/></v:rect></w:pict></w:r></w:p></w:body></w:document>"#;
        let text = String::from_utf8(
            normalizer
                .normalize(&part(), offset.as_bytes())
                .unwrap()
                .into_owned(),
        )
        .unwrap();
        assert!(text.contains("<wp:wrapTopBottom"), "{text}");
        // 68.05 pt and 9.95 pt, at 12 700 EMU to a point.
        assert!(
            text.contains("<wp:posOffset>864235</wp:posOffset>"),
            "the margin-left offset, not a zero: {text}"
        );
        assert!(
            text.contains("<wp:posOffset>126365</wp:posOffset>"),
            "{text}"
        );
        assert!(
            text.contains(r#"behindDoc="1""#),
            "a negative z-index: {text}"
        );

        let aligned = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w10="urn:schemas-microsoft-com:office:word">
<w:body><w:p><w:r><w:pict><v:rect id="b" style="position:absolute;left:0;margin-left:406.05pt;margin-top:20.4pt;width:83.9pt;height:106.5pt;z-index:-251659264"
 fillcolor="white" strokecolor="black"><v:textbox><w:txbxContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:txbxContent></v:textbox><w10:wrap type="tight"/></v:rect></w:pict></w:r></w:p></w:body></w:document>"#;
        let text = String::from_utf8(
            normalizer
                .normalize(&part(), aligned.as_bytes())
                .unwrap()
                .into_owned(),
        )
        .unwrap();
        assert!(text.contains("<wp:wrapTight"), "{text}");
        // `CT_WrapTight` requires a `wp:wrapPolygon`, and `CT_WrapPath` wants one
        // `wp:start` and at least two `wp:lineTo`, so an empty polygon is not
        // conformant either - the frame's own rectangle goes there.
        assert!(text.contains("<wp:wrapPolygon>"), "{text}");
        assert_eq!(text.matches("<wp:lineTo").count(), 3, "{text}");
        // No `mso-position-*` at all, so the offsets are what places it: 406.05 pt
        // and 20.4 pt, at 12 700 EMU to a point.
        assert!(
            text.contains("<wp:posOffset>5156835</wp:posOffset>"),
            "the margin-left offset, not a zero: {text}"
        );
        assert!(
            text.contains("<wp:posOffset>259080</wp:posOffset>"),
            "{text}"
        );
    }

    /// An inline VML shape — `position` absent or `static` — is an inline image,
    /// and the anchor path is not for it.
    #[test]
    fn an_inline_vml_shape_becomes_an_inline_picture() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="s1" style="width:72pt;height:36pt">
<v:imagedata r:id="rId3"/></v:shape></w:pict></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(text.contains("<wp:inline"), "{text}");
        assert!(!text.contains("<wp:anchor"), "not an anchor: {text}");
        assert!(
            text.contains(r#"<wp:extent cx="914400" cy="457200"/>"#),
            "72 pt by 36 pt: {text}"
        );
    }

    /// An OLE object keeps its **preview raster** and loses only the object.
    ///
    /// This is the case the census measurement found and the audit §12 predicted:
    /// `w:object` is declared by Strict, so it is not a removal — but everything
    /// inside it is, `o:OLEObject` and the VML shape alike, and an empty
    /// `<w:object/>` is a schema-valid element with nothing in it. What a reader saw
    /// before was the `v:imagedata` preview, and the audit is explicit that this is
    /// the only part of an OLE object that is recoverable: "OLE is an executable
    /// object; Strict has no substitute for it".
    ///
    /// So the two claims are made **separately**, and that is the point of the test:
    /// the picture is a mapping (`T7.vml-shape`, no loss) and the object is a named
    /// loss (`T7.ole`). One record for both would let a reader conclude the OLE object
    /// survived, which is the opposite of what happened.
    #[test]
    fn an_ole_object_keeps_its_preview_raster_and_names_what_is_gone() {
        let normalizer = TransitionalNormalizer::new();
        let source = r##"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"
 xmlns:w10="urn:schemas-microsoft-com:office:word">
<w:body><w:p><w:r><w:object w:dxaOrig="1440" w:dyaOrig="1440"><v:shape id="_x0000_i1025" o:spt="75"
 type="#_x0000_t75" style="height:45.5pt;width:75.4pt;" o:ole="t" filled="f" stroked="f" coordsize="21600,21600">
<v:imagedata r:id="rId5" o:title=""/><o:OLEObject Type="Embed" ProgID="Equation.3" ShapeID="_x0000_i1025"
 DrawAspect="Content" ObjectID="_1234567890" r:id="rId6"/></v:shape></w:object></w:r></w:p></w:body></w:document>"##;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            !text.contains("w:object"),
            "an empty w:object is dead markup: {text}"
        );
        assert!(
            !text.contains("OLEObject"),
            "OLE has no Strict substitute: {text}"
        );
        assert!(
            !text.contains("rId6"),
            "nor the object relationship: {text}"
        );
        assert!(
            text.contains(r#"<a:blip r:embed="rId5"/>"#),
            "the preview raster is what the reader saw, so it stays: {text}"
        );
        let report = normalizer.report();
        assert_eq!(
            report.lossy_count(),
            1,
            "exactly one loss, and it is the object rather than the picture: {report}"
        );
        let rendered = report.to_string();
        assert!(
            rendered.contains("executable object Strict has no substitute"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("picture; Strict declares no w:pict"),
            "the VML branch must not also fire for an object: {rendered}"
        );
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// A shape with no usable size still gets a `wp:extent`, because the element is
    /// `use="required"`, and the zero is **named**: a picture that draws at no
    /// size is a real defect and the report is the only place a reader learns of
    /// it.
    #[test]
    fn an_unsized_vml_picture_gets_a_named_zero_extent() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:v="urn:schemas-microsoft-com:vml">
<w:body><w:p><w:r><w:pict><v:shape id="s1"><v:imagedata r:id="rId3"/></v:shape></w:pict></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(text.contains(r#"<wp:extent cx="0" cy="0"/>"#), "{text}");
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 1, "named: {report}");
        assert!(report.to_string().contains("no usable width"), "{report}");
    }

    /// Queue item 7, and the half of it that is easy to get backwards.
    ///
    /// `w:left` is an edge in four containers and a page margin in three, and
    /// Strict renamed the first and kept the second. A global rename — which is
    /// what `TZ-STRICT-OOXML-RUST.md` §10.4 asks for — would produce a document
    /// that **validates as Strict** and lays out with no paragraph borders, no
    /// page borders and no page margins at all: a silent, total loss of layout on
    /// every page, and a much worse defect than the invalid document it replaced.
    ///
    /// So the positive and the negative are asserted in one test on purpose. A
    /// test of the renames alone would pass with an empty table, which is the
    /// state the project was in until this date; a test of the negatives alone
    /// would pass with a table that renames nothing.
    #[test]
    fn the_direction_neutral_rename_follows_the_container_and_not_the_name() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:tbl><w:tblPr><w:tblBorders><w:left w:val="single"/><w:right w:val="single"/></w:tblBorders>
<w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr>
<w:tr><w:tc><w:tcPr><w:tcBorders><w:left w:val="single"/><w:right w:val="single"/></w:tcBorders>
<w:tcMar><w:left w:w="0" w:type="dxa"/><w:right w:w="0" w:type="dxa"/></w:tcMar></w:tcPr>
<w:p><w:pPr><w:pBdr><w:left w:val="single"/><w:right w:val="single"/></w:pBdr>
<w:ind w:left="720" w:right="720"/><w:jc w:val="left"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:sectPr><w:pgMar w:top="1134" w:right="850" w:bottom="1134" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/>
<w:pgBorders><w:left w:val="single"/><w:right w:val="single"/></w:pgBorders></w:sectPr>
</w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();

        // ---- renamed: the four containers Strict made direction-neutral ----
        for container in ["tblBorders", "tcBorders", "tblCellMar", "tcMar"] {
            assert!(
                text.contains(&format!("<w:{container}><w:start ")),
                "{container} declares start/end in Strict: {text}"
            );
            assert!(
                !text.contains(&format!("<w:{container}><w:left ")),
                "{container} must not keep left: {text}"
            );
        }
        assert!(
            text.contains(r#"<w:ind w:start="720" w:end="720"/>"#),
            "{text}"
        );

        // ---- kept: the three containers Strict did not touch ----------------
        assert!(
            text.contains(r#"<w:pBdr><w:left w:val="single"/><w:right w:val="single"/></w:pBdr>"#),
            "CT_PBdr still declares left/right and zeroing it loses every paragraph border: {text}"
        );
        assert!(
            text.contains(
                r#"<w:pgBorders><w:left w:val="single"/><w:right w:val="single"/></w:pgBorders>"#
            ),
            "CT_PageBorders still declares left/right: {text}"
        );
        assert!(
            text.contains(r#"<w:pgMar w:top="1134" w:right="850" w:bottom="1134" w:left="1701""#),
            "CT_PageMar still declares @w:left/@w:right: {text}"
        );
    }

    /// The same rename, in the one place it is a *value* rather than a name, and
    /// the case that keeps it from being a blanket rule: `left` is a **legal
    /// Strict value** of `ST_PTabAlignment`, so a `w:ptab` alignment that followed
    /// the `w:tab` rule would be written outside its own simple type.
    #[test]
    fn a_left_that_is_legal_strict_is_not_renamed() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:ptab w:alignment="left" w:relativeTo="margin" w:pos="720"/>
<w:tab w:val="left" w:pos="720"/><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"<w:ptab w:alignment="left""#),
            "ST_PTabAlignment still has left/center/right: {text}"
        );
        assert!(
            text.contains(r#"<w:tab w:val="start""#),
            "ST_TabJc does not: {text}"
        );
    }

    /// `w:tblLook`'s bit mask, and the case that made the first attempt produce
    /// invalid XML: Word writes `w:val` **and** the six booleans in the same tag,
    /// for consumers that understand either spelling. Six of this corpus's tables
    /// do exactly that, and six more attributes would have been a repeated
    /// `w:firstRow` — a hard XML error, not a tolerated one.
    #[test]
    fn a_table_look_mask_becomes_six_booleans_and_nothing_else() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:tbl><w:tblPr>
<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>
</w:tblPr><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid>
<w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:tbl><w:tblPr><w:tblLook w:val="04A0"/></w:tblPr><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid>
<w:tr><w:tc><w:p><w:r><w:t>y</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            !text.contains("w:val=\"04A0\""),
            "Strict's CT_TblLook declares no @w:val: {text}"
        );
        // 04A0 = firstRow|firstColumn|noVBand, so this is what the mask says.
        for expected in [
            r#"w:firstRow="true""#,
            r#"w:lastRow="false""#,
            r#"w:firstColumn="true""#,
            r#"w:lastColumn="false""#,
            r#"w:noHBand="false""#,
            r#"w:noVBand="true""#,
        ] {
            assert!(text.contains(expected), "{expected} is missing: {text}");
        }
        assert_eq!(
            text.matches("w:firstRow").count(),
            2,
            "once per table, not twice: {text}"
        );
        // The producer's own `1`/`0` are `xsd:boolean` and are left as they are.
        assert!(
            text.contains(r#"w:lastRow="0""#),
            "the producer's value stays: {text}"
        );
    }

    /// A mask that is not hexadecimal is left in place **and named**. The part
    /// still fails the schema on it, and a value we could not convert is exactly
    /// the loss a reader of the report has to be told about — guessing would
    /// zero a table look, and a zeroed table look is a page that lost its
    /// banding.
    #[test]
    fn an_unreadable_table_look_mask_is_carried_and_recorded() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:tbl><w:tblPr><w:tblLook w:val="firstRowOnly"/></w:tblPr>
<w:tblGrid><w:gridCol w:w="1"/></w:tblGrid>
<w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("firstRowOnly"),
            "carried, not invented: {text}"
        );
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 1, "and named: {report}");
        assert!(
            report.to_string().contains("not a hexadecimal bit mask"),
            "{report}"
        );
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// Queue item 3, the pass-through half of `XS-17`.
    ///
    /// `w14` **elements** were dropped and `w14` **attributes** were carried
    /// through, so `word/comments.xml` kept three `w14:paraId`s. Nothing caught
    /// it: an attribute in a namespace the ECMA set does not declare is removed by
    /// an MCE processor *before* conformance is defined (ADR-0014), so the schema
    /// gate files it in the extension basket and the `xsd_gate.py` `extension`
    /// counter reads zero either. Census `TZ-13` is what sees it, by scanning the
    /// written part directly and counting both halves.
    #[test]
    fn a_producer_extension_attribute_goes_with_its_elements() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">
<w:comment w:id="1" w:author="a"><w:p w14:paraId="1A2B3C4D" w14:textId="77777777"><w:r><w:t>x</w:t></w:r></w:p></w:comment>
<w:p><w:r><w14:glow w14:rad="63500"/></w:r></w:p></w:comments>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("paraId"), "the attribute must go: {text}");
        assert!(!text.contains("textId"), "and the other: {text}");
        assert!(
            !text.contains("w14:glow"),
            "the element already went: {text}"
        );
        assert!(text.contains("<w:t>x</w:t>"), "content survives: {text}");
        let applied = normalizer.report().applied();
        assert!(
            applied
                .iter()
                .any(|record| record.id == "T5.extension-attribute" && record.count == 2),
            "both attributes are counted, not one: {applied:?}"
        );
    }

    /// Queue item 3, and `ADR-0014`'s own condition.
    ///
    /// A `mc:Ignorable` whose prefixes no longer resolve is a declaration of
    /// nothing. Shipping it is shipping the part un-normalized: we claim the part
    /// is Strict, Strict conformance is defined on the post-MCE part, and this
    /// is markup whose entire job is to tell a consumer to do the removing we
    /// already did. Thirty survived in the corpus (audit §3) - and the gate could
    /// not see them, because an attribute MCE itself would remove is by definition
    /// not a schema violation.
    #[test]
    fn an_mc_ignorable_naming_nothing_left_is_dropped() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">
<w:comment w:id="1" w:author="a"><w:p><w:r><w14:glow w14:rad="63500"/></w:r></w:p></w:comment></w:comments>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("Ignorable"), "the dead list must go: {text}");
        // The declaration it named goes with it: leaving `xmlns:w14` behind
        // advertises a namespace nothing uses.
        assert!(!text.contains("w14"), "and its declaration: {text}");
        assert!(
            normalizer
                .report()
                .applied()
                .iter()
                .any(|record| record.id == "T6.mce" && record.count > 0),
            "T6 must be counted: {}",
            normalizer.report()
        );
    }

    /// The same attribute, with a prefix that is still in use. This is the case a
    /// blanket "delete `mc:Ignorable`" would break, and it is the reason the
    /// cleanup asks whether the namespace is *used* rather than whether it is
    /// known: the drawing extensions (`wps`, `wpg`, `wp14`) carry shapes the
    /// renderer does implement and are deliberately **not** dropped (see
    /// `IGNORABLE_EXTENSION_NAMESPACES`), so a part that uses one must keep naming
    /// it as ignorable or a consumer will stop skipping it.
    #[test]
    fn an_mc_ignorable_that_still_names_a_used_namespace_is_kept() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14 wps wp14">
<w:body><w:p><w:r><w:t>x</w:t></w:r><wps:wsp wps:txbx="1"/></w:p></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"mc:Ignorable="wps""#),
            "wps is used, so the list keeps it: {text}"
        );
        assert!(
            !text.contains("w14"),
            "and drops w14 with its nodes: {text}"
        );
        assert!(
            !text.contains("wp14"),
            "and a prefix never declared: {text}"
        );
    }

    #[test]
    fn an_ignorable_removal_does_not_count_as_a_cost() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:shapeDefaults/><w:zoom w:percent="100"/></w:settings>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("shapeDefaults"), "{text}");
        assert!(
            text.contains("w:zoom"),
            "unrelated settings must survive: {text}"
        );
        assert_eq!(normalizer.report().lossy_count(), 0);
        assert_eq!(
            normalizer.report().severity_counts()[&Severity::Ignorable],
            1
        );
    }

    #[test]
    fn attribute_values_survive_escaping() {
        // Decoding a value and writing it back raw turns `&amp;` into a bare
        // `&`, and the part stops being XML. Every document whose
        // `[Content_Types].xml` has an escaped character failed to reparse
        // before this was fixed.
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:t xml:space="preserve">a &amp; b &lt; c</w:t></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("&amp;"),
            "the ampersand must stay escaped: {text}"
        );
        assert!(
            text.contains("&lt;"),
            "the less-than must stay escaped: {text}"
        );
        // The output has to parse again, which is the property that matters.
        let second = TransitionalNormalizer::new();
        second.normalize(&part(), text.as_bytes()).expect("reparse");
    }

    #[test]
    fn an_escaped_quote_in_a_value_does_not_end_the_attribute() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:t xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" w:val="say &quot;hi&quot;"/>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(text.contains("&quot;"), "{text}");
        TransitionalNormalizer::new()
            .normalize(&part(), text.as_bytes())
            .expect("reparse");
    }

    #[test]
    fn a_strict_part_is_borrowed_unchanged_and_the_report_is_empty() {
        let normalizer = TransitionalNormalizer::new();
        let strict = TRANSITIONAL.replace(
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            "http://purl.oclc.org/ooxml/wordprocessingml/main",
        );
        let output = normalizer.normalize(&part(), strict.as_bytes()).unwrap();
        assert!(
            matches!(output, Cow::Borrowed(_)),
            "Strict input must not be touched"
        );
        assert!(normalizer.report().is_noop(), "SC-1: no-op on Strict input");
    }

    #[test]
    fn normalization_is_idempotent_over_bytes() {
        let normalizer = TransitionalNormalizer::new();
        let once = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        let twice = normalizer.normalize(&part(), &once).unwrap();
        assert_eq!(
            &*twice,
            &once[..],
            "SC-3: normalize(normalize(x)) == normalize(x)"
        );
    }

    #[test]
    fn two_runs_agree_byte_for_byte_and_report_for_report() {
        let first = TransitionalNormalizer::new();
        let second = TransitionalNormalizer::new();
        let a = first
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        let b = second
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap()
            .into_owned();
        assert_eq!(a, b, "SC-2: determinism");
        assert_eq!(
            first.report().to_string(),
            second.report().to_string(),
            "SC-2: report"
        );
    }

    #[test]
    fn namespaces_that_do_not_exist_in_strict_are_left_alone() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:zz="urn:private:ext"><w:body><zz:thing/></w:body></w:document>"#;
        let output = normalizer.normalize(&part(), source.as_bytes()).unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("urn:private:ext"),
            "a private extension must survive: {text}"
        );
    }

    #[test]
    fn relationship_types_are_mapped() {
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/_rels/.rels"), 1, 1, 0);
        let to = map_rel_or_content_type(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
            &location,
            &mut report,
        )
        .expect("a Transitional relationship type maps");
        assert_eq!(
            to,
            "http://purl.oclc.org/ooxml/officeDocument/relationships/styles"
        );
        assert_eq!(report.applied()[0].id, "T2.reltype");
    }

    #[test]
    fn strict_relationship_types_are_left_alone() {
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/_rels/.rels"), 1, 1, 0);
        assert!(map_rel_or_content_type(
            "http://purl.oclc.org/ooxml/officeDocument/relationships/styles",
            &location,
            &mut report,
        )
        .is_none());
        assert!(report.is_noop());
    }

    #[test]
    fn an_unrecognized_transitional_office_relationship_is_not_rewritten_but_is_reported() {
        // AUD-22: a Transitional URI under the officeDocument base with no
        // table row must not be rewritten (there is nothing to rewrite it to)
        // and must not be silently dropped either.
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/_rels/.rels"), 1, 1, 0);
        let value = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/zzz";
        assert!(map_rel_or_content_type(value, &location, &mut report).is_none());
        let losses = report.losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].transform_id, "T2.reltype-unknown");
        assert!(losses[0].reason.contains(value), "{}", losses[0].reason);
        assert_eq!(losses[0].severity, Severity::Lossy);
    }

    #[test]
    fn styles_with_effects_has_no_strict_form_and_is_reported_not_rewritten() {
        // AUD-22: `stylesWithEffects` is in the table with `strict: None`; it
        // must be kept exactly and reported as `T2.reltype-no-strict`, never
        // rewritten into an invented Strict URI.
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/word/_rels/document.xml.rels"), 1, 1, 0);
        let value = "http://schemas.microsoft.com/office/2007/relationships/stylesWithEffects";
        assert!(map_rel_or_content_type(value, &location, &mut report).is_none());
        let losses = report.losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].transform_id, "T2.reltype-no-strict");
        assert_eq!(losses[0].severity, Severity::Lossy);
    }

    #[test]
    fn extended_and_custom_properties_rewrite_to_their_strict_spelling_exactly() {
        // AUD-22: the bug this table replaces rewrote the base and kept the
        // suffix, producing `extended-properties` (unchanged) instead of
        // `extendedProperties`.
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/_rels/.rels"), 1, 1, 0);
        let to = map_rel_or_content_type(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties",
            &location,
            &mut report,
        )
        .expect("extended-properties maps");
        assert_eq!(
            to,
            "http://purl.oclc.org/ooxml/officeDocument/relationships/extendedProperties"
        );
        let to = map_rel_or_content_type(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties",
            &location,
            &mut report,
        )
        .expect("custom-properties maps");
        assert_eq!(
            to,
            "http://purl.oclc.org/ooxml/officeDocument/relationships/customProperties"
        );
    }

    #[test]
    fn legacy_purl_package_uris_are_repaired() {
        // AUD-20: packages written before ADR-0015 carried non-standard purl
        // OPC URIs; the normalizer puts the ECMA-376 Part 2 spelling back.
        let normalizer = TransitionalNormalizer::new();
        let source = concat!(
            r#"<Relationships xmlns="http://purl.oclc.org/ooxml/package/relationships">"#,
            r#"<Relationship Id="rId1" "#,
            r#"Type="http://purl.oclc.org/ooxml/package/relationships/metadata/core-properties" "#,
            r#"Target="docProps/core.xml"/>"#,
            r#"</Relationships>"#,
        );
        let output = normalizer
            .normalize(&PartId::new("/_rels/.rels"), source.as_bytes())
            .expect("normalize");
        let text = String::from_utf8(output.into_owned()).expect("utf8");
        assert!(
            text.contains("http://schemas.openxmlformats.org/package/2006/relationships\""),
            "xmlns must be the standard OPC URI: {text}"
        );
        assert!(
            text.contains(
                "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
            ),
            "Type must be the standard OPC URI: {text}"
        );
        assert!(
            !text.contains("purl.oclc.org/ooxml/package"),
            "legacy purl OPC URIs must be gone: {text}"
        );
        let report = normalizer.report().to_string();
        assert!(
            report.contains("T1.namespace-repair"),
            "repair must be recorded: {report}"
        );
    }

    #[test]
    fn standard_opc_package_uris_are_not_rewritten_to_purl() {
        // AUD-20: the openxmlformats package vocabulary is already correct.
        let mut report = NormalizationReport::new();
        let location = SourceLocation::new(PartId::new("/_rels/.rels"), 1, 1, 0);
        assert!(map_rel_or_content_type(
            "http://schemas.openxmlformats.org/package/2006/relationships",
            &location,
            &mut report,
        )
        .is_none());
        assert!(report.is_noop());
        assert_eq!(
            repair_legacy_package_uri(
                "http://schemas.openxmlformats.org/package/2006/relationships"
            ),
            None
        );
    }

    #[test]
    fn a_non_xml_part_is_untouched() {
        let normalizer = TransitionalNormalizer::new();
        let image = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let output = normalizer
            .normalize(&PartId::new("/word/media/image1.png"), &image)
            .unwrap();
        assert!(matches!(output, Cow::Borrowed(_)));
    }

    #[test]
    fn the_transitional_signal_check_is_textual_and_narrow() {
        assert!(part_needs_normalization(TRANSITIONAL.as_bytes()));
        assert!(!part_needs_normalization(b"<w:document/>"));
    }

    /// Queue item 14, T6. `McePolicy` was declared, documented and default-valued
    /// from the start and **read nowhere** — the public API promised a behaviour
    /// that did not exist, which is the same category of defect as the XSD harness
    /// that caught `XMLSchemaParseError` and passed.
    ///
    /// The corpus's shapes make the first assertion the interesting one: six of the
    /// nine blocks are `<mc:Choice Requires="wps">` holding a **real shape**, and
    /// `wps` is not in ECMA-376. Taking the `mc:Fallback` for content this project
    /// reads and writes back would be a downgrade the producer never asked for, so
    /// "understood" has to mean "we handle this namespace", not "the standard has
    /// it" — see [`tables::REPRODUCED_EXTENSION_NAMESPACES`].
    #[test]
    fn an_alternate_content_block_takes_the_choice_we_understand() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
 mc:Ignorable="wps">
<w:body><w:p><w:r><mc:AlternateContent>
<mc:Choice Requires="wps"><wps:wsp wps:txbx="1"><wps:cNvSpPr/></wps:wsp></mc:Choice>
<mc:Fallback><w:t>the plain fallback</w:t></mc:Fallback>
</mc:AlternateContent></w:r></w:p></w:body></w:document>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/document.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("wps:wsp"),
            "the shape is what we can render, so it is what we take: {text}"
        );
        assert!(
            !text.contains("the plain fallback"),
            "and the fallback is not: {text}"
        );
        assert!(!text.contains("AlternateContent"), "{text}");
        let report = normalizer.report();
        assert_eq!(
            report.lossy_count(),
            0,
            "a choice taken removes nothing: {report}"
        );
        let rendered = report.to_string();
        assert!(
            rendered.contains("resolved to the first mc:Choice"),
            "and the decision is written down: {rendered}"
        );
    }

    /// A namespace we **drop** is not one we understand, and taking its choice
    /// would mean selecting content the next stage deletes.
    #[test]
    fn a_choice_requiring_a_namespace_we_drop_is_not_taken() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
 xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml">
<w:body><w:p><w:r><mc:AlternateContent>
<mc:Choice Requires="w14"><w14:shadow w14:blurRad="0"/></mc:Choice>
<mc:Fallback><w:t>the plain fallback</w:t></mc:Fallback>
</mc:AlternateContent></w:r></w:p></w:body></w:document>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/document.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains("the plain fallback"),
            "w14 is dropped, so its choice is not understood: {text}"
        );
        assert!(!text.contains("w14:shadow"), "{text}");
        let report = normalizer.report();
        assert!(
            report.to_string().contains("resolved to its mc:Fallback"),
            "{report}"
        );
    }

    /// The five value forms Strict spells differently, in one pass-through part.
    ///
    /// **Pass-through is the only place these are testable end to end.** Every one
    /// of them occurs in the corpus only inside parts the writer REGENERATES -
    /// `document.xml`, `styles.xml`, `numbering.xml`, `fontTable.xml` - where the
    /// writer's own spelling is conformant and the normalizer is never asked. So a
    /// corpus gate measured the writer for all four of them and reported a clean
    /// zero on rules that did not exist. `word/header2.xml` is not regenerated, and
    /// a table in it comes out of this pipeline byte-for-byte whatever this function
    /// does, which is what makes it the fixture.
    #[test]
    fn the_five_value_forms_are_converted_in_a_pass_through_part() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <w:tbl>
  <w:tblPr>
   <w:tblW w:w="2200" w:type="dxa"/>
   <w:tblInd w:w="17663" w:type="dxa"/>
   <w:tblCellMar>
    <w:top w:w="0" w:type="dxa"/>
    <w:start w:w="108" w:type="dxa"/>
    <w:bottom w:w="0" w:type="dxa"/>
    <w:end w:w="108" w:type="dxa"/>
   </w:tblCellMar>
  </w:tblPr>
  <w:tblGrid><w:gridCol w:w="2200"/></w:tblGrid>
 </w:tbl>
 <w:p><w:pPr><w:spacing w:before="240"/></w:pPr><w:r><w:rPr><w:w w:val="90"/></w:rPr><w:t>x</w:t></w:r></w:p>
 <w:sectPr><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>
</w:hdr>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/header2.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();

        // 1. ST_MeasurementOrPercent. `ST_UniversalMeasure`'s pattern is
        //    `-?[0-9]+(\.[0-9]+)?(mm|cm|in|pt|pc|pi)` - there is no `twip`, so the
        //    number is converted rather than suffixed. One twip is 1/20 pt.
        assert!(text.contains(r#"<w:tblW w:w="110pt""#), "{text}");
        // 17663 twips is not a whole number of points and must not be rounded.
        assert!(text.contains(r#"<w:tblInd w:w="883.15pt""#), "{text}");
        assert!(text.contains(r#"<w:top w:w="0pt""#), "{text}");
        // `w:wBefore`/`w:wAfter` share the type; this checks the OTHER edge of
        // the same container, which is what a one-sided rule would miss.
        assert!(text.contains(r#"<w:start w:w="5.4pt""#), "{text}");

        // 2. ST_TextScale on `w:rPr/w:w/@w:val`.
        assert!(text.contains(r#"<w:w w:val="90%"/>"#), "{text}");

        // 3. A `w:spacing` edge is ST_TwipsMeasure, NOT a universal measure, and
        //    must stay a bare number. This is the negative half: matching on the
        //    attribute name alone would corrupt every measurement in the document.
        assert!(
            text.contains(r#"<w:spacing w:before="240"/>"#),
            "ST_TwipsMeasure keeps its bare number: {text}"
        );

        // 4. CT_PageMar's three required attributes, synthesised.
        assert!(text.contains(r#"w:gutter="0""#), "{text}");
        assert!(text.contains(r#"w:header="720""#), "{text}");
        assert!(text.contains(r#"w:footer="720""#), "{text}");
    }

    /// `w:charset/@w:val` becomes `@w:characterSet`.
    ///
    /// `CT_Charset` declares one attribute and it is not `@w:val`, so this is a
    /// RENAME rather than a value map, and it belongs in `RENAMES` where the
    /// container-keyed half can be reasoned about with the other three.
    #[test]
    fn a_charset_value_becomes_the_attribute_strict_declares() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
 <w:docDefaults><w:rPrDefault><w:rPr><w:charset w:val="CC"/></w:rPr></w:rPrDefault></w:docDefaults>
</w:styles>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/styles.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"<w:charset w:characterSet="CC"/>"#),
            "{text}"
        );
        assert!(
            !text.contains(r#"w:val="CC""#),
            "and not the old name: {text}"
        );
    }

    /// A block with no understood choice and **no** `mc:Fallback` resolves to
    /// nothing, and that is a removal — so it is counted and named.
    ///
    /// Three corpus documents carry exactly this in `word/settings.xml`:
    /// `<mc:Choice Requires="wpsCustomData"/>` with no fallback, wrapping a Word
    /// spelling-version flag. MCE says the content goes; the report says so too,
    /// because `verify_no_silent_loss` compares the two counters and a removal that
    /// is not in the report is the failure this whole module is written against.
    #[test]
    fn a_block_with_no_understood_choice_and_no_fallback_is_a_named_removal() {
        let normalizer = TransitionalNormalizer::new();
        let source = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
 xmlns:cd="http://schemas.microsoft.com/office/word/2010/wordprocessingCustomData">
<w:settings><mc:AlternateContent><mc:Choice Requires="cd"><cd:typoFeatureVersion val="1"/>
</mc:Choice></mc:AlternateContent><w:zoom w:percent="100"/></w:settings>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/settings.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(!text.contains("typoFeatureVersion"), "it is gone: {text}");
        // `100%`, not `100`. `ST_DecimalNumberOrPercent` is a union of
        // `s:ST_Percentage` alone and `ST_Percentage`'s pattern is
        // `-?[0-9]+(\.[0-9]+)?%`, so a bare number is not a value this attribute can
        // hold. The assertion used to require the Transitional spelling, which is
        // the same defect the census item this rule closes describes - the corpus
        // is written by producers that fail their own schema, and a normalizer
        // that agrees with them is not a normalizer.
        assert!(
            text.contains(r#"<w:zoom w:percent="100%"/>"#),
            "and the percent gets the sign Strict requires: {text}"
        );
        let report = normalizer.report();
        assert_eq!(report.lossy_count(), 1, "one removal: {report}");
        assert!(
            report.to_string().contains("resolved to nothing"),
            "{report}"
        );
        // The counters agree: a removal that the report names is not a silent loss.
        report.verify_no_silent_loss().expect("SC-4");
    }

    /// The other two policies, on the same input, because a policy that is only
    /// exercised by its default is a policy nobody knows works.
    #[test]
    fn the_other_two_policies_do_what_they_say() {
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
 mc:Ignorable="wps">
<w:body><w:p><w:r><mc:AlternateContent>
<mc:Choice Requires="wps"><wps:wsp/></mc:Choice>
<mc:Fallback><w:t>fallback text</w:t></mc:Fallback>
</mc:AlternateContent></w:r></w:p></w:body></w:document>"#;
        let part = PartId::new("/word/document.xml");

        let fallback = TransitionalNormalizer::with_options(NormalizerOptions::with_mce(
            McePolicy::PreferFallback,
        ));
        let text = String::from_utf8(
            fallback
                .normalize(&part, source.as_bytes())
                .unwrap()
                .into_owned(),
        )
        .unwrap();
        assert!(text.contains("fallback text"), "{text}");
        assert!(!text.contains("wps:wsp"), "{text}");

        let reported =
            TransitionalNormalizer::with_options(NormalizerOptions::with_mce(McePolicy::Report));
        let text = String::from_utf8(
            reported
                .normalize(&part, source.as_bytes())
                .unwrap()
                .into_owned(),
        )
        .unwrap();
        assert!(
            text.contains("AlternateContent"),
            "Report leaves the block exactly as it was, choices included: {text}"
        );
        assert!(
            text.contains("wps:wsp"),
            "which means the choice is still there to be read: {text}"
        );
        assert!(
            text.contains("fallback text"),
            "and so is the fallback: {text}"
        );
        assert!(
            reported
                .report()
                .to_string()
                .contains("McePolicy::Report reports MCE"),
            "{}",
            reported.report()
        );
    }

    /// A resolved branch meets the same stages as the rest of the part, and the
    /// corpus is why: **six of the nine blocks have an `mc:Fallback` that is a
    /// `w:pict`**, and a branch written straight out would have had its picture
    /// dropped by T7 instead of converted by it — the same picture, from the same
    /// element, that a `w:pict` outside an `mc:AlternateContent` keeps. Under
    /// [`McePolicy::PreferFallback`] that is exactly what happens, and this test is
    /// what holds it.
    #[test]
    fn a_fallback_branch_still_meets_the_vml_picture_conversion() {
        let normalizer = TransitionalNormalizer::with_options(NormalizerOptions::with_mce(
            McePolicy::PreferFallback,
        ));
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"
 xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">
<w:body><w:p><w:r><mc:AlternateContent>
<mc:Choice Requires="wps"><wps:wsp/></mc:Choice>
<mc:Fallback><w:pict><v:shape id="s1" style="width:72pt;height:36pt"><v:imagedata r:id="rId3"/></v:shape></w:pict></mc:Fallback>
</mc:AlternateContent></w:r></w:p></w:body></w:document>"#;
        let output = normalizer
            .normalize(&PartId::new("/word/document.xml"), source.as_bytes())
            .unwrap();
        let text = String::from_utf8(output.into_owned()).unwrap();
        assert!(
            text.contains(r#"<a:blip r:embed="rId3"/>"#),
            "the fallback's picture is converted, not dropped: {text}"
        );
        assert!(!text.contains("v:shape"), "{text}");
    }

    /// `InvariantMode::Strict`, whose promise the doc comment on `normalize()`
    /// could not keep until now because nothing read the field.
    ///
    /// A namespace the pipeline does not own and did not remove is a fact about the
    /// **output**, and under `Strict` it is an error. The same input under
    /// [`Lenient`](InvariantMode::Lenient) is kept with a record, which is what §10.9
    /// asks for and what the default must stay.
    #[test]
    fn the_strict_invariant_mode_turns_a_surviving_namespace_into_an_error() {
        let source = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:zz="urn:private:extension-that-survives">
<w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
        let part = PartId::new("/word/document.xml");

        let lenient = TransitionalNormalizer::new();
        assert!(
            lenient.normalize(&part, source.as_bytes()).is_ok(),
            "the default keeps a part it cannot vouch for"
        );
        assert!(
            lenient
                .report()
                .to_string()
                .contains("neither the Strict form of a registered family"),
            "and says so: {}",
            lenient.report()
        );

        let strict = TransitionalNormalizer::with_options(NormalizerOptions::with_invariants(
            InvariantMode::Strict,
        ));
        let error = strict
            .normalize(&part, source.as_bytes())
            .expect_err("Strict must not return a part it cannot vouch for");
        assert!(
            error.to_string().contains("Strict invariant violation"),
            "{error}"
        );
        assert!(
            error.to_string().contains("urn:private:extension"),
            "{error}"
        );
    }

    /// A part the pipeline did not **change** is not policed, and this is the
    /// boundary that makes the invariant mean something.
    ///
    /// The seam offers every part of the package and the writer copies some of
    /// them verbatim — a custom-XML item is a producer's private vocabulary carried
    /// on purpose (ADR-0007), and a `SmartArt` diagram part carries a dozen vendor
    /// namespaces for the same reason. Those bytes are the producer's, no stage of
    /// ours touched them, and calling their namespaces a violation of *our* output
    /// would be a false positive on the first document that has one. This test is
    /// the boundary, and it was found by `strict-profile`, whose
    /// `word/diagrams/data1.xml` declares `ds`, `dgm` and `a14` and is copied
    /// verbatim.
    #[test]
    fn a_part_no_stage_touched_is_not_policed() {
        let strict = TransitionalNormalizer::with_options(NormalizerOptions::with_invariants(
            InvariantMode::Strict,
        ));
        // A `customXml` item: a private vocabulary, no Transitional family in it,
        // so `part_needs_normalization` is false and the part comes back borrowed.
        let source = br#"<?xml version="1.0"?><properties xmlns="urn:a-vendor:properties:2024"><reviewed val="1"/></properties>"#;
        let output = strict
            .normalize(&PartId::new("/customXml/item1.xml"), source)
            .expect("a pass-through part is not ours to fail");
        assert!(matches!(output, Cow::Borrowed(_)), "borrowed unchanged");
    }

    #[test]
    fn options_are_configurable_without_changing_the_default_behaviour() {
        let normalizer =
            TransitionalNormalizer::with_options(NormalizerOptions::with_mce(McePolicy::Report));
        let output = normalizer
            .normalize(&part(), TRANSITIONAL.as_bytes())
            .unwrap();
        assert!(String::from_utf8(output.into_owned())
            .unwrap()
            .contains("purl.oclc.org"));
    }
}
