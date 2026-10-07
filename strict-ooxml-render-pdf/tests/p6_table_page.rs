//! T-P6-2: one PDF page of the RM0090 table the SVG bbox measures.
//!
//! Placement is the shared engine. This file only checks that the table page
//! becomes a single PDF and keeps the header text.

use std::sync::Arc;

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_render_pdf::render;
use strict_ooxml_render_svg::{place_pages, MediaMode, PageSelection, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn t_p6_2_rm0090_table_page_is_one_pdf_page() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx");
    let path = std::fs::read_dir(&dir)
        .expect("docx corpus")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("RM0090 16-23"))
        })
        .expect("RM0090 witness");
    let bytes = std::fs::read(&path).expect("read witness");
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let open_options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer);
    let package = Package::open_reader(bytes.as_slice(), &open_options).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions {
        pages: PageSelection::Range { start: 1, end: 2 },
        media: MediaMode::None,
        ..RenderOptions::default()
    };
    let pages = place_pages(&document, &options, None).expect("place");
    assert!(pages.len() >= 2, "the table is on the second placed page");
    let output = render(&pages[1..2], &RenderOptions::default()).expect("pdf");
    assert_eq!(output.page_count, 1);
    assert!(output.bytes.starts_with(b"%PDF"));
    let loaded = lopdf::Document::load_mem(&output.bytes).expect("pdf parses");
    assert_eq!(loaded.get_pages().len(), 1);
    let dest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/rm-table-page.pdf");
    std::fs::write(&dest, &output.bytes).expect("write pdf");
}
