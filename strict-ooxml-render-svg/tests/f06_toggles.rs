//! A10 / F06: direct bold and italic assign a state. They do not XOR.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    clippy::useless_format,
    clippy::bool_assert_comparison
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::font::face_source;
use strict_ooxml_render_svg::style::{compute_paragraph, compute_run};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::Document;

fn package(body: &str, styles: &str) -> Document {
    let styles_xml =
        format!("<?xml version=\"1.0\"?><w:styles xmlns:w=\"{W}\">{styles}</w:styles>");
    let rels = format!(
        "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rIdStyles\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/styles\" Target=\"styles.xml\"/>\
         </Relationships>"
    );
    let entries = [
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
        ("word/styles.xml", styles_xml.into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
    ];
    open_bytes(build_docx(&entries)).1
}

fn svg_of(doc: &Document) -> String {
    render(doc, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

/// `(italic, bold)` of the `<text>` element that contains `needle`.
fn painted(svg: &str, needle: &str) -> (bool, bool) {
    let parsed = roxmltree::Document::parse(svg).expect("svg");
    for node in parsed.descendants() {
        if node.tag_name().name() != "text" {
            continue;
        }
        let value = node.text().unwrap_or("");
        if !value.contains(needle) {
            continue;
        }
        let italic = node.attribute("font-style") == Some("italic");
        let bold = node.attribute("font-weight") == Some("bold");
        return (italic, bold);
    }
    panic!("no text {needle:?} in {svg}");
}

fn run_flags(doc: &Document, index: usize) -> (bool, bool, bool, bool) {
    let Block::Paragraph(para) = &doc.body.blocks[0] else {
        panic!("paragraph");
    };
    let computed_para = compute_paragraph(doc, para);
    let mut seen = 0usize;
    for inline in &para.inlines {
        let Inline::Run(run) = inline else {
            continue;
        };
        if seen == index {
            let computed = compute_run(doc, &computed_para, run);
            return (
                computed.italic,
                computed.bold,
                computed.caps,
                computed.vanish,
            );
        }
        seen += 1;
    }
    panic!("run {index}");
}

/// Paragraph-mark italic/bold plus the same direct flags stay on in SVG and PDF.
#[test]
fn f06_direct_italic_is_not_xor() {
    let body = "\
<w:p><w:pPr><w:rPr><w:b/><w:i/></w:rPr></w:pPr>\
<w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>Hi</w:t></w:r></w:p>";
    let doc = package(body, "");
    let (italic, bold, _, _) = run_flags(&doc, 0);
    assert!(
        italic && bold,
        "computed direct flags assign, they do not XOR"
    );
    let svg = svg_of(&doc);
    assert_eq!(painted(&svg, "Hi"), (true, true), "{svg}");

    // The PDF backend embeds `face_source` for the placed run. Bold-italic must
    // be a different program from bold-upright, or the PDF face would ignore `i`.
    let options = RenderOptions::default();
    let placed = place_pages(&doc, &options, None).expect("place");
    let mut matched = false;
    for page in &placed {
        for item in &page.items {
            let Item::Text(text) = item else {
                continue;
            };
            if !text.text.contains("Hi") {
                continue;
            }
            assert!(text.run.italic && text.run.bold);
            let italic = face_source(&text.run.family, true, true).expect("italic face");
            let upright = face_source(&text.run.family, true, false).expect("upright face");
            assert_ne!(
                italic.data, upright.data,
                "the face the PDF backend embeds for this run is the italic program"
            );
            matched = true;
        }
    }
    assert!(matched, "Hi was not placed");
}

/// A mark formats the default run once. A neighbour with the same `On` stays on;
/// a neighbour with `Off` stays off.
#[test]
fn f06_mark_does_not_flip_a_neighbouring_run() {
    let body = "\
<w:p><w:pPr><w:rPr><w:i/></w:rPr></w:pPr>\
<w:r><w:rPr><w:i w:val=\"0\"/></w:rPr><w:t>Off</w:t></w:r>\
<w:r><w:t>Inh</w:t></w:r>\
<w:r><w:rPr><w:i/></w:rPr><w:t>On</w:t></w:r>\
<w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r></w:p>";
    let doc = package(body, "");
    let svg = svg_of(&doc);
    assert_eq!(painted(&svg, "Off").0, false, "{svg}");
    assert_eq!(painted(&svg, "Inh").0, true, "{svg}");
    assert_eq!(painted(&svg, "On"), (true, false), "{svg}");
    assert_eq!(painted(&svg, "Bold"), (true, true), "{svg}");
    assert!(!run_flags(&doc, 0).0);
    assert!(run_flags(&doc, 1).0);
    assert!(run_flags(&doc, 2).0);
}

/// An empty paragraph mark does not invent italic, and one direct run does not
/// leak onto the next.
#[test]
fn f06_empty_mark_leaves_direct_flags_on_their_run() {
    let body = "\
<w:p><w:pPr><w:rPr/></w:pPr>\
<w:r><w:rPr><w:i/></w:rPr><w:t>Only</w:t></w:r>\
<w:r><w:t>Plain</w:t></w:r></w:p>";
    let doc = package(body, "");
    let svg = svg_of(&doc);
    assert_eq!(painted(&svg, "Only").0, true, "{svg}");
    assert_eq!(painted(&svg, "Plain"), (false, false), "{svg}");
}

/// Style `basedOn` still XORs. Direct `On` after that chain assigns.
#[test]
fn f06_style_chain_xors_and_direct_assigns() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"Base\"><w:rPr><w:i/><w:b/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Child\"><w:basedOn w:val=\"Base\"/>\
<w:rPr><w:i/></w:rPr></w:style>";
    let chained = package(
        "<w:p><w:pPr><w:pStyle w:val=\"Child\"/></w:pPr><w:r><w:t>Chain</w:t></w:r></w:p>",
        styles,
    );
    let svg = svg_of(&chained);
    // Base italic XOR Child italic is off. Base bold is inherited.
    assert_eq!(painted(&svg, "Chain"), (false, true), "{svg}");

    let direct = package(
        "<w:p><w:pPr><w:pStyle w:val=\"Child\"/></w:pPr>\
         <w:r><w:rPr><w:i/></w:rPr><w:t>Direct</w:t></w:r></w:p>",
        styles,
    );
    let svg = svg_of(&direct);
    assert_eq!(painted(&svg, "Direct"), (true, true), "{svg}");
}

/// A character style toggles against the paragraph style. Direct `On` does not
/// toggle again.
#[test]
fn f06_character_style_xor_stops_at_direct_formatting() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"P\"><w:rPr><w:b/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"C\"><w:rPr><w:b/></w:rPr></w:style>";
    let direct = package(
        "<w:p><w:pPr><w:pStyle w:val=\"P\"/></w:pPr>\
         <w:r><w:rPr><w:rStyle w:val=\"C\"/><w:b/></w:rPr><w:t>Keep</w:t></w:r></w:p>",
        styles,
    );
    assert_eq!(painted(&svg_of(&direct), "Keep").1, true);
}

/// Caps and vanish follow the same direct assignment, including the mark.
#[test]
fn f06_caps_and_vanish_assign_direct_values() {
    let body = "\
<w:p><w:pPr><w:rPr><w:caps/><w:vanish/></w:rPr></w:pPr>\
<w:r><w:rPr><w:caps/><w:vanish/></w:rPr><w:t>ab</w:t></w:r>\
<w:r><w:rPr><w:vanish w:val=\"0\"/></w:rPr><w:t>cd</w:t></w:r>\
<w:r><w:rPr><w:caps w:val=\"0\"/><w:vanish w:val=\"0\"/></w:rPr><w:t>ef</w:t></w:r></w:p>";
    let doc = package(body, "");
    assert!(run_flags(&doc, 0).2 && run_flags(&doc, 0).3);
    assert!(run_flags(&doc, 1).2 && !run_flags(&doc, 1).3);
    assert!(!run_flags(&doc, 2).2 && !run_flags(&doc, 2).3);
    let svg = svg_of(&doc);
    assert!(!svg.contains(">ab<") && !svg.contains(">AB<"), "{svg}");
    assert!(svg.contains(">CD<"), "{svg}");
    assert!(svg.contains(">ef<"), "{svg}");
}

/// Latin and Cyrillic share a run's direct italic. `iCs` applies only with a CS face.
#[test]
fn f06_direct_italic_covers_latin_and_cyrillic_and_cs() {
    let body = "\
<w:p>\
<w:r><w:rPr><w:i/></w:rPr><w:t>Hello</w:t></w:r>\
<w:r><w:rPr><w:i/></w:rPr><w:t>Привет</w:t></w:r>\
<w:r><w:rPr><w:i w:val=\"0\"/><w:iCs/><w:rFonts w:cs=\"Segoe UI\"/></w:rPr><w:t>Cs</w:t></w:r>\
<w:r><w:rPr><w:i/><w:iCs w:val=\"0\"/></w:rPr><w:t>Lat</w:t></w:r>\
</w:p>";
    let doc = package(body, "");
    let svg = svg_of(&doc);
    assert_eq!(painted(&svg, "Hello").0, true, "{svg}");
    assert_eq!(painted(&svg, "Привет").0, true, "{svg}");
    assert_eq!(
        painted(&svg, "Cs").0,
        true,
        "iCs assigns italic when a CS face is named: {svg}"
    );
    assert_eq!(
        painted(&svg, "Lat").0,
        true,
        "iCs is ignored without a CS face or rtl: {svg}"
    );
    assert!(run_flags(&doc, 2).0);
    assert!(run_flags(&doc, 3).0);
}

/// A character style's `w:sz` wins over the `basedOn` character style (Clio primers).
#[test]
fn f06_character_style_size_overrides_based_on() {
    let styles = "\
<w:style w:type=\"character\" w:styleId=\"a8\"><w:rPr><w:sz w:val=\"21\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"LucidaSansUnicode4pt0pt0\">\
<w:basedOn w:val=\"a8\"/><w:rPr><w:sz w:val=\"8\"/></w:rPr></w:style>";
    let doc = package(
        "<w:p><w:r><w:rPr><w:rStyle w:val=\"LucidaSansUnicode4pt0pt0\"/></w:rPr>\
<w:t>L16055</w:t></w:r></w:p>",
        styles,
    );
    let para = match &doc.body.blocks[0] {
        Block::Paragraph(para) => para,
        _ => panic!("paragraph"),
    };
    let computed_para = compute_paragraph(&doc, para);
    let Inline::Run(run) = &para.inlines[0] else {
        panic!("run");
    };
    let computed = compute_run(&doc, &computed_para, run);
    assert!(
        (computed.size_pt - 4.0).abs() < 1e-9,
        "character sz=8 is 4pt, got {}",
        computed.size_pt
    );
    let svg = svg_of(&doc);
    assert!(
        svg.contains("font-size=\"5.333\""),
        "4pt at 96 dpi is 5.333px: {svg}"
    );
}

/// Clio witness: primer label `L16055` resolves to 4pt via character style.
#[test]
fn f06_clio_l16055_is_four_point() {
    use std::sync::Arc;
    use strict_ooxml_core::normalize::TransitionalNormalizer;
    use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
    use strict_ooxml_wml::{parse_document, ParseOptions};

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../strict-ooxml-core/tests/docx/",
        "Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx"
    );
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_path(path, &options).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    fn visit_para(document: &Document, para: &strict_ooxml_wml::model::Paragraph) -> Option<f64> {
        for inline in &para.inlines {
            let Inline::Run(run) = inline else {
                continue;
            };
            let text: String = run
                .content
                .iter()
                .filter_map(|content| match content {
                    strict_ooxml_wml::model::RunContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect();
            if !text.contains("L16055") {
                continue;
            }
            let computed = compute_run(document, &compute_paragraph(document, para), run);
            eprintln!(
                "L16055 rStyle={:?} size={} family={} spacing={}",
                run.props.style, computed.size_pt, computed.family, computed.spacing_pt
            );
            if let Some(style_id) = &run.props.style {
                eprintln!("style present={}", document.styles.get(style_id).is_some());
                if let Some(style) = document.styles.get(style_id) {
                    eprintln!("style.size={:?} based_on={:?}", style.run.size, style.based_on);
                }
            }
            return Some(computed.size_pt);
        }
        None
    }
    fn visit_block(document: &Document, block: &Block) -> Option<f64> {
        match block {
            Block::Paragraph(para) => visit_para(document, para),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        for nested in &cell.blocks {
                            if let Some(size) = visit_block(document, nested) {
                                return Some(size);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
    let mut size = None;
    for block in &document.body.blocks {
        if let Some(found) = visit_block(&document, block) {
            size = Some(found);
            break;
        }
    }
    let size = size.expect("L16055 run");
    assert!(
        (size - 4.0).abs() < 1e-9,
        "Clio L16055 must be 4pt from LucidaSansUnicode4pt0pt0, got {size}"
    );
    let _ = package;
}

/// Expanded character spacing is part of the run width (Clio style 130, 13 twips).
#[test]
fn f06_character_spacing_widens_the_run() {
    let tight = package(
        "<w:p><w:r><w:rPr><w:sz w:val=\"16\"/></w:rPr><w:t>AB</w:t></w:r></w:p>",
        "",
    );
    let expanded = package(
        "<w:p><w:r><w:rPr><w:sz w:val=\"16\"/><w:spacing w:val=\"13\"/></w:rPr>\
<w:t>AB</w:t></w:r></w:p>",
        "",
    );
    let gap = |svg: &str| -> f64 {
        let parsed = roxmltree::Document::parse(svg).expect("svg");
        let node = parsed
            .descendants()
            .find(|node| {
                node.tag_name().name() == "text"
                    && node.text().is_some_and(|text| text.contains("AB"))
            })
            .expect("AB");
        let xs: Vec<f64> = node
            .attribute("x")
            .expect("x")
            .split_whitespace()
            .map(|value| value.parse().expect("x"))
            .collect();
        assert_eq!(xs.len(), 2, "{svg}");
        xs[1] - xs[0]
    };
    let extra = gap(&svg_of(&expanded)) - gap(&svg_of(&tight));
    let expected = 13.0 / 20.0 * (10.667 / 8.0);
    assert!(
        (extra - expected).abs() <= 0.05,
        "13 twips of character spacing at 8pt, got {extra} expected {expected}"
    );
}

/// Empty `<w:b/>` on a paragraph style must yield bold default_run (Clio style 50).
///
/// Clio also sets `<w:bCs/>` and `w:cs` on the same style; those must not XOR-cancel
/// Latin bold just because a complex-script face is named.
#[test]
fn f06_empty_b_on_paragraph_style_is_bold() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"a\" w:default=\"1\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"50\" w:customStyle=\"1\">\
<w:basedOn w:val=\"a\"/>\
<w:rPr><w:rFonts w:ascii=\"Times New Roman\" w:hAnsi=\"Times New Roman\" w:cs=\"Times New Roman\"/>\
<w:b/><w:bCs/><w:sz w:val=\"21\"/><w:szCs w:val=\"21\"/></w:rPr>\
</w:style>";
    let body = "\
<w:p><w:pPr><w:pStyle w:val=\"50\"/></w:pPr>\
<w:r><w:t>Figure</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let doc = package(body, styles);
    let Block::Paragraph(para) = &doc.body.blocks[0] else {
        panic!("paragraph");
    };
    let computed = compute_paragraph(&doc, para);
    assert!(
        computed.default_run.bold,
        "style 50 empty w:b must set default_run.bold (bCs+cs must not cancel)"
    );
    let Inline::Run(run) = &para.inlines[0] else {
        panic!("run");
    };
    let cr = compute_run(&doc, &computed, run);
    assert!(cr.bold, "run must inherit style bold");
    let svg = svg_of(&doc);
    assert!(
        svg.contains("font-weight=\"bold\""),
        "SVG must paint bold: {svg}"
    );
}
