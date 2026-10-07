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
    // The RM0090 witness lives in the gitignored local corpus; a clean
    // checkout skips loudly, the same as `strict-ooxml-core/tests/docx_corpus.rs`.
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx");
    let witness = std::fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("RM0090 16-23"))
            })
    });
    let Some(path) = witness else {
        eprintln!(
            "SKIP: local corpus document not present: {}/RM0090 16-23*",
            dir.display()
        );
        return;
    };
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
    // Keep the page for inspection. `<manifest>/../target` does not exist when
    // CARGO_TARGET_DIR points elsewhere, so the copy goes to the temp dir.
    let dest = std::env::temp_dir().join("strict-ooxml-rm-table-page.pdf");
    std::fs::write(&dest, &output.bytes).expect("write pdf");
    eprintln!("wrote {}", dest.display());
}
