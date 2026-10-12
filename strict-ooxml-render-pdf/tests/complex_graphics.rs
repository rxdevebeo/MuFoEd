//! An independent PDF parser checks native fill resources and clipping.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::doc_markdown
)]
use std::io::Cursor;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_testkit::graphics::{fill_docx, pattern, GRADIENT};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn pdf(fill: &str) -> Vec<u8> {
    let package = Package::open_reader(
        Cursor::new(fill_docx("ellipse", fill)),
        &OpenOptions::default(),
    )
    .expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let pages = place_pages(&document, &options, Some(&package)).expect("place");
    strict_ooxml_render_pdf::render_with_source(&pages, &options, Some(&package))
        .expect("PDF")
        .bytes
}

#[test]
fn gradient_is_a_resolved_native_shading_clipped_by_the_shape() {
    let bytes = pdf(GRADIENT);
    assert_eq!(bytes, pdf(GRADIENT), "deterministic output");
    let document = lopdf::Document::load_mem(&bytes).expect("independent parser");
    let page = *document.get_pages().values().next().expect("page");
    let resources = document
        .get_dictionary(page)
        .expect("page dict")
        .get(b"Resources")
        .expect("resources")
        .as_dict()
        .expect("dict");
    let shadings = resources
        .get(b"Shading")
        .expect("shadings")
        .as_dict()
        .expect("dict");
    assert_eq!(shadings.len(), 1);
    let (_, reference) = shadings.iter().next().expect("shading");
    let shading = document
        .get_dictionary(reference.as_reference().expect("ref"))
        .expect("shading dict");
    assert_eq!(
        shading
            .get(b"ShadingType")
            .expect("type")
            .as_i64()
            .expect("int"),
        2
    );
    let function = shading
        .get(b"Function")
        .expect("function")
        .as_dict()
        .expect("dict");
    assert_eq!(
        function
            .get(b"Functions")
            .expect("functions")
            .as_array()
            .expect("array")
            .len(),
        2
    );
    let content = document.get_page_content(page);
    let ops = lopdf::content::Content::decode(&content).expect("operators");
    let clip = ops
        .operations
        .iter()
        .position(|op| op.operator == "W")
        .expect("clip");
    let shade = ops
        .operations
        .iter()
        .position(|op| op.operator == "sh")
        .expect("shading paint");
    assert!(clip < shade);
}

#[test]
fn patterned_shape_uses_a_coloured_tiling_pattern_with_both_colours() {
    let document = lopdf::Document::load_mem(&pdf(&pattern("cross"))).expect("independent parser");
    let page = *document.get_pages().values().next().expect("page");
    let resources = document
        .get_dictionary(page)
        .expect("page dict")
        .get(b"Resources")
        .expect("resources")
        .as_dict()
        .expect("dict");
    let patterns = resources
        .get(b"Pattern")
        .expect("patterns")
        .as_dict()
        .expect("dict");
    let (_, reference) = patterns.iter().next().expect("pattern");
    let stream = document
        .get_object(reference.as_reference().expect("ref"))
        .expect("object")
        .as_stream()
        .expect("stream");
    assert_eq!(
        stream
            .dict
            .get(b"PatternType")
            .expect("type")
            .as_i64()
            .expect("int"),
        1
    );
    let content = lopdf::content::Content::decode(&stream.content).expect("tile operators");
    let fills: Vec<_> = content
        .operations
        .iter()
        .filter(|op| op.operator == "rg")
        .collect();
    assert_eq!(fills.len(), 2, "both background and foreground are drawn");
    assert_ne!(fills[0].operands, fills[1].operands);
}
