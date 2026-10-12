//! Output budget must keep complete pages and report omitted output.
use std::io::Cursor;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{render_with_media, RenderOptions};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn svg_budget_preserves_complete_first_page_and_warns() {
    let bytes = DocxBuilder::strict()
        .body(r#"<w:p><w:r><w:t>first</w:t><w:br w:type="page"/><w:t>second</w:t></w:r></w:p>"#)
        .build();
    let package = Package::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let complete = render_with_media(&document, &RenderOptions::default(), Some(&package))
        .expect("full render");
    assert_eq!(complete.len(), 2);
    let first = complete.first().expect("first page");
    let options = RenderOptions {
        max_svg_bytes: first.svg.len() + 1,
        ..RenderOptions::default()
    };
    let limited = render_with_media(&document, &options, Some(&package)).expect("partial render");
    assert_eq!(limited.len(), 1);
    assert_eq!(limited.first().expect("page").svg, first.svg);
    assert!(limited
        .first()
        .expect("page")
        .warnings
        .iter()
        .any(|warning| warning.contains("render.output-limit")));
    let options = RenderOptions {
        max_svg_bytes: 0,
        ..RenderOptions::default()
    };
    assert!(render_with_media(&document, &options, Some(&package)).is_err());
}
