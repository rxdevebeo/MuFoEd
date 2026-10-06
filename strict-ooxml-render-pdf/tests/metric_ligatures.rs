//! Scalar and shaped CIDs coexist in a shared font subset.
#![allow(clippy::expect_used, clippy::doc_markdown)]
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{layout::Item, place_pages, RenderOptions, TextAdvanceKind};
use strict_ooxml_wml::{parse_document, ParseOptions};
#[test]
fn metric_and_shaped_pdf_preserve_ligatures_and_combining_marks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-text.docx");
    let package = Package::open_path(path, &OpenOptions::default()).unwrap();
    let document = parse_document(&package, &ParseOptions::default()).unwrap();
    let options = RenderOptions::default();
    let mut pages = place_pages(&document, &options, None).unwrap();
    pages.truncate(1);
    let mut text = pages[0]
        .items
        .iter()
        .find_map(|i| {
            if let Item::Text(t) = i {
                Some(t.clone())
            } else {
                None
            }
        })
        .unwrap();
    text.text = "office".into();
    text.run.family = "Carlito".into();
    text.run.bold = false;
    text.run.italic = false;
    for kind in [TextAdvanceKind::Shaped, TextAdvanceKind::Metric] {
        text.advance = kind;
        pages[0].items = vec![Item::Text(text.clone())];
        // A second run forces scalar and cluster CIDs into the same face.
        let mut other = text.clone();
        other.advance = TextAdvanceKind::Metric;
        other.text = "ffi".into();
        other.baseline += 30.0;
        pages[0].items.push(Item::Text(other.clone()));
        other.advance = TextAdvanceKind::Shaped;
        other.text = "e\u{301}".into();
        other.baseline += 30.0;
        pages[0].items.push(Item::Text(other));
        let pdf = strict_ooxml_render_pdf::render(&pages, &options).unwrap();
        let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        assert!(
            pdf.report.losses().is_empty(),
            "unexpected PDF loss for {kind:?}: {:?}",
            pdf.report.losses()
        );
        let page = *parsed.get_pages().get(&1).unwrap();
        let content = lopdf::content::Content::decode(&parsed.get_page_content(page)).unwrap();
        let drawn = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tj")
            .count();
        let first_run_glyphs = if kind == TextAdvanceKind::Metric {
            6
        } else {
            4
        };
        assert_eq!(drawn, first_run_glyphs + 4, "glyphs must actually be drawn");
        let cmap = parsed
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter_map(|s| s.decompressed_content().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .find(|s| s.contains("beginbfchar"))
            .unwrap();
        for unicode in ["0066", "0069", "00650301"] {
            assert!(
                cmap.contains(&format!("<{unicode}>")),
                "missing scalar ToUnicode {unicode}"
            );
        }
        if kind == TextAdvanceKind::Shaped {
            assert!(
                cmap.contains("<006600660069>"),
                "missing ffi cluster ToUnicode"
            );
        }
    }
}
