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
