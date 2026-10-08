//! Loss accounting distinguishes a renamed byte copy from an omitted source part.
//!
//! Since P10 (binary media identity, 7ea3fd9) the writer carries every source
//! binary resource whose digest the output does not already hold, so an
//! unreferenced image is kept rather than omitted. The omitted part here is
//! therefore an unreferenced XML part, which no pass copies.
use strict_ooxml_core::{
    opc::{OpenOptions, Package},
    part::PartId,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{
    model::{MediaIndex, MediaItem, MediaKind},
    parse_document, ParseOptions,
};
use strict_ooxml_write::{write_package, WriteOptions};

#[test]
fn copied_media_is_not_lost_but_an_omitted_part_is_reported() {
    let bytes = DocxBuilder::strict()
        .body("<w:p/>")
        .part("word/media/custom-name.png", b"preserved bytes".to_vec())
        .part("word/media/orphan.png", b"unreferenced bytes".to_vec())
        .part("word/orphan.xml", b"<orphan/>".to_vec())
        .content_type("/word/media/custom-name.png", "image/png")
        .content_type("/word/media/orphan.png", "image/png")
        .content_type("/word/orphan.xml", "application/xml")
        .build();
    let source = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut document = parse_document(&source, &ParseOptions::default()).unwrap();
    document.media = MediaIndex::default();
    document.media.insert(MediaItem {
        part: PartId::new("/word/media/custom-name.png"),
        kind: MediaKind::Png,
        content_type: Some("image/png".into()),
    });
    let result = write_package(&document, Some(&source), &WriteOptions::default()).unwrap();
    let output = Package::open_reader(&result.bytes[..], &OpenOptions::default()).unwrap();
    assert_eq!(
        output
            .read_part(&PartId::new("/word/media/image1.png"))
            .unwrap(),
        b"preserved bytes"
    );
    // P10: unreferenced binary bytes are carried under their own name.
    assert_eq!(
        output
            .read_part(&PartId::new("/word/media/orphan.png"))
            .unwrap(),
        b"unreferenced bytes"
    );
    let losses: Vec<_> = result
        .report
        .losses()
        .into_iter()
        .filter(|loss| loss.feature_id == "W7.dropped-part")
        .collect();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].reason.contains("orphan.xml"));
    assert!(!losses[0].reason.contains("orphan.png"));
    assert!(!losses[0].reason.contains("custom-name.png"));
}
