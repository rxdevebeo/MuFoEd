//! Pass-through of the parts this project does not model (`STAGE-8-TASK.md`
//! §3, W7).
//!
//! # What is being passed through, and why it is not a copy feature
//!
//! A `.docx` can carry parts whose XML this project does not model: a
//! DrawingML chart, the four parts of a SmartArt diagram, the workbook a chart
//! is linked to, custom XML. The model does not carry them and is not going to —
//! modelling a chart means modelling DrawingML charts. But the *reference* to
//! them lives in `word/document.xml`, and a document that keeps the reference
//! without the part behind it is what Word calls unreadable content.
//!
//! So the writer does the smallest honest thing: it copies the parts a reference
//! reaches, keeps their names, keeps their `.rels` byte for byte, and re-points
//! the reference at a relationship the written package declares.
//!
//! # Why the bytes are copied verbatim
//!
//! A copied part's *own* `.rels` ids are referenced from inside that part
//! (`<c:externalData r:id="rId3"/>`). Renumbering them would break the part, and
//! keeping the part names is what lets its `.rels` be copied unchanged. The only
//! ids this write renumbers are the ones **it** writes: the relationships of
//! `word/document.xml`, because that part is regenerated and so is the body that
//! points into it.
//!
//! # What is not passed through
//!
//! - **A part the source declares and the writer produces itself.** Styles,
//!   numbering, settings, theme, font table, notes, headers and footers are
//!   regenerated from the model; the model is the authority for them, and copying
//!   the source's version too would give the package two parts of one name.
//! - **A part nothing reaches.** `word/diagrams/drawing1.xml` in a producer's
//!   package can be an orphan (no `.rels` points at it). It stays out, and that is
//!   not a loss: nothing referenced it.
//! - **A part that cannot be read.** Recorded, never dropped in silence (SC-10).

use std::collections::{BTreeMap, BTreeSet};

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelType;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::{AnchorDrawing, Drawing, DrawingKind, Graphic};
use strict_ooxml_wml::model::inline::{Inline, RunContent};

use crate::ctx::Ctx;
use crate::package::{RelBuilder, RelationshipInfo, Source};

/// The relationship types the writer produces from the model itself.
///
/// A source relationship of one of these types is satisfied by the writer's own
/// part, so the source's version is neither copied nor reported.
pub(crate) const OWNED_TYPES: &[RelType] = &[
    RelType::OfficeDocument,
    RelType::Styles,
    RelType::Numbering,
    RelType::Settings,
    RelType::Theme,
    RelType::FontTable,
    RelType::Image,
    RelType::Hyperlink,
    RelType::Header,
    RelType::Footer,
    RelType::Footnotes,
    RelType::Endnotes,
];

/// A vendor part that is a SHADOW of one this writer regenerates, so it must not
/// be copied either.
///
/// Word 2010 wrote `word/stylesWithEffects.xml` beside `word/styles.xml`: the
/// same styles in a 2007-compatible spelling, under the relationship type below.
/// Nothing in the body references it, so the pass-through copied it like any other
/// unmodelled part — and the copy is a *stale duplicate*: the styles it holds are
/// the ones the source had, while `word/styles.xml` in the written package is the
/// model's. On `sdk-tbllayout.docx` the written pair is 2 258 bytes against
/// 15 668, and Word 2010 prefers the shadow when it is there. So the package
/// contradicted itself, and it did so silently, which is the failure mode SC-10
/// exists to prevent.
///
/// The part is therefore left out, with a loss recorded, because it is not
/// content: it is a second copy of a part this write owns. Copying a part we
/// regenerate is what [`OWNED_TYPES`] already forbids for the part itself; this is
/// the same rule one shadow further out.
const SHADOW_REL_TYPES: &[&str] =
    &["http://schemas.microsoft.com/office/2007/relationships/stylesWithEffects"];

fn is_shadow(info: &RelationshipInfo) -> bool {
    SHADOW_REL_TYPES
        .contains(&strict_ooxml_core::opc::rels::strict_type_uri(&info.rel_type).as_str())
}

/// The most unmodelled parts one write will copy.
///
/// A document does not have hundreds of charts, and a hostile `.rels` graph
/// could otherwise make this write read a whole package. The limit is checked
/// while copying, not after.
const MAX_COPIED_PARTS: usize = 512;

/// One part to copy, and the content type the source declared for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopiedPart {
    /// The part name, unchanged from the source.
    pub name: String,
    /// The bytes, unchanged.
    pub bytes: Vec<u8>,
    /// The source's content type, when it declared one.
    pub content_type: Option<String>,
}

/// The pass-through a write computed, ready to be emitted.
#[derive(Clone, Debug, Default)]
pub(crate) struct PassThrough {
    /// The relationship ids the document part's source used, mapped to the ids
    /// this write emits.
    document: BTreeMap<String, String>,
    /// The parts to copy, ordered by name.
    parts: Vec<CopiedPart>,
    /// The package root's own relationships this write emits besides the office
    /// document, which the writer always writes (`O-1a`).
    root: Vec<RootRelationship>,
}

/// One relationship of the package root, to write into `_rels/.rels`.
///
/// The id is the one this write hands out, not the source's: nothing in the body
/// refers to a root relationship, so the source's numbering is not a reference
/// that has to keep working — and a stable `rId2` is what SC-1 needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootRelationship {
    /// The relationship id to write.
    pub id: String,
    /// The raw type URI, Strict.
    pub raw_type: String,
    /// The target, relative to the package root.
    pub target: String,
}

impl PassThrough {
    /// The relationship id to write for a reference that used `old_id`.
    pub(crate) fn document_rel(&self, old_id: &str) -> Option<&str> {
        self.document.get(old_id).map(String::as_str)
    }

    /// The parts to copy, ordered by name.
    pub(crate) fn parts(&self) -> &[CopiedPart] {
        &self.parts
    }

    /// The root relationships to write after the office document.
    pub(crate) fn root_relationships(&self) -> &[RootRelationship] {
        &self.root
    }
}

/// Computes the pass-through for one write.
///
/// `referenced` is the relationship ids the model points at from the body, in
/// document order — the ones a `c:chart` or a `dgm:relIds` element carries. They
/// are registered first so the ids the body writes are the earliest the
/// [`RelBuilder`] hands out, which keeps the output stable for a document that
/// has both hyperlinks and a chart.
pub(crate) fn plan(
    ctx: &mut Ctx<'_>,
    source: &dyn Source,
    rels: &mut RelBuilder,
    main: &PartId,
    referenced: &[String],
) -> PassThrough {
    let mut out = PassThrough::default();
    let mut reached: Vec<RelationshipInfo> = Vec::new();

    for old_id in referenced {
        let Some(info) = source.relationship(main, old_id) else {
            continue;
        };
        let id = rels.add(&info.rel_type, info.target.clone(), info.external);
        out.document.insert(old_id.clone(), id);
        if !info.external {
            reached.push(info);
        }
    }

    // What the source's document part declared and the writer does not produce.
    // `w:webSettings` is the case that matters: a relationship of the document
    // part with nothing in the body pointing at it, which is why it was being
    // dropped in silence before.
    let mut seen: BTreeSet<String> = out.document.keys().cloned().collect();
    for info in source.relationships(main) {
        if seen.contains(&info.id) || OWNED_TYPES.contains(&info.rel_type) {
            continue;
        }
        seen.insert(info.id.clone());
        if is_shadow(&info) {
            // Recorded as a loss, not dropped in silence (SC-10, ADR-0007): the
            // part is genuinely not in the written package, and the reason is
            // the reason a reader would want to know that.
            ctx.report_unsupported(
                "W7.stylesWithEffects",
                &format!(
                    "{} was not copied: it shadows word/styles.xml, which this write \
                     regenerates from the model, so a copy would leave the package with a \
                     second and stale set of styles",
                    info.target
                ),
                &SourceLocation::unknown(),
            );
            continue;
        }
        let id = rels.add(&info.rel_type, info.target.clone(), info.external);
        out.document.insert(info.id.clone(), id);
        if !info.external {
            reached.push(info);
        }
    }

    // The parts themselves, transitively: a chart reaches its own `.rels`, which
    // reaches a workbook, and stopping at the first hop would produce a package
    // with a chart that cannot open its data.
    let mut queue: Vec<PartId> = reached
        .iter()
        .filter_map(|info| resolve(main, &info.target))
        .collect();
    let mut copied: BTreeSet<String> = BTreeSet::new();
    let mut index = 0;
    while index < queue.len() {
        let part = queue[index].clone();
        index += 1;
        if !copied.insert(part.as_str().to_owned()) {
            continue;
        }
        if copied.len() > MAX_COPIED_PARTS {
            ctx.report_unsupported(
                "W7.passthrough",
                &format!("more than {MAX_COPIED_PARTS} unmodelled parts were reached; the rest were not copied"),
                &SourceLocation::unknown(),
            );
            break;
        }
        let bytes = match source.read_part(&part) {
            Ok(bytes) => bytes,
            Err(error) => {
                ctx.report_unsupported(
                    "W7.passthrough",
                    &format!("{} could not be read: {error}", part.as_str()),
                    &SourceLocation::unknown(),
                );
                continue;
            }
        };
        let is_rels = part
            .as_str()
            .to_ascii_lowercase()
            .ends_with(CONTENT_TYPE_RELS_SUFFIX);
        out.parts.push(CopiedPart {
            name: part.as_str().to_owned(),
            bytes: if is_rels {
                strict_rels_namespace(&bytes)
            } else {
                bytes
            },
            content_type: source.content_type(&part),
        });
        if let Some(rels_part) = rels_part_of(&part) {
            if let Ok(bytes) = source.read_part(&rels_part) {
                out.parts.push(CopiedPart {
                    name: rels_part.as_str().to_owned(),
                    bytes: strict_rels_namespace(&bytes),
                    content_type: None,
                });
            }
        }
        for info in source.relationships(&part) {
            if info.external {
                continue;
            }
            if let Some(target) = resolve(&part, &info.target) {
                queue.push(target);
            }
        }
    }
    // Ordered by name so two runs of the writer agree on the ZIP's entry order
    // (SC-1) whatever order the traversal reached them in.
    out.parts.sort_by(|left, right| left.name.cmp(&right.name));

    // Third pass: the package root's own relationships. `_rels/.rels` is written
    // from scratch, so everything the source declared there has to be either
    // carried or named.
    //
    // **`docProps/core.xml` is carried whole** (`O-1a`). It is a set of statements
    // *about the document*: title, subject, creator, keywords, description,
    // language, revision, and the dates the producer recorded. All of them stay
    // true of the content this write copies, so copying them is not a claim — and
    // `dcterms:modified` in particular is the *content's* history, not this
    // container's: a fresh date cannot be written anyway, because SC-1 makes the
    // output reproducible and a clock cannot be part of that.
    //
    // **`docProps/app.xml` is carried, minus the counters that are statements
    // about a rendering.** The part used to be named instead, on the grounds
    // that it holds "statistics about a rendering this writer does not perform".
    // That reason is right about `Pages`, `Words`, `Characters`, `Lines` and
    // `TotalTime` and wrong about the rest: `Template` and `Company` are facts
    // about the document's provenance, `TitlesOfParts` and `HeadingPairs` are an
    // index of the content, and `DocSecurity` is a fact about the file. Dropping
    // the whole part threw 49 of 58 corpus documents' extended properties away to
    // avoid asserting five numbers — a loss the report named and nobody could do
    // anything about. So the part comes, the five counters do not, and the names
    // of the five are in [`APP_RENDERING_COUNTERS`] for the same reason
    // `Vec::retain` is: a whitelist nobody can read is a whitelist nobody can
    // check.
    //
    // **`docProps/custom.xml` is carried whole** (`TZ-17`). Custom properties are
    // the producer's own statements about its document — a project code, a
    // review state, a workflow id — and none of them describes a rendering.
    for info in source.relationships(&PartId::new("/")) {
        if matches!(info.rel_type, RelType::OfficeDocument) {
            continue;
        }
        let Some(target) = resolve(&PartId::new("/"), &info.target) else {
            ctx.report_unsupported(
                "W7.package-properties",
                &format!("{} could not be resolved to a part", info.target),
                &SourceLocation::unknown(),
            );
            continue;
        };
        // The package thumbnail is a **root** relationship, not one of `app.xml`'s:
        // `docProps/app.xml` has no `.rels` at all in nine of the ten corpus
        // documents that carry one, and the relationship that names the thumbnail
        // is `.../relationships/metadata/thumbnail` on `_rels/.rels`. Looking in
        // the wrong place is how a property part arrived declaring a thumbnail the
        // package did not contain.
        //
        // **The bytes and the relationship travel together**, and the first
        // version got that wrong in the most annoying direction: it carried the
        // thumbnail and then `continue`d, which dropped the relationship naming
        // it — so the second write of the same document found an orphan and
        // reported it, and the package did not settle in one generation.
        if is_package_thumbnail(&info) {
            match source.read_part(&target) {
                Ok(bytes) => out.parts.push(CopiedPart {
                    name: target.as_str().to_owned(),
                    bytes,
                    content_type: source.content_type(&target),
                }),
                Err(error) => ctx.report_unsupported(
                    "W7.dropped-part",
                    &format!(
                        "{target} is the package thumbnail and could not be read ({error}), so \
                         the relationship is kept and the bytes are not"
                    ),
                    &SourceLocation::unknown(),
                ),
            }
            out.root.push(RootRelationship {
                id: format!("rId{}", out.root.len() + 2),
                raw_type: thumbnail_type_uri(),
                target: target.as_str().trim_start_matches('/').to_owned(),
            });
            continue;
        }
        let transform: fn(&[u8]) -> Vec<u8> = match target.as_str() {
            CORE_PROPERTIES_PART => strict_core_properties_namespace,
            APP_PROPERTIES_PART => without_rendering_counters,
            CUSTOM_PROPERTIES_PART => strict_custom_properties_namespaces,
            _ => {
                ctx.report_unsupported(
                    "W7.package-properties",
                    &format!(
                        "{} is a part this write neither produces nor can vouch for, so it is \
                         not carried; the relationship type is {:?}",
                        info.target, info.rel_type
                    ),
                    &SourceLocation::unknown(),
                );
                continue;
            }
        };
        let type_uri = match target.as_str() {
            CORE_PROPERTIES_PART => core_properties_type_uri().to_owned(),
            APP_PROPERTIES_PART => app_properties_type_uri().to_owned(),
            _ => custom_properties_type_uri().to_owned(),
        };
        match source.read_part(&target) {
            Ok(bytes) => {
                let mut copied = copied_property_part(&target, &bytes, transform, source);
                // A property part's own relationships, and the parts they reach.
                // `docProps/app.xml` references `docProps/thumbnail.jpeg` through
                // them, and a relationship part copied without its target is a
                // reference to nothing: the census ledger called nine thumbnails
                // lost for exactly this, and it was right — the app properties
                // arrived with a `.rels` pointing at a part that was not in the
                // package.
                copied.extend(property_rel_targets(&target, source, ctx));
                out.parts.extend(copied);
            }
            Err(error) => ctx.report_unsupported(
                "W7.package-properties",
                &format!(
                    "{} is a statement about the document, but its bytes could not be \
                     read ({error}), so it is lost rather than half-copied",
                    info.target
                ),
                &SourceLocation::unknown(),
            ),
        }
        out.root.push(RootRelationship {
            id: format!("rId{}", out.root.len() + 2),
            raw_type: type_uri,
            target: target.as_str().trim_start_matches('/').to_owned(),
        });
    }
    out
}

/// The two parts OPC defines and **every** package has, which this write emits
/// from scratch whatever the source held.
///
/// They are excluded from the accounting below for the ordinary reason, and it is
/// worth naming because the first version of this function reported them: a part
/// that is not in the list of parts the source had is not evidence that anything
/// was lost, and `[Content_Types].xml` is the one part no package can do without.
const OPC_SCAFFOLD: &[&str] = &["/[Content_Types].xml", "/_rels/.rels"];

/// Names every part the source had that the written package does not, grouped by
/// directory, with a reason that says what happened to it.
///
/// This is the closing of the audit §8 finding, and the finding is worth
/// restating because it is not obvious from the outside: **a dropped part is
/// invisible.** A part that is absent draws nothing, validates against every
/// schema, and leaves no trace in a loss report that never mentioned it. The
/// audit named one case — 16 embedded font binaries in 2 documents, gone together
/// with `word/_rels/fontTable.xml.rels` — and said the whole defect was that
/// "this is not in the loss report: a silent loss".
///
/// It takes the **written** part list rather than a list of parts this write
/// *might* produce, because several are conditional: `word/numbering.xml` is
/// written only when the model carries a numbering table, and one corpus document
/// has a numbering part the model read as empty. A plan-time list would have
/// called that a non-loss, which is precisely the bug this function was written
/// to remove.
///
/// Grouping is by directory. A per-part record would be one line per file for
/// what is one decision, and a report that long stops being read; the directory
/// is what a person can act on, and it is what the census's `unaccounted` signal
/// matches on, so the two cannot drift apart.
pub(crate) fn report_what_was_dropped(ctx: &mut Ctx<'_>, source: &dyn Source, written: &[String]) {
    let produced: BTreeSet<&str> = written.iter().map(String::as_str).collect();
    let mut by_directory: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for part in source.parts() {
        let name = part.as_str();
        if produced.contains(name) || OPC_SCAFFOLD.contains(&name) || !name.starts_with('/') {
            continue;
        }
        // A `.rels` beside a part that IS in the output was replaced by this
        // write's own: the source's copy describes relationships the written part
        // does not have, and keeping it would leave the package declaring them.
        // The owner being present is the test, and not "a rels part is present" —
        // the writer is free to emit none for a part that ended up with no
        // relationships.
        if name.ends_with(CONTENT_TYPE_RELS_SUFFIX)
            && rels_owner(name).is_none_or(|owner| produced.contains(owner.as_str()))
        {
            continue;
        }
        let directory = name
            .rsplit_once('/')
            .map_or_else(|| name.to_owned(), |(dir, _)| dir.to_owned());
        by_directory
            .entry(directory)
            .or_default()
            .push(name.to_owned());
    }
    for (directory, names) in by_directory {
        ctx.report_unsupported(
            "W7.dropped-part",
            &format!(
                "{directory}: {} part(s) in the source are not in the written package \
                 - {}. The bytes are legal Strict and no schema rejects their absence - a \
                 part that is missing draws nothing and validates - so the only record \
                 that they were here is this line",
                names.len(),
                names.join(", ")
            ),
            &SourceLocation::unknown(),
        );
    }
}

/// Whether a root relationship is the package thumbnail.
///
/// Named by its type suffix, because there is no normalized variant for it —
/// `RelationshipInfo` keeps a *normalized* `RelType` and everything unrecognised
/// becomes `Other`, and the thumbnail lands there. `package/2006` is Transitional
/// and `purl.oclc.org` is Strict, so a Strict package can carry this relationship
/// and it still has to be recognized; matching the suffix is what sees both.
fn is_package_thumbnail(info: &RelationshipInfo) -> bool {
    match &info.rel_type {
        RelType::Other(uri) => uri
            .rsplit('/')
            .next()
            .is_some_and(|suffix| suffix == "thumbnail"),
        _ => false,
    }
}

/// The Strict form of the package-thumbnail relationship type.
///
/// There is no normalized variant — nothing in a body ever refers to a thumbnail —
/// so the URI is built from the OPC base the same way the core-properties one is.
/// The source's spelling is Transitional or Strict depending on what it was, and
/// `map_rel_or_content_type` rewrites exactly this string, so both are covered by
/// building ours rather than copying the input's.
fn thumbnail_type_uri() -> String {
    "http://purl.oclc.org/ooxml/package/relationships/metadata/thumbnail".to_owned()
}

/// The internal targets a property part's `.rels` reaches, copied beside it.
///
/// `docProps/app.xml` has one relationship — to `docProps/thumbnail.jpeg` — and a
/// property part whose rels arrives without its target is a package with a
/// dangling reference, which is the one thing a pass-through exists to avoid.
/// External targets are left alone: they are a URI on the internet, not bytes.
fn property_rel_targets(
    target: &PartId,
    source: &dyn Source,
    ctx: &mut Ctx<'_>,
) -> Vec<CopiedPart> {
    let Some(rels_part) = rels_part_of(target) else {
        return Vec::new();
    };
    let Ok(_rels_bytes) = source.read_part(&rels_part) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for info in source.relationships(target) {
        if info.external {
            continue;
        }
        let Some(resolved) = resolve(target, &info.target) else {
            continue;
        };
        match source.read_part(&resolved) {
            Ok(bytes) => out.push(CopiedPart {
                name: resolved.as_str().to_owned(),
                bytes,
                content_type: source.content_type(&resolved),
            }),
            Err(error) => ctx.report_unsupported(
                "W7.dropped-part",
                &format!(
                    "{resolved} is referenced by {target}'s relationships but could not be \
                     read ({error}), so the reference is kept and the bytes are not"
                ),
                &SourceLocation::unknown(),
            ),
        }
    }
    out
}

/// The part a `.rels` belongs to: `/word/_rels/header1.xml.rels` -> `/word/header1.xml`.
fn rels_owner(rels_name: &str) -> Option<String> {
    let (dir, file) = rels_name.rsplit_once('/')?;
    let dir = dir.strip_suffix("/_rels")?;
    let file = file.strip_suffix(".rels")?;
    Some(format!("{dir}/{file}"))
}

/// The property part itself, plus its `.rels` when it has one.
///
/// A `docProps/app.xml` that references a thumbnail (`docProps/thumbnail.jpeg`)
/// is only usable with it, and a copy of the properties that points at a missing
/// thumbnail is a dangling reference - which is the one thing the pass-through
/// exists to prevent. Nine corpus documents carry one.
fn copied_property_part(
    target: &PartId,
    bytes: &[u8],
    transform: fn(&[u8]) -> Vec<u8>,
    source: &dyn Source,
) -> Vec<CopiedPart> {
    let mut out = vec![CopiedPart {
        name: target.as_str().to_owned(),
        bytes: transform(bytes),
        content_type: source.content_type(target),
    }];
    if let Some(rels_part) = rels_part_of(target) {
        if let Ok(rels_bytes) = source.read_part(&rels_part) {
            out.push(CopiedPart {
                name: rels_part.as_str().to_owned(),
                bytes: strict_rels_namespace(&rels_bytes),
                content_type: None,
            });
        }
    }
    out
}

/// The one part of `docProps` a write carries whole (`O-1a`).
const CORE_PROPERTIES_PART: &str = "/docProps/core.xml";

/// The extended-properties part: statements about the document, minus the five
/// that are statements about a rendering.
const APP_PROPERTIES_PART: &str = "/docProps/app.xml";

/// The custom-properties part: the producer's own statements about its document.
const CUSTOM_PROPERTIES_PART: &str = "/docProps/custom.xml";

/// The elements of `docProps/app.xml` that describe a **rendering** rather than a
/// document, and are therefore not carried.
///
/// Names, not a byte pattern: `<Pages>1</Pages>` and `<Lines>42</Lines>` are
/// elements, and an element is removed whole. Each is a number this writer cannot
/// produce — it lays nothing out, so it has no page count, no line count and no
/// editing time — and carrying the producer's would assert them for a document
/// nobody re-paginated. Everything else in the part is either a fact about the
/// file (`Template`, `Company`, `DocSecurity`, `Application`, `AppVersion`) or an
/// index of its content (`HeadingPairs`, `TitlesOfParts`), and all of it stays.
///
/// `Application` and `AppVersion` are the two that a reader could reasonably
/// call a claim: they name the *producer*. They are kept because they remain
/// true in the sense that matters — the content, the styles and the metadata of
/// this package came from that application — and because a part that claims it
/// was written by nothing at all is a worse answer than one that names where the
/// document came from. The claim this writer does not make anywhere is about a
/// *rendering*, and that is what the five above are.
const APP_RENDERING_COUNTERS: &[&str] = &[
    "Pages",
    "Words",
    "Characters",
    "CharactersWithSpaces",
    "Lines",
    "Paragraphs",
    "TotalTime",
];

/// The Strict relationship type of [`CORE_PROPERTIES_PART`].
///
/// Written out rather than taken from `RelType::from_uri`, because this type has
/// no normalized variant: nothing in the body refers to it, so normalizing it
/// would be a variant only this one use site needs.
fn core_properties_type_uri() -> &'static str {
    "http://purl.oclc.org/ooxml/package/relationships/metadata/core-properties"
}

/// The Strict relationship type of [`APP_PROPERTIES_PART`].
fn app_properties_type_uri() -> &'static str {
    "http://purl.oclc.org/ooxml/officeDocument/relationships/extended-properties"
}

/// The Strict relationship type of [`CUSTOM_PROPERTIES_PART`].
fn custom_properties_type_uri() -> &'static str {
    "http://purl.oclc.org/ooxml/officeDocument/relationships/custom-properties"
}

/// Rewrites the namespace of a copied `docProps/app.xml` and removes the
/// elements that are statements about a rendering.
///
/// Two operations, both byte-level, and neither touches a value:
///
/// 1. the namespace declaration, for the same reason as
///    [`strict_core_properties_namespace`] — a declaration is not content, and
///    Strict renamed this one too;
/// 2. the elements named in [`APP_RENDERING_COUNTERS`], whole.
///
/// A byte-level element removal is only safe because the elements are matched by
/// their **full tag**, `<Name>` … `</Name>`: an element whose name is a prefix of
/// another's would be cut at the wrong place, and `Lines` is a substring of
/// `CharactersWithSpaces` only in the sense of spelling, not of XML — which is
/// exactly the kind of assumption a regex makes and a parser does not. The whole
/// subtree of the counters is what goes, and each of them is a leaf, so a
/// well-formed `app.xml` yields a well-formed result.
///
/// The value of what stays is *not* touched, so `Company`, `Template`,
/// `DocSecurity`, `Application`, `AppVersion`, `HeadingPairs` and `TitlesOfParts`
/// come through exactly as the producer wrote them.
fn without_rendering_counters(bytes: &[u8]) -> Vec<u8> {
    let mut out = replace_all(
        bytes,
        TRANSITIONAL_APP_PROPERTIES_NS,
        STRICT_APP_PROPERTIES_NS,
    );
    // `app.xml` declares the variant-types vocabulary too, for
    // `HeadingPairs`/`TitlesOfParts`, and `vt` was renamed along with `app`.
    // Rewriting only the outer namespace left a Transitional URI in a Strict
    // package, which is what `no_written_part_carries_a_transitional_uri` said,
    // with the bytes in front of it.
    out = replace_all(&out, TRANSITIONAL_VARIANT_TYPES_NS, STRICT_VARIANT_TYPES_NS);
    for name in APP_RENDERING_COUNTERS {
        out = remove_element(&out, name);
    }
    out
}

/// The extended-properties namespace a Transitional producer writes.
const TRANSITIONAL_APP_PROPERTIES_NS: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

/// The Strict name ISO/IEC 29500 gives the same part.
const STRICT_APP_PROPERTIES_NS: &[u8] =
    b"http://purl.oclc.org/ooxml/officeDocument/extendedProperties";

/// The custom-properties namespace a Transitional producer writes, and the Strict
/// one. Two rewrites, because the part declares the `vt` vocabulary too and both
/// were renamed: `docProps/custom.xml` in the corpus carries
/// `<property …><vt:lpwstr>…</vt:lpwstr></property>`, and `vt` is in
/// `shared-documentPropertiesVariantTypes.xsd` in both families under the
/// `purl.oclc.org` names.
const TRANSITIONAL_CUSTOM_PROPERTIES_NS: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const TRANSITIONAL_VARIANT_TYPES_NS: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const STRICT_CUSTOM_PROPERTIES_NS: &[u8] =
    b"http://purl.oclc.org/ooxml/officeDocument/customProperties";
const STRICT_VARIANT_TYPES_NS: &[u8] = b"http://purl.oclc.org/ooxml/officeDocument/docPropsVTypes";

/// Rewrites the two namespaces of a copied `docProps/custom.xml`.
///
/// Declarations only, and the same reasoning as every other namespace rewrite
/// here: the properties themselves are the producer's statements about its own
/// document and stay byte for byte.
fn strict_custom_properties_namespaces(bytes: &[u8]) -> Vec<u8> {
    let out = replace_all(
        bytes,
        TRANSITIONAL_CUSTOM_PROPERTIES_NS,
        STRICT_CUSTOM_PROPERTIES_NS,
    );
    replace_all(&out, TRANSITIONAL_VARIANT_TYPES_NS, STRICT_VARIANT_TYPES_NS)
}

/// Removes every `<name>…</name>` and `<name/>` from `bytes`.
///
/// `name` is matched whole, so `Lines` never matches inside
/// `CharactersWithSpaces` and `Pages` never matches inside `PageSetup`. The
/// search starts after a `<` so a name appearing in an attribute *value* cannot
/// be mistaken for a tag.
fn remove_element(bytes: &[u8], name: &str) -> Vec<u8> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let empty = format!("<{name}/>");
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        let Some(at) = find(rest, open.as_bytes()) else {
            break;
        };
        // A longer element name that merely starts with `name`: skip it.
        //
        // `get` rather than an index: the input ends wherever it ends, and a
        // part cut short on `<Pages` - a real `docProps/app.xml` truncated by a
        // producer - has no byte after the last `<` it contains (AUD-10).
        let Some(&after) = rest.get(at + open.len()) else {
            // `return`, not `break`: the tail is appended again below the loop,
            // and copying it here as well would duplicate the part.
            out.extend_from_slice(rest);
            return out;
        };
        if after != b'>' && after != b'/' && after != b' ' {
            out.extend_from_slice(&rest[..at + open.len()]);
            rest = &rest[at + open.len()..];
            continue;
        }
        out.extend_from_slice(&rest[..at]);
        let tail = &rest[at..];
        if tail.starts_with(empty.as_bytes()) {
            rest = &tail[empty.len()..];
            continue;
        }
        match find(tail, close.as_bytes()) {
            Some(end) => {
                rest = &tail[end + close.len()..];
            }
            // An opening tag with no matching close: the input was not
            // well-formed, and cutting at the end would invent a removal that
            // did not happen. The element is left alone.
            None => {
                out.extend_from_slice(tail);
                return out;
            }
        }
    }
    out.extend_from_slice(rest);
    out
}

/// The extension every relationship part carries, lowercased for the comparison.
const CONTENT_TYPE_RELS_SUFFIX: &str = ".rels";

/// The OPC relationship namespace a Transitional producer writes.
const TRANSITIONAL_RELS_NS: &[u8] =
    b"xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"";

/// The Strict one this project writes everywhere else.
const STRICT_RELS_NS: &[u8] = b"xmlns=\"http://purl.oclc.org/ooxml/package/relationships\"";

/// The core-properties namespace a Transitional producer writes.
const TRANSITIONAL_CORE_PROPERTIES_NS: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/metadata/core-properties";

/// The Strict name ISO/IEC 29500 gives the same part.
const STRICT_CORE_PROPERTIES_NS: &[u8] =
    b"http://purl.oclc.org/ooxml/package/metadata/coreProperties";

/// Rewrites the core-properties namespace of a copied `docProps/core.xml`.
///
/// **The second and last byte a pass-through changes, and for the same reason as
/// the first** ([`strict_rels_namespace`]): a namespace *declaration* is not
/// content. Unlike a chart's internals, this part is one OPC itself defines and
/// Strict renamed, so a package carrying the Transitional spelling of it is a
/// package a conformance detector reports as `unknown` — and the writer's promise
/// is that its output is Strict on the first open
/// (`normalize_roundtrip.rs::a_written_package_needs_no_second_normalization_pass`,
/// which is what caught this).
///
/// Everything that is *in* the part — `dc:title`, `dc:creator`, `dcterms:created`,
/// `dcterms:modified`, the revision — is the producer's and stays byte for byte.
/// Those are the statements the part exists to make, and they are still true of the
/// document this write copies.
fn strict_core_properties_namespace(bytes: &[u8]) -> Vec<u8> {
    replace_all(
        bytes,
        TRANSITIONAL_CORE_PROPERTIES_NS,
        STRICT_CORE_PROPERTIES_NS,
    )
}

/// Replaces every occurrence of `from` with `to`.
fn replace_all(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = find(rest, from) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(to);
        rest = &rest[at + from.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// Rewrites the relationship namespace of a copied `.rels` part.
///
/// This is the **only** byte a pass-through changes, and it is a declaration, not
/// content: ISO/IEC 29500 Strict renamed the OPC relationships namespace, and a
/// package that mixes the two makes a conformance detector report `unknown`,
/// which is a claim a caller then has to investigate. Ids, types and targets are
/// untouched — a part's own references (`<c:externalData r:id="rId3"/>`) keep
/// working, which is the whole reason the rest of the file is copied verbatim.
///
/// A part's *content* is never rewritten. A Microsoft extension carries a
/// Transitional URI in an attribute **value**
/// (`<dsp:dataModelExt minVer="…/drawingml/2006/diagram"/>`), and rewriting a
/// value is a semantic edit this writer does not make. That part stays the
/// producer's own bytes, and the trade-off is recorded in ADR-0007.
fn strict_rels_namespace(bytes: &[u8]) -> Vec<u8> {
    replace_all(bytes, TRANSITIONAL_RELS_NS, STRICT_RELS_NS)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Resolves a relationship target against the part that declared it.
///
/// `../embeddings/book.xlsx` from `/word/charts/chart1.xml` is
/// `/word/embeddings/book.xlsx`, and a target that climbs above the root stops at
/// the root rather than producing a `/../..`.
fn resolve(from: &PartId, target: &str) -> Option<PartId> {
    if target.is_empty() {
        return None;
    }
    if target.starts_with('/') {
        // An absolute part name is already what a `PartId` holds; stripping the
        // leading slash would make a part the package cannot find.
        return Some(PartId::new(target));
    }
    let base = match from.as_str().rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    };
    let mut segments: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(PartId::new(format!("/{}", segments.join("/")).as_str()))
}

/// The `.rels` part that belongs to a part, per OPC.
fn rels_part_of(part: &PartId) -> Option<PartId> {
    let name = part.as_str().rsplit('/').next()?;
    let dir = part.as_str().rsplit_once('/').map_or("", |(dir, _)| dir);
    if dir.is_empty() {
        Some(PartId::new(format!("/_rels/{name}.rels").as_str()))
    } else {
        Some(PartId::new(format!("{dir}/_rels/{name}.rels").as_str()))
    }
}

/// The relationship ids the body points at for parts the model does not carry.
///
/// The traversal is the hyperlink traversal plus the places a drawing can hide: a
/// table cell, a shape's text box, a group's shapes. A reference the pass-through
/// never sees is a reference that dangles, and a chart inside a text box is
/// ordinary.
pub(crate) fn referenced_ids(blocks: &[Block]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => inlines(&paragraph.inlines, &mut out),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        for id in referenced_ids(&cell.blocks) {
                            push(&mut out, id);
                        }
                    }
                }
            }
            Block::SdtBlock(sdt) => {
                for id in referenced_ids(&sdt.blocks) {
                    push(&mut out, id);
                }
            }
            _ => {}
        }
    }
    out
}

fn inlines(items: &[Inline], out: &mut Vec<String>) {
    for item in items {
        match item {
            Inline::Drawing(item) => drawing_refs(item, out),
            // A `w:drawing` is normally *run content* (`w:r/w:drawing`), not an
            // inline of its own: a chart in a document almost always arrives
            // this way, and a traversal that only looked at `Inline::Drawing`
            // would find nothing at all — and the reference would then be written
            // only because the second pass happened to register it.
            Inline::Run(run) => {
                for content in &run.content {
                    if let RunContent::Drawing(item) = content {
                        drawing_refs(item, out);
                    }
                }
            }
            Inline::Hyperlink(link) => inlines(&link.inlines, out),
            Inline::Field(field) => inlines(&field.inlines, out),
            Inline::SdtInline(sdt) => inlines(&sdt.inlines, out),
            _ => {}
        }
    }
}

fn drawing_refs(item: &Drawing, out: &mut Vec<String>) {
    let payload: &Graphic = match &item.kind {
        DrawingKind::Inline(inline) => inline.graphic.as_ref(),
        DrawingKind::Anchor(AnchorDrawing { graphic, .. }) => graphic.as_ref(),
        // An opaque `w:drawing` was never resolved into a graphic, so there is
        // no reference to re-point; the writer records it as unserializable.
        DrawingKind::Opaque(_) => return,
    };
    match payload {
        Graphic::Chart(refs) | Graphic::Diagram(refs) => {
            for id in refs.ids() {
                push(out, id.to_owned());
            }
        }
        Graphic::Shape(shape) => text_box(&shape.text, out),
        Graphic::Group(group) => {
            for child in &group.children {
                if let Graphic::Shape(shape) = child {
                    text_box(&shape.text, out);
                }
            }
        }
        Graphic::None | Graphic::Picture(_) | Graphic::Other => {}
    }
}

fn text_box(text_box: &Option<strict_ooxml_wml::model::drawing::TextBox>, out: &mut Vec<String>) {
    if let Some(text_box) = text_box {
        for id in referenced_ids(&text_box.blocks) {
            push(out, id);
        }
    }
}

fn push(out: &mut Vec<String>, id: String) {
    if !out.contains(&id) {
        out.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        referenced_ids, rels_part_of, remove_element, resolve, without_rendering_counters,
    };
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::block::{Block, Paragraph, Table, TableCell, TableRow};
    use strict_ooxml_wml::model::drawing::{
        Drawing, DrawingKind, ForeignRefs, Graphic, InlineDrawing,
    };
    use strict_ooxml_wml::model::inline::Inline;

    fn chart(ids: &[&str]) -> Block {
        Block::Paragraph(Paragraph {
            props: Default::default(),
            inlines: vec![Inline::Drawing(Drawing {
                kind: DrawingKind::Inline(InlineDrawing {
                    extent: None,
                    doc_pr: None,
                    graphic_uri: None,
                    graphic: Box::new(Graphic::Chart(ForeignRefs {
                        rels: ids.iter().map(|id| std::sync::Arc::from(*id)).collect(),
                        location: Default::default(),
                    })),
                    location: Default::default(),
                }),
                location: Default::default(),
            })],
            rsids: Default::default(),
            para_id: None,
            text_id: None,
            location: Default::default(),
        })
    }

    /// A target is resolved against the part that declared it, which is what
    /// makes `../embeddings/book.xlsx` from a chart land in `/word/embeddings/`.
    #[test]
    fn a_target_is_resolved_against_its_part() {
        let chart = PartId::new("/word/charts/chart1.xml");
        assert_eq!(
            resolve(&chart, "../embeddings/book.xlsx").map(|p| p.as_str().to_owned()),
            Some("/word/embeddings/book.xlsx".to_owned())
        );
        assert_eq!(
            resolve(&chart, "colors1.xml").map(|p| p.as_str().to_owned()),
            Some("/word/charts/colors1.xml".to_owned())
        );
        assert_eq!(
            resolve(&chart, "/word/theme/theme1.xml").map(|p| p.as_str().to_owned()),
            Some("/word/theme/theme1.xml".to_owned())
        );
        // Climbing above the root stops at the root instead of producing `/..`.
        assert_eq!(
            resolve(&PartId::new("/word/document.xml"), "../../outside.xml")
                .map(|p| p.as_str().to_owned()),
            Some("/outside.xml".to_owned())
        );
        assert!(resolve(&chart, "").is_none());
    }

    /// The `.rels` of a part sits in a `_rels` directory beside it.
    #[test]
    fn the_rels_of_a_part_sit_beside_it() {
        assert_eq!(
            rels_part_of(&PartId::new("/word/charts/chart1.xml")).map(|p| p.as_str().to_owned()),
            Some("/word/charts/_rels/chart1.xml.rels".to_owned())
        );
        assert_eq!(
            rels_part_of(&PartId::new("/docProps/app.xml")).map(|p| p.as_str().to_owned()),
            Some("/docProps/_rels/app.xml.rels".to_owned())
        );
    }

    /// A chart inside a table cell is still a reference the pass-through must
    /// see: the cells are the most common place a table of figures lives.
    #[test]
    fn a_chart_in_a_table_cell_is_reached() {
        let blocks = vec![Block::Table(Table {
            props: Default::default(),
            grid: Vec::new(),
            rows: vec![TableRow {
                props: Default::default(),
                cells: vec![TableCell {
                    props: Default::default(),
                    blocks: vec![chart(&["rId4"])],
                    location: Default::default(),
                }],
                location: Default::default(),
            }],
            location: Default::default(),
        })];
        assert_eq!(referenced_ids(&blocks), vec!["rId4".to_owned()]);
    }

    /// The same id twice is one relationship, not two.
    #[test]
    fn a_repeated_reference_is_one_id() {
        let blocks = vec![chart(&["rId4"]), chart(&["rId4", "rId5"])];
        assert_eq!(
            referenced_ids(&blocks),
            vec!["rId4".to_owned(), "rId5".to_owned()]
        );
    }

    /// A part cut short in the middle of a tag must not be indexed past its end.
    ///
    /// `docProps/app.xml` truncated on `<Pages` is a real input, not a torture
    /// case: the file ends there, the bytes after the last `<` do not exist, and
    /// the byte the scanner wanted to read next to tell `<Pages` from
    /// `<PagesWords>` was that one (AUD-10).
    #[test]
    fn remove_element_survives_a_part_that_ends_inside_a_tag() {
        for input in [
            &b"<x><Pages"[..],
            &b"<x><Pages"[0..6],
            &b"<Pages"[..],
            &b"<Pages/"[..],
            &b""[..],
            &b"<"[..],
            &b"<x><Pages></Pages></x>"[..],
            &b"<x><PagesWords>1</PagesWords></x>"[..],
            &b"<x><Pages>1</Pages><PagesWords>2</PagesWords></x>"[..],
        ] {
            let out = remove_element(input, "Pages");
            if input == b"<x><PagesWords>1</PagesWords></x>" {
                // A longer name that starts with the one we remove.
                assert_eq!(out, input, "a longer name must not be touched");
            }
            if input == b"<x><Pages>1</Pages><PagesWords>2</PagesWords></x>" {
                assert_eq!(
                    String::from_utf8_lossy(&out),
                    "<x><PagesWords>2</PagesWords></x>"
                );
            }
        }
    }

    #[test]
    fn a_truncated_app_properties_part_is_returned_as_it_arrived() {
        // Same input, through the function that calls `remove_element` for each
        // rendering counter: the counters are gone, everything else is intact,
        // and nothing is invented. The counters are gone and the values the producer
        let full = b"<Properties><Pages>1</Pages><Company>ACME</Company></Properties>";
        assert_eq!(
            String::from_utf8_lossy(&without_rendering_counters(full)),
            "<Properties><Company>ACME</Company></Properties>"
        );
        let cut = b"<Properties><Pages";
        assert_eq!(
            String::from_utf8_lossy(&without_rendering_counters(cut)),
            "<Properties><Pages"
        );
    }
}
