//! End-to-end test over the Stage-5C Strict corpus: the synthetic fixture
//! `strict-stage5c.docx` and the real hand-built Strict packages that shipped
//! with `STAGE-5C-REWORK-1` (`05`/`06`/`07`/`09`/`10`).
//!
//! The real packages are the independent evidence for the vertical-metric work
//! (C1) and for the `DrawingML`/chart findings (D1/D3/D4): they are *not*
//! self-consistent fixtures like `strict-stage5c`, so a defect cannot hide by
//! agreeing with itself.

#![allow(clippy::expect_used, missing_docs)]

use std::path::{Path, PathBuf};

use strict_ooxml::{OpenOptions, StrictDocument};

/// The real Strict packages committed with the rework order.
const REPRO: &[&str] = &[
    "05-strict-math-simple",
    "06-strict-math-display",
    "07-strict-drawingml-shapes",
    "09-strict-math-drawing-chart",
    "10-strict-math-eqarr",
];

/// The page count the WPS `12.1.0.28485` reference produces for each repro.
const REPRO_PAGES: &[(&str, usize)] = &[
    ("05-strict-math-simple", 1),
    ("06-strict-math-display", 1),
    ("07-strict-drawingml-shapes", 1),
    ("09-strict-math-drawing-chart", 1),
    ("10-strict-math-eqarr", 1),
];

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

fn fixture(name: &str) -> PathBuf {
    corpus().join(format!("{name}.docx"))
}

fn open(name: &str) -> StrictDocument {
    let path = fixture(name);
    assert!(path.is_file(), "missing corpus fixture {}", path.display());
    StrictDocument::open_path(&path, &OpenOptions::default()).expect("open strict fixture")
}

/// Drops the embedded `@font-face` block before a text search.
///
/// Layout checks read ink coordinates. The font program's base64 can contain
/// the character sequences those checks reject.
fn without_font_faces(svg: &str) -> String {
    let Some(start) = svg.find("  <style type=\"text/css\">") else {
        return svg.to_owned();
    };
    let rest = &svg[start..];
    let Some(end) = rest.find("]]></style>\n") else {
        return svg.to_owned();
    };
    let mut out = String::with_capacity(svg.len());
    out.push_str(&svg[..start]);
    out.push_str(&rest[end + "]]></style>\n".len()..]);
    out
}

#[test]
fn every_repro_fixture_is_present_and_parses() {
    for name in REPRO {
        let document = open(name);
        assert!(
            !document.document().body.blocks.is_empty(),
            "{name} parsed to an empty body"
        );
    }
}

#[cfg(feature = "report")]
#[test]
fn no_repro_fixture_reports_an_unsupported_mechanism() {
    // STAGE-5C-REWORK-1 D2: a real Word/LibreOffice Strict package must not be
    // reported as blocked on a technicality such as `w:rsids` or
    // `a:objectDefaults`.
    for name in REPRO {
        let document = open(name);
        let report = document.support_report();
        assert!(
            !report.has_critical_problems(),
            "{name} must not report unsupported/error mechanisms: {:?}",
            report
                .features
                .iter()
                .filter(|feature| feature.status.as_str() == "unsupported")
                .map(|feature| &feature.feature_id)
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(feature = "svg")]
#[test]
fn every_repro_fixture_renders_with_the_reference_page_count() {
    // The page-count invariant (D4): `09` used to spill onto a second page.
    for (name, pages) in REPRO_PAGES {
        let document = open(name);
        let rendered = document
            .render_svg(&strict_ooxml::RenderOptions::default())
            .expect("render");
        assert_eq!(
            rendered.len(),
            *pages,
            "{name}: page count must match the WPS reference"
        );
        for page in &rendered {
            assert!(page.svg.contains("<svg "), "{name}: invalid SVG");
            // The embedded font program is base64; those bytes can spell "NaN"
            // without any coordinate being NaN.
            assert!(
                !without_font_faces(&page.svg).contains("NaN"),
                "{name}: NaN in SVG"
            );
        }
    }
}

#[cfg(feature = "svg")]
#[test]
fn the_math_repros_carry_formula_ink() {
    // A regression control for the C1 defect: an empty formula render used to
    // satisfy "one page, not blank" while the band count collapsed.
    for name in [
        "05-strict-math-simple",
        "06-strict-math-display",
        "10-strict-math-eqarr",
    ] {
        let document = open(name);
        let rendered = document
            .render_svg(&strict_ooxml::RenderOptions::default())
            .expect("render");
        let svg = rendered[0].svg.clone();
        assert!(svg.contains("STIX Two Math"), "{name}: no math font run");
        assert!(
            svg.matches("<text").count() >= 8,
            "{name}: too few runs — the formula layer collapsed"
        );
    }
}

#[allow(dead_code)]
fn relative(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}
