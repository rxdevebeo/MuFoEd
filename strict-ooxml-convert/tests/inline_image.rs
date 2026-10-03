//! AUD-84 / CORE-QUEUE L-9: an inline PDF image reaches the written package.

#![allow(clippy::doc_markdown)]

use strict_ooxml_convert::{convert, PdfOptions};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_testkit::PdfBuilder;
use strict_ooxml_write::{write_package, WriteOptions};

fn pdf_with_hostile_inline() -> Vec<u8> {
    let garbage = b"re f cm BT ETTj";
    assert_eq!(garbage.len(), 15);
    let mut content = Vec::new();
    content.extend_from_slice(b"q 100 0 0 100 50 50 cm BI /W 5 /H 1 /BPC 8 /CS /RGB ID ");
    content.extend_from_slice(garbage);
    content.extend_from_slice(b" EI Q");
    let mut pdf = PdfBuilder::new();
    pdf.page(&content);
    pdf.build()
}

#[test]
fn inline_image_reaches_written_media() {
    let mut document =
        PdfDocument::open(&pdf_with_hostile_inline(), PdfLimits::default()).expect("open");
    let converted = convert(&mut document, &PdfOptions::default()).expect("convert");
    assert_eq!(
        converted.media.len(),
        1,
        "converter must emit one media part for the inline picture"
    );
    let (part, bytes) = &converted.media[0];
    assert!(
        part.as_str().contains("/word/media/"),
        "unexpected media part: {}",
        part.as_str()
    );
    assert!(!bytes.is_empty(), "media bytes must not be empty");

    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written =
        write_package(&converted.document, Some(&bag), &WriteOptions::default()).expect("write");
    let package = Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("open");
    let media = package
        .read_part(&PartId::new(part.as_str()))
        .expect("read media part from package");
    assert_eq!(media.as_slice(), bytes.as_slice());
}
