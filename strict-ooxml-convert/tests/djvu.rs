//! `DjVu` regression tests using deterministic native fixtures.
#![allow(clippy::float_cmp)]
#![cfg(feature = "djvu")]
#[path = "../../strict-ooxml-djvu/tests/common/mod.rs"]
mod common;
use common::*;
use strict_ooxml_convert::{convert_djvu, DjvuOptions, Mode};
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
fn convert(bytes: &[u8], options: &DjvuOptions) -> strict_ooxml_convert::Converted {
    let mut d = DjvuDocument::open(bytes, DjvuLimits::default()).expect("reader");
    convert_djvu(&mut d, options).expect("convert")
}
fn xml(c: &strict_ooxml_convert::Converted) -> String {
    parts(c).0
}

/// The written `document.xml` and `document.xml.rels`.
fn parts(c: &strict_ooxml_convert::Converted) -> (String, String) {
    let mut media = strict_ooxml_write::package::MediaBag::new();
    for (id, bytes) in &c.media {
        media.insert(id.clone(), bytes.clone());
    }
    let written = strict_ooxml_write::write_package(
        &c.document,
        Some(&media),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .expect("write");
    let p = strict_ooxml_core::opc::Package::open_reader(
        written.bytes.as_slice(),
        &strict_ooxml_core::opc::OpenOptions::default(),
    )
    .expect("Strict package");
    let read = |name: &str| {
        p.read_part(&strict_ooxml_core::part::PartId::new(name))
            .map(|bytes| String::from_utf8(bytes).expect("UTF-8"))
            .unwrap_or_default()
    };
    (
        read("/word/document.xml"),
        read("/word/_rels/document.xml.rels"),
    )
}
#[test]
fn native_text_reaches_strict_xml_without_loss() {
    let c = convert(
        &text_file("Привет 世界 café", true),
        &DjvuOptions::default(),
    );
    let xml = xml(&c);
    let dom = roxmltree::Document::parse(&xml).expect("independent XML oracle");
    let texts: Vec<_> = dom
        .descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect();
    assert_eq!(texts, vec!["Привет 世界 café"]);
    assert!(dom
        .descendants()
        .any(|n| n.tag_name().namespace()
            == Some("http://purl.oclc.org/ooxml/wordprocessingml/main")));
}
#[test]
fn conversion_is_deterministic_with_no_recognition() {
    let bytes = text_file("same", true);
    assert_eq!(
        xml(&convert(&bytes, &DjvuOptions::default())),
        xml(&convert(&bytes, &DjvuOptions::default()))
    );
}
#[test]
fn visual_lines_carry_estimated_geometry_without_fake_glyphs() {
    let c = convert(
        &text_file("line", false),
        &DjvuOptions::default().mode(Mode::Visual),
    );
    let xml = xml(&c);
    assert!(xml.contains(r#"lineRule="exact""#));
    assert!(c
        .report
        .losses()
        .iter()
        .any(|l| l.id == "djvu.visual-placement"));
}
#[test]
fn page_range_is_strictly_checked() {
    let mut doc = DjvuDocument::open(&text_file("one", false), DjvuLimits::default()).expect("one");
    for (a, b) in [(0, 1), (2, 2), (1, 2), (2, 1)] {
        assert!(convert_djvu(&mut doc, &DjvuOptions::default().pages(a, b)).is_err());
    }
}
#[test]
fn mixed_page_sizes_and_section_boundaries_are_written() {
    let a = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("one")),
        ],
    );
    let b = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(200, 100, 72)),
            chunk(b"TXTa", &text("two")),
        ],
    );
    let c = convert(&bundled(&[a, b]), &DjvuOptions::default());
    let xml = xml(&c);
    let dom = roxmltree::Document::parse(&xml).expect("xml");
    assert_eq!(
        dom.descendants()
            .filter(|n| n.tag_name().name() == "sectPr")
            .count(),
        2
    );
}
#[cfg(feature = "djvu-image")]
mod recognition {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use strict_ooxml_ocr::{Image, Recovered, TextRecovery, VisionError};
    struct Mock {
        calls: AtomicUsize,
        answer: Result<Option<Recovered>, VisionError>,
    }
    impl TextRecovery for Mock {
        fn model_name(&self) -> &'static str {
            "mock"
        }
        fn recover_page(&self, image: &Image) -> Result<Option<Recovered>, VisionError> {
            assert_eq!(&image.bytes[..8], b"\x89PNG\r\n\x1a\n");
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer.clone()
        }
    }
    fn provider(answer: Result<Option<Recovered>, VisionError>) -> Arc<Mock> {
        Arc::new(Mock {
            calls: AtomicUsize::new(0),
            answer,
        })
    }
    fn success() -> Arc<Mock> {
        provider(Ok(Some(Recovered {
            text: "OCR first\nOCR second".into(),
            model: "mock".into(),
            version: "sha-1".into(),
            confidence: None,
        })))
    }
    #[test]
    fn ocr_recovers_scan_marks_style_and_records_model_version() {
        let model = success();
        let c = convert(
            &scan(),
            &DjvuOptions::default().text_recovery(model.clone()),
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
        assert_eq!(c.report.recovered_pages(), 1);
        assert_eq!(c.report.recovered_paragraphs(), 2);
        let xml = xml(&c);
        assert!(xml.contains("OCR first"));
        assert!(xml.contains("Recovered"));
        assert!(c
            .report
            .losses()
            .iter()
            .any(|l| l.id == "djvu.ocr.recovered" && l.detail.contains("sha-1")));
        assert_eq!(c.media.len(), 1);
    }
    #[test]
    fn readable_native_text_is_never_sent_to_ocr() {
        let model = success();
        let c = convert(
            &text_file("native", true),
            &DjvuOptions::default().text_recovery(model.clone()),
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
        assert_eq!(c.report.recovered_pages(), 0);
    }
    #[test]
    fn native_text_without_zones_is_retained_and_not_overwritten() {
        let bytes = single(&[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &[0, 0, 1, b'x']),
        ]);
        let model = success();
        let c = convert(&bytes, &DjvuOptions::default().text_recovery(model.clone()));
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
        assert!(xml(&c).contains(">x</w:t>"));
    }
    #[test]
    fn ocr_refusal_timeout_and_empty_answers_keep_the_scan() {
        for answer in [
            Err(VisionError::TimedOut),
            Err(VisionError::Unreachable("offline".into())),
            Ok(None),
            Ok(Some(Recovered {
                text: " ".into(),
                model: "mock".into(),
                version: "1".into(),
                confidence: None,
            })),
        ] {
            let model = provider(answer);
            let c = convert(&scan(), &DjvuOptions::default().text_recovery(model));
            assert_eq!(c.media.len(), 1);
            assert_eq!(c.report.recovered_pages(), 0);
            assert!(c
                .report
                .losses()
                .iter()
                .any(|l| l.id.starts_with("djvu.ocr.")));
            let _ = xml(&c);
        }
    }
    #[test]
    fn oversized_ocr_is_rejected_and_invalid_xml_controls_are_reported() {
        let model = success();
        let options = DjvuOptions {
            max_recovered_text_bytes: 3,
            ..DjvuOptions::default().text_recovery(model)
        };
        let c = convert(&scan(), &options);
        assert_eq!(c.report.recovered_pages(), 0);
        let c = convert(&text_file("a\u{0}b", true), &DjvuOptions::default());
        assert!(c
            .report
            .losses()
            .iter()
            .any(|l| l.id == "convert.invalid-xml-char"));
    }
}

#[cfg(feature = "djvu-image")]
#[test]
fn blank_page_does_not_call_recognition_provider() {
    struct NoCall;
    impl strict_ooxml_ocr::TextRecovery for NoCall {
        fn model_name(&self) -> &'static str {
            "must not call"
        }
        fn recover_page(
            &self,
            _: &strict_ooxml_ocr::Image,
        ) -> Result<Option<strict_ooxml_ocr::Recovered>, strict_ooxml_ocr::VisionError> {
            panic!("blank page");
        }
    }
    let bm = djvu_bitmap::Bitmap::new(100, 100);
    let bytes = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"Sjbz", &djvu_jb2::encode::encode_jb2_lossless(&bm)),
    ]);
    let c = convert(
        &bytes,
        &DjvuOptions::default().text_recovery(std::sync::Arc::new(NoCall)),
    );
    assert_eq!(c.report.recovered_pages(), 0);
}

#[test]
fn a_url_maparea_becomes_an_external_hyperlink() {
    let page = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &text("link")),
        ant("(maparea \"\" (rect 0 0 100 100) (url \"https://example.invalid/x\"))"),
    ]);
    let c = convert(&page, &DjvuOptions::default());
    let (document, rels) = parts(&c);
    assert_eq!(document.matches("<w:hyperlink").count(), 1, "{document}");
    assert!(document.contains("r:id="), "{document}");
    assert!(rels.contains("https://example.invalid/x"), "{rels}");
    assert!(rels.contains("TargetMode=\"External\""), "{rels}");
}

#[test]
fn an_internal_maparea_links_to_a_page_bookmark() {
    let first = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("link")),
            ant("(maparea \"\" (rect 0 0 100 100) (url \"#page=2\"))"),
        ],
    );
    let second = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("target")),
        ],
    );
    let c = convert(&bundled(&[first, second]), &DjvuOptions::default());
    let (document, _rels) = parts(&c);
    assert!(document.contains("w:anchor=\"djvu-page-2\""), "{document}");
    assert!(document.contains("w:name=\"djvu-page-2\""), "{document}");
}
