//! Static `Send + Sync` checks required by ТЗ §6.5 (AUD-52).

#![allow(dead_code)]

use std::sync::Arc;

use strict_ooxml::StrictDocument;
use strict_ooxml_core::normalize::report::NormalizationReport;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::Package;
use strict_ooxml_render_pdf::PdfOutput;
use strict_ooxml_render_svg::Page;
use strict_ooxml_report::SupportReport;
use strict_ooxml_wml::model::Document;
use strict_ooxml_write::WriteOutput;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn public_handles_are_send_sync() {
    assert_send_sync::<StrictDocument>();
    assert_send_sync::<Document>();
    assert_send_sync::<Package>();
    assert_send_sync::<SupportReport>();
    assert_send_sync::<NormalizationReport>();
    assert_send_sync::<TransitionalNormalizer>();
    assert_send_sync::<Page>();
    assert_send_sync::<PdfOutput>();
    assert_send_sync::<WriteOutput>();
    // Arc wrappers stay Send+Sync when the inner type is.
    assert_send_sync::<Arc<StrictDocument>>();
}

#[test]
fn render_page_svg_rejects_usize_max() {
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body("<w:p><w:r><w:t>x</w:t></w:r></w:p>")
        .build();
    let doc = strict_ooxml::StrictDocument::open_reader(
        std::io::Cursor::new(bytes),
        &strict_ooxml::OpenOptions::default(),
    )
    .expect("open");
    let err = doc
        .render_page_svg(usize::MAX)
        .expect_err("usize::MAX must not wrap");
    let message = err.to_string();
    assert!(
        message.contains("out of range"),
        "unexpected error: {message}"
    );
}
