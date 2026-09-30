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
    // from scratch with the office-document relationship and nothing else, so
    // `docProps/core.xml` and `docProps/app.xml` are not in the output. They are
    // metadata *about* the document rather than part of it, and copying a stale
    // `dcterms:modified` would be a claim this writer cannot support - but
    // dropping them in silence is not an option either, so each is named.
    for info in source.relationships(&PartId::new("/")) {
        if matches!(info.rel_type, RelType::OfficeDocument) {
            continue;
        }
        ctx.report_unsupported(
            "W7.package-properties",
            &format!(
                "the package declares {} ({:?}), which this writer does not produce",
                info.target, info.rel_type
            ),
            &SourceLocation::unknown(),
        );
    }
    out
}

/// The extension every relationship part carries, lowercased for the comparison.
const CONTENT_TYPE_RELS_SUFFIX: &str = ".rels";

/// The OPC relationship namespace a Transitional producer writes.
const TRANSITIONAL_RELS_NS: &[u8] =
    b"xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"";

/// The Strict one this project writes everywhere else.
const STRICT_RELS_NS: &[u8] = b"xmlns=\"http://purl.oclc.org/ooxml/package/relationships\"";

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
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = find(rest, TRANSITIONAL_RELS_NS) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(STRICT_RELS_NS);
        rest = &rest[at + TRANSITIONAL_RELS_NS.len()..];
    }
    out.extend_from_slice(rest);
    out
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
    use super::{referenced_ids, rels_part_of, resolve};
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
}
