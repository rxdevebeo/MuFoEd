//! A11 / F07: the face that measures text is the face the SVG delivers.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    clippy::useless_format,
    clippy::bool_assert_comparison
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions};
use strict_ooxml_wml::model::Document;

fn package(body: &str) -> Document {
    let entries = [
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ];
    open_bytes(build_docx(&entries)).1
}

fn svg_of(body: &str) -> String {
    let doc = package(body);
    render(&doc, &RenderOptions::default())
        .expect("render")
        .into_iter()
        .next()
        .expect("page")
        .svg
}

fn text_width(body: &str, needle: &str) -> f64 {
    let doc = package(body);
    let placed = place_pages(&doc, &RenderOptions::default(), None).expect("place");
    for page in &placed {
        for item in &page.items {
            let Item::Text(text) = item else {
                continue;
            };
            if text.text.contains(needle) {
                return text.width;
            }
        }
    }
    panic!("missing {needle}");
}

/// A heading face and a code face are both inside the standalone SVG.
#[test]
fn f07_font_resource_travels_with_svg() {
    let body = "\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\"/></w:rPr><w:t>Title</w:t></w:r></w:p>\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/></w:rPr><w:t>code</w:t></w:r></w:p>";
    let svg = svg_of(body);
    assert!(svg.contains("@font-face"), "{svg}");
    assert!(svg.contains("font-family: \"Carlito\""), "{svg}");
    assert!(svg.contains("font-family: \"Cousine\""), "{svg}");
    assert!(svg.contains("data:font/ttf;base64,"), "{svg}");
    assert!(svg.contains("font-family=\"Carlito\""), "{svg}");
    assert!(svg.contains("font-family=\"Cousine\""), "{svg}");
}

/// `w:cs` does not replace the ascii/hAnsi face for Latin or Cyrillic.
#[test]
fn f07_cs_font_does_not_override_cyrillic() {
    let body = "\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Cambria\" w:hAnsi=\"Cambria\" w:cs=\"Segoe UI\"/></w:rPr>\
<w:t>Hello Привет</w:t></w:r></w:p>";
    let svg = svg_of(body);
    assert!(svg.contains("font-family=\"Caladea\""), "{svg}");
    assert!(
        !svg.contains("font-family=\"Carlito\""),
        "Segoe UI must not become the run face: {svg}"
    );
    assert!(svg.contains("@font-face") && svg.contains("font-family: \"Caladea\""));
}

/// An unknown family is measured and delivered as the same bundled face.
#[test]
fn f07_unknown_face_has_consistent_fallback() {
    let unknown = "\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Not A Real Font\" w:hAnsi=\"Not A Real Font\"/></w:rPr>\
<w:t>Abc</w:t></w:r></w:p>";
    let known = "\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\"/></w:rPr>\
<w:t>Abc</w:t></w:r></w:p>";
    let svg = svg_of(unknown);
    assert!(svg.contains("font-family=\"Carlito\""), "{svg}");
    assert!(svg.contains("font-family: \"Carlito\""), "{svg}");
    assert!(!svg.contains("Not A Real Font"), "{svg}");
    let delta = (text_width(unknown, "Abc") - text_width(known, "Abc")).abs();
    assert!(
        delta <= 0.25,
        "unknown and Calibri advances differ by {delta} px"
    );
}

#[test]
fn f07_theme_tab_and_missing_glyph_matrix() {
    let tab = "\
<w:p><w:r><w:t xml:space=\"preserve\">A</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>B</w:t></w:r></w:p>";
    let svg = svg_of(tab);
    assert!(svg.contains(">A<") && svg.contains(">B<"), "{svg}");
    let missing = "\
<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Carlito\" w:hAnsi=\"Carlito\"/></w:rPr>\
<w:t>Abc&#xFFFF;</w:t></w:r></w:p>";
    let with_missing = svg_of(missing);
    assert!(
        with_missing.contains("font-family=\"Carlito\""),
        "missing glyph still uses the measuring face: {with_missing}"
    );
}
