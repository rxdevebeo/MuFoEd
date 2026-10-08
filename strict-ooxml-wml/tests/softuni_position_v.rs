//! SoftUni `positionV` wrapped in `mc:AlternateContent` (`wp14` pct vs EMU).

use std::sync::Arc;

use strict_ooxml_core::normalize::transitional::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::DrawingKind;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn softuni_preserves_page_percent_position() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../strict-ooxml-core/tests/docx/4. Complex-Conditions.docx"
    );
    // The corpus document is gitignored (local only); a checkout without it
    // has nothing to check here.
    if !std::path::Path::new(path).is_file() {
        eprintln!("SKIP softuni_preserves_page_percent_position: {path} is absent");
        return;
    }
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_path(path, &options).expect("open");
    let doc = parse_document(&package, &ParseOptions::default()).expect("parse");
    let mut out = Vec::new();
    let mut opaque = 0usize;
    let mut drawings = 0usize;
    for block in &doc.body.blocks {
        walk_count(block, &mut out, &mut opaque, &mut drawings);
    }
    assert!(
        out.iter().any(|line| {
            line.contains("id=Some(149)")
                && (line.contains("pct=Some(2300)") || line.contains("off=Some(201930)"))
        }),
        "Group 149 lost page position: {out:?}"
    );
}

fn walk_count(block: &Block, out: &mut Vec<String>, opaque: &mut usize, drawings: &mut usize) {
    match block {
        Block::Paragraph(paragraph) => {
            for inline in &paragraph.inlines {
                walk_inline_count(inline, out, opaque, drawings);
            }
        }
        Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for nested in &cell.blocks {
                        walk_count(nested, out, opaque, drawings);
                    }
                }
            }
        }
        Block::SdtBlock(sdt) => {
            for nested in &sdt.blocks {
                walk_count(nested, out, opaque, drawings);
            }
            for inline in &sdt.inlines {
                walk_inline_count(inline, out, opaque, drawings);
            }
        }
        Block::Opaque(_) | Block::AltChunk(_) => {}
    }
}

fn walk_inline_count(
    inline: &Inline,
    out: &mut Vec<String>,
    opaque: &mut usize,
    drawings: &mut usize,
) {
    match inline {
        Inline::Drawing(drawing) => {
            *drawings += 1;
            match &drawing.kind {
                DrawingKind::Anchor(anchor) => {
                    let id = anchor.doc_pr.as_ref().and_then(|pr| pr.id);
                    let position = anchor.position_v.as_ref();
                    out.push(format!(
                        "id={:?} rel={:?} off={:?} pct={:?}",
                        id,
                        position.and_then(|p| p.relative_from.as_deref()),
                        position.and_then(|p| p.offset.map(|emu| emu.0)),
                        position.and_then(|p| p.percent_offset),
                    ));
                }
                DrawingKind::Inline(_) => out.push("inline-drawing".into()),
                DrawingKind::Opaque(o) => {
                    *opaque += 1;
                    out.push(format!("opaque-drawing local={}", o.local));
                }
            }
        }
        Inline::Hyperlink(hyperlink) => {
            for child in &hyperlink.inlines {
                walk_inline_count(child, out, opaque, drawings);
            }
        }
        Inline::SdtInline(sdt) => {
            for child in &sdt.inlines {
                walk_inline_count(child, out, opaque, drawings);
            }
        }
        Inline::Directional(directional) => {
            for child in &directional.inlines {
                walk_inline_count(child, out, opaque, drawings);
            }
        }
        Inline::Field(field) => {
            for child in &field.inlines {
                walk_inline_count(child, out, opaque, drawings);
            }
        }
        Inline::Run(run) => {
            for content in &run.content {
                if let strict_ooxml_wml::model::inline::RunContent::Drawing(drawing) = content {
                    *drawings += 1;
                    match &drawing.kind {
                        DrawingKind::Anchor(anchor) => {
                            let id = anchor.doc_pr.as_ref().and_then(|pr| pr.id);
                            let position = anchor.position_v.as_ref();
                            out.push(format!(
                                "id={:?} rel={:?} off={:?} pct={:?}",
                                id,
                                position.and_then(|p| p.relative_from.as_deref()),
                                position.and_then(|p| p.offset.map(|emu| emu.0)),
                                position.and_then(|p| p.percent_offset),
                            ));
                        }
                        DrawingKind::Inline(_) => out.push("inline-drawing".into()),
                        DrawingKind::Opaque(o) => {
                            *opaque += 1;
                            out.push(format!("opaque-drawing local={}", o.local));
                        }
                    }
                }
            }
        }
        Inline::Opaque(_) => *opaque += 1,
        _ => {}
    }
}

/// The same construct on a CC0 document (`docs/CC0_CORPUS_MIGRATION_PLAN.md`
/// §6), so it runs in CI: `CC0_DOCX_1/076` anchors text box `docPr id=32` with
/// `<wp:positionV relativeFrom="page">` whose `mc:Choice` is
/// `<wp14:pctPosVOffset>88000` and whose `mc:Fallback` is
/// `<wp:posOffset>8851265` (read from `word/document.xml` of that file).
#[test]
fn cc0_preserves_page_percent_position() {
    let doc = strict_ooxml_testkit::corpus_doc!("cc0-docx-1/076");
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_path(&doc.path, &options).expect("open");
    let parsed = parse_document(&package, &ParseOptions::default()).expect("parse");
    let mut out = Vec::new();
    let mut opaque = 0usize;
    let mut drawings = 0usize;
    for block in &parsed.body.blocks {
        walk_count(block, &mut out, &mut opaque, &mut drawings);
    }
    assert!(
        out.iter().any(|line| {
            line.contains("id=Some(32)")
                && line.contains("rel=Some(\"page\")")
                && (line.contains("pct=Some(88000)") || line.contains("off=Some(8851265)"))
        }),
        "text box 32 lost its page position: {out:?}"
    );
}
