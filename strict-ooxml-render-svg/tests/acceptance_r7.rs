//! Acceptance of the paths added in 55f1584 that had no fixture of their own
//! (`docs/PARSE-RENDER-GAPS-2026-10-11.md`, R7): embedded font programs,
//! custom geometry guides and arcs, and positioned (floating) tables.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::float_cmp)]

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::font::face_source;
use strict_ooxml_render_svg::{place_pages, render_with_media, Item, PlacedPage, RenderOptions};
use strict_ooxml_testkit::docx::DocxBuilder;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

fn open(builder: &DocxBuilder) -> (Package, Document) {
    let bytes = builder.build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    (package, document)
}

fn placed(builder: &DocxBuilder) -> PlacedPage {
    let (package, document) = open(builder);
    place_pages(&document, &RenderOptions::default(), Some(&package))
        .expect("place")
        .remove(0)
}

/// ECMA-376 Part 1 §17.8.1 obfuscation: the first 32 bytes XOR the key bytes
/// in reverse order.
fn obfuscate(bytes: &[u8], key_hex: &str) -> Vec<u8> {
    let key: Vec<u8> = (0..16)
        .map(|index| u8::from_str_radix(&key_hex[index * 2..index * 2 + 2], 16).expect("hex"))
        .collect();
    let mut out = bytes.to_vec();
    for (index, byte) in out.iter_mut().take(32).enumerate() {
        *byte ^= key[15 - index % 16];
    }
    out
}

/// An embedded, obfuscated font is deobfuscated, measured with the Windows
/// line box and delivered to the SVG as its own resource.
#[test]
fn an_embedded_font_is_used_for_layout_and_paint() {
    let program = face_source("Calibri", false, false).expect("Carlito").data;
    let key = "00112233445566778899AABBCCDDEEFF";
    let builder = DocxBuilder::strict()
        .body(
            "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Embedded Test\" w:hAnsi=\"Embedded Test\"/></w:rPr>\
             <w:t>Embedded</w:t></w:r></w:p>",
        )
        .rel("rIdFonts", "fontTable", "fontTable.xml")
        .part_xml(
            "word/fontTable.xml",
            "w:fonts",
            "<w:font w:name=\"Embedded Test\"><w:embedRegular r:id=\"rIdF1\" \
             w:fontKey=\"{00112233-4455-6677-8899-AABBCCDDEEFF}\"/></w:font>",
        )
        .content_type(
            "/word/fontTable.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml",
        )
        .part(
            "word/_rels/fontTable.xml.rels",
            "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rIdF1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/font\" \
             Target=\"fonts/font1.odttf\"/></Relationships>",
        )
        .part("word/fonts/font1.odttf", obfuscate(program, key))
        .content_type(
            "/word/fonts/font1.odttf",
            "application/vnd.openxmlformats-officedocument.obfuscatedFont",
        );
    let (package, document) = open(&builder);
    let pages =
        render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render");
    let svg = &pages[0].svg;
    assert!(svg.contains("font-family=\"OOXML-"), "{svg:.400}");
    assert!(
        svg.contains("@font-face"),
        "the program travels with the SVG"
    );
    assert!(
        pages.iter().all(|page| page
            .warnings
            .iter()
            .all(|w| !w.starts_with("font.embedded"))),
        "{:?}",
        pages[0].warnings
    );
}

/// Guides (`a:gd`) feed a custom path, and `a:arcTo` is drawn inside the
/// shape's box.
#[test]
fn custom_geometry_guides_and_arcs_stay_in_the_box() {
    let shape = "<wps:wsp><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
         <a:custGeom><a:avLst/><a:gdLst><a:gd name=\"half\" fmla=\"*/ w 1 2\"/></a:gdLst><a:ahLst/><a:cxnLst/>\
         <a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"100\" h=\"100\">\
         <a:moveTo><a:pt x=\"0\" y=\"50\"/></a:moveTo>\
         <a:arcTo wR=\"50\" hR=\"50\" stAng=\"10800000\" swAng=\"10800000\"/>\
         <a:lnTo><a:pt x=\"half\" y=\"100\"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom>\
         <a:ln><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln></wps:spPr><wps:bodyPr/></wps:wsp>";
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/>\
         <wp:docPr id=\"1\" name=\"shape\"/><a:graphic>\
         <a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/wordprocessingShape\">\
         {shape}</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let page = placed(&DocxBuilder::strict().body(&body));
    let paths: Vec<_> = page
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Path(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1, "{:?}", page.items);
    let path = &paths[0];
    assert!(
        path.d.contains('A') || path.d.contains('C'),
        "the arc is a curve: {}",
        path.d
    );
    let numbers: Vec<f64> = path
        .d
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
        .filter_map(|value| value.parse().ok())
        .collect();
    assert!(
        numbers.iter().all(|value| (-0.5..=96.5).contains(value)),
        "inside the 96 px box: {}",
        path.d
    );
    // The guide `half` puts the line's end at the middle of the width.
    assert!(
        path.d.contains(" 48 96") || path.d.contains("48 96"),
        "{}",
        path.d
    );
}

/// `w:tblpPr` with `vertAnchor="page"` places the table at its absolute
/// position, out of the text flow.
#[test]
fn a_positioned_table_sits_at_its_page_coordinates() {
    let body = "<w:p><w:r><w:t>Flow</w:t></w:r></w:p>\
        <w:tbl><w:tblPr><w:tblpPr w:vertAnchor=\"page\" w:horzAnchor=\"page\" w:tblpX=\"2880\" w:tblpY=\"7200\"/>\
        <w:tblW w:w=\"2880\" w:type=\"dxa\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"2880\"/></w:tblGrid>\
        <w:tr><w:tc><w:tcPr><w:tcW w:w=\"2880\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
        <w:p><w:r><w:t>After</w:t></w:r></w:p>";
    let page = placed(&DocxBuilder::strict().body(body));
    let text_at = |needle: &str| {
        page.items
            .iter()
            .find_map(|item| match item {
                Item::Text(text) if text.text.contains(needle) => Some((text.x, text.baseline)),
                _ => None,
            })
            .expect(needle)
    };
    let (cell_x, cell_y) = text_at("Cell");
    let (_, flow_y) = text_at("Flow");
    let (_, after_y) = text_at("After");
    // 2880 twips = 192 px across, 7200 twips = 480 px down, plus the cell's
    // left margin and the text ascent.
    assert!((cell_x - 192.0).abs() < 12.0, "x {cell_x}");
    assert!(cell_y > 480.0 && cell_y < 520.0, "y {cell_y}");
    // The following paragraph is not pushed below the table.
    assert!(
        after_y < cell_y,
        "after {after_y} vs cell {cell_y}, flow {flow_y}"
    );
}
