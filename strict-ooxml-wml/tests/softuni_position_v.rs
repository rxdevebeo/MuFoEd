//! SoftUni `positionV` wrapped in `mc:AlternateContent` (wp14 pct vs EMU).

use std::sync::Arc;

use strict_ooxml_core::normalize::transitional::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
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
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_path(path, &options).expect("open");
    let raw = package
        .read_part(&PartId::new("/word/document.xml"))
        .expect("read");
    let text = String::from_utf8_lossy(&raw);
    eprintln!(
        "normalized has pctPosVOffset={} page={}",
        text.contains("pctPosVOffset"),
        text.matches("relativeFrom=\"page\"").count()
    );
    if let Some(idx) = text.find("pctPosVOffset") {
        let start = idx.saturating_sub(180);
        let end = (idx + 180).min(text.len());
        eprintln!("XML snippet:\n{}", &text[start..end]);
    } else if let Some(i) = text.find("positionV") {
        let end = (i + 280).min(text.len());
        eprintln!("positionV snippet:\n{}", &text[i..end]);
    }

    let doc = parse_document(&package, &ParseOptions::default()).expect("parse");
    eprintln!("body blocks={}", doc.body.blocks.len());
    let mut out = Vec::new();
    let mut opaque = 0usize;
    let mut drawings = 0usize;
    for block in &doc.body.blocks {
        walk_count(block, &mut out, &mut opaque, &mut drawings);
    }
    eprintln!("anchors found={} drawings={drawings} opaque_inlines={opaque}", out.len());
    for line in &out {
        eprintln!("{line}");
    }
    // Also show every pctPos occurrence neighborhood in normalized XML.
    let mut search_from = 0usize;
    let mut n = 0usize;
    while let Some(rel) = text[search_from..].find("pctPosVOffset") {
        let idx = search_from + rel;
        n += 1;
        let start = idx.saturating_sub(120);
        let end = (idx + 80).min(text.len());
        eprintln!("pct#{n} @{idx}: ...{}...", &text[start..end].replace('\n', " "));
        search_from = idx + 12;
    }
    assert!(
        out.iter().any(|line| {
            line.contains("id=Some(149)")
                && (line.contains("pct=Some(2300)") || line.contains("off=Some(201930)"))
        }),
        "Group 149 lost page position: {out:?}"
    );
}

fn walk(block: &Block, out: &mut Vec<String>) {
    let mut opaque = 0;
    let mut drawings = 0;
    walk_count(block, out, &mut opaque, &mut drawings);
}

fn walk_count(
    block: &Block,
    out: &mut Vec<String>,
    opaque: &mut usize,
    drawings: &mut usize,
) {
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
        Block::Opaque(_) => {}
        _ => {}
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

fn walk_inline(inline: &Inline, out: &mut Vec<String>) {
    let mut opaque = 0;
    let mut drawings = 0;
    walk_inline_count(inline, out, &mut opaque, &mut drawings);
}
