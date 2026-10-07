//! Corpus run: renderable Strict documents must render valid SVG without
//! panics; Transitional samples are refused by the parser (ADR-0005/0006).

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

use std::path::Path;

mod common;

use strict_ooxml_core::ns::Conformance;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_render_svg::{render, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

#[test]
fn corpus_renders_or_refuses_without_panics() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/samples");
    if !dir.is_dir() {
        eprintln!("corpus directory not present; skipped");
        return;
    }
    let permissive = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    let mut rendered = 0u32;
    let mut refused = 0u32;
    let mut other = 0u32;
    for entry in std::fs::read_dir(&dir).expect("read corpus") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        let Ok(package) = Package::open_path(&path, &permissive) else {
            other += 1;
            continue;
        };
        match parse_document(&package, &ParseOptions::default()) {
            Ok(document) => {
                let pages =
                    render(&document, &RenderOptions::default()).expect("render must not fail");
                assert!(!pages.is_empty());
                for page in &pages {
                    roxmltree::Document::parse(&page.svg).expect("valid SVG");
                    let ink = common::without_font_faces(&page.svg);
                    assert!(!ink.contains("NaN"), "{ink}");
                    assert!(!ink.contains("inf"), "{ink}");
                }
                rendered += 1;
            }
            Err(error) => {
                if package.conformance() == Conformance::Transitional {
                    refused += 1;
                } else {
                    eprintln!("{}: {error}", path.display());
                    other += 1;
                }
            }
        }
    }
    eprintln!("corpus: rendered={rendered} refused={refused} other={other}");
    assert!(
        rendered + refused + other > 0,
        "expected at least one sample"
    );
}

/// The CC0 `ci-core` tier, normalized: every document opens, parses and
/// renders (first 20 pages) to valid SVG with finite coordinates. The samples
/// above are Transitional and only exercise the refusal; these exercise the
/// layout on real Word, LibreOffice and WPS output.
#[test]
fn cc0_core_corpus_renders() {
    use strict_ooxml_core::normalize::transitional::TransitionalNormalizer;
    use strict_ooxml_render_svg::PageSelection;
    use strict_ooxml_testkit::corpus::{tier, Tier};

    let docs = tier(Tier::CiCore);
    if docs.is_empty() {
        eprintln!("SKIP cc0 ci-core corpus not fetched");
        return;
    }
    let options = RenderOptions {
        pages: PageSelection::Range { start: 1, end: 20 },
        ..RenderOptions::default()
    };
    for doc in docs {
        let open = OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(std::sync::Arc::new(TransitionalNormalizer::new()));
        let package = Package::open_path(&doc.path, &open)
            .unwrap_or_else(|error| panic!("{}: open: {error}", doc.id));
        let document = parse_document(&package, &ParseOptions::default())
            .unwrap_or_else(|error| panic!("{}: parse: {error}", doc.id));
        let pages = render(&document, &options)
            .unwrap_or_else(|error| panic!("{}: render: {error}", doc.id));
        assert!(!pages.is_empty(), "{}: no pages", doc.id);
        for page in &pages {
            roxmltree::Document::parse(&page.svg)
                .unwrap_or_else(|error| panic!("{}: invalid SVG: {error}", doc.id));
            let ink = common::without_font_faces(&page.svg);
            assert!(!ink.contains("NaN"), "{}: NaN in SVG", doc.id);
            assert!(!ink.contains("inf"), "{}: inf in SVG", doc.id);
        }
    }
}
