//! Independent acceptance control: both geometry policies retain ligature text.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_render_svg::{layout::Item, place_pages, RenderOptions, TextAdvanceKind};
use strict_ooxml_wml::{parse_document, ParseOptions};
#[test]
fn metric_and_shaped_pdf_preserve_office() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict/strict-text.docx");
    let package = Package::open_path(path, &OpenOptions::default()).unwrap();
    let document = parse_document(&package, &ParseOptions::default()).unwrap();
    let options = RenderOptions::default();
    let mut pages = place_pages(&document, &options, None).unwrap();
    pages.truncate(1);
    let mut text = pages[0].items.iter().find_map(|i| if let Item::Text(t) = i {Some(t.clone())} else {None}).unwrap();
    text.text = "office".into();
    text.run.family = "Carlito".into();
    text.run.bold = false;
    text.run.italic = false;
    for kind in [TextAdvanceKind::Shaped, TextAdvanceKind::Metric] {
        text.advance = kind;
        pages[0].items = vec![Item::Text(text.clone())];
        let pdf = strict_ooxml_render_pdf::render(&pages, &options).unwrap();
        let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        let extracted = parsed.extract_text(&[1]).unwrap();
        println!("kind={kind:?} extracted={extracted:?} losses={:?}", pdf.report.losses());
        assert!(pdf.report.losses().is_empty(), "unexpected PDF loss for {kind:?}");
    }
}
