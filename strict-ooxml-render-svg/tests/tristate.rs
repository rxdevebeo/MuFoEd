//! AUD-44: vanish and XOR toggle rendering.

#![allow(
    clippy::expect_used,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    clippy::useless_format
)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels, W};
use strict_ooxml_render_svg::{render, RenderOptions};

fn svg_for(body: &str, styles: &str) -> String {
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
    let (_package, parsed) = open_bytes(build_docx(&entries));
    render(&parsed, &RenderOptions::default()).expect("render")[0]
        .svg
        .clone()
}

#[test]
fn vanished_text_is_not_drawn_unless_explicitly_turned_off() {
    let styles = "\
<w:style w:type=\"character\" w:styleId=\"Hidden\"><w:rPr><w:vanish/></w:rPr></w:style>";
    let hidden = svg_for(
        "<w:p><w:r><w:rPr><w:rStyle w:val=\"Hidden\"/></w:rPr><w:t>secret</w:t></w:r></w:p>",
        styles,
    );
    assert!(!hidden.contains(">secret<"), "{hidden}");

    let shown = svg_for(
        "<w:p><w:r><w:rPr><w:rStyle w:val=\"Hidden\"/><w:vanish w:val=\"0\"/></w:rPr>\
         <w:t>secret</w:t></w:r></w:p>",
        styles,
    );
    assert!(shown.contains(">secret<"), "{shown}");
}

#[test]
fn paragraph_and_character_bold_xor_to_not_bold() {
    let styles = "\
<w:style w:type=\"paragraph\" w:styleId=\"P\"><w:rPr><w:b/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"C\"><w:rPr><w:b/></w:rPr></w:style>";
    let svg = svg_for(
        "<w:p><w:pPr><w:pStyle w:val=\"P\"/></w:pPr>\
         <w:r><w:rPr><w:rStyle w:val=\"C\"/></w:rPr><w:t>plain</w:t></w:r></w:p>",
        styles,
    );
    // Bold text is emitted with font-weight=\"bold\"; XOR leaves it regular.
    assert!(svg.contains(">plain<"), "{svg}");
    assert!(
        !svg.contains("font-weight=\"bold\""),
        "XOR should cancel bold: {svg}"
    );
}
