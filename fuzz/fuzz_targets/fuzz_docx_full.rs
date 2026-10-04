#![no_main]

//! AUD-92: open (Normalize) → support_report → render_svg → write → reopen must
//! not panic; a successful write must reopen as Ok.

use std::io::Cursor;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use strict_ooxml::{
    write_package, ConformancePolicy, OpenOptions, PageSelection, RenderOptions, ResourceLimits,
    StrictDocument, TransitionalNormalizer, WriteOptions,
};

fuzz_target!(|data: &[u8]| {
    let limits = ResourceLimits {
        max_single_uncompressed: 1 << 20,
        max_total_uncompressed: 4 << 20,
        max_xml_depth: 64,
        max_xml_attributes_per_elem: 64,
        max_text_len: 1 << 20,
        max_block_nesting: 8,
        max_text_box_nesting: 3,
        max_math_nodes: 512,
        max_math_depth: 32,
        max_support_features: 1_000,
        max_render_items: 50_000,
        max_pages: 3,
        ..ResourceLimits::default()
    };
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .limits(limits)
        .shared_normalization(normalizer);
    let Ok(doc) = StrictDocument::open_reader(Cursor::new(data), &options) else {
        return;
    };
    let _ = doc.support_report();
    let render_opts = RenderOptions {
        pages: PageSelection::Range { start: 1, end: 3 },
        limits,
        ..RenderOptions::default()
    };
    let _ = doc.render_svg(&render_opts);
    let Ok(written) = write_package(doc.document(), Some(doc.package()), &WriteOptions::default())
    else {
        return;
    };
    let reopen = StrictDocument::open_reader(Cursor::new(written.bytes), &options);
    assert!(
        reopen.is_ok(),
        "write_package Ok bytes must reopen under Normalize"
    );
});
