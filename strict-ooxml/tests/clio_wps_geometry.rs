//! P1 WPS geometry on Clio, as a Rust test (`docs/history/P01-wps-geometry-tests.md`).
//!
//! The document is not in the repository: it lives in the local corpus
//! `strict-ooxml-core/tests/docx`, which the host gate copies in before it runs.
//! Without it the test says so and passes - the same rule as the other local
//! corpus tests - so the protocol is enforced where the document is, and
//! `xtool/wps-gate/wps_ledger.py` stays the instrument the numbers below came from.
//!
//! The reference points are WPS's glyph origins (page top-left, 96 dpi) for the
//! mandatory P1 keys on pages 54, 56 and 104, frozen from the ledger. The match
//! follows the ledger: a key is a whole occurrence in the page's glyph stream,
//! identified by the twelve non-space characters either side of it, and a key
//! that names more than one place is a failure, never "the first one".
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::path::PathBuf;
use std::sync::Arc;

use strict_ooxml::{
    ConformancePolicy, OpenOptions, PageSelection, RenderOptions, StrictDocument,
    TransitionalNormalizer,
};

const CLIO: &str =
    "Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx";
const TOLERANCE_PX: f64 = 0.25;
const NEIGHBOURHOOD: usize = 12;

/// One mandatory point: the WPS origin of `text` on `page`, between `left`
/// and `right` (the non-space characters either side; `None` at a line edge).
struct Point {
    id: &'static str,
    page: usize,
    text: &'static str,
    left: Option<&'static str>,
    right: Option<&'static str>,
    x: f64,
    y: f64,
}

#[rustfmt::skip]
const POINTS: &[Point] = &[
    Point { id: "p54.HVR_I.0", page: 54, text: "HVR-I", left: None, right: Some("Positions'15"), x: 382.5600, y: 99.2667 },
    Point { id: "p54.HVR_I.1", page: 54, text: "HVR-I", left: Some("ofprimersfor"), right: Some("amplificatio"), x: 310.2400, y: 480.8667 },
    Point { id: "p54.HVR_I.2", page: 54, text: "HVR-I", left: Some("roupsbasedon"), right: Some("sequenceswas"), x: 460.3200, y: 581.9867 },
    Point { id: "p54.L15996.0", page: 54, text: "L15996", left: Some("'15996-16410"), right: Some("H16142L16055"), x: 169.6000, y: 129.1867 },
    Point { id: "p54.L15996.1", page: 54, text: "L15996", left: Some("L16212H16402"), right: Some("H16410Figure"), x: 169.6000, y: 388.3867 },
    Point { id: "p54.H16142.0", page: 54, text: "H16142", left: Some("-16410L15996"), right: Some("L16055H16142"), x: 283.0400, y: 143.5867 },
    Point { id: "p54.H16142.1", page: 54, text: "H16142", left: Some("H16142L16055"), right: Some("L16117H16233"), x: 283.0400, y: 165.6667 },
    Point { id: "p54.L16055.0", page: 54, text: "L16055", left: Some("L15996H16142"), right: Some("H16142L16117"), x: 211.0400, y: 154.9453 },
    Point { id: "p54.L16117.0", page: 54, text: "L16117", left: Some("L16055H16142"), right: Some("H16233L16117"), x: 283.0400, y: 189.9867 },
    Point { id: "p54.L16117.1", page: 54, text: "L16117", left: Some("L16117H16233"), right: Some("H16217L16209"), x: 283.0400, y: 218.7867 },
    Point { id: "p54.H16233.0", page: 54, text: "H16233", left: Some("H16142L16117"), right: Some("L16117H16217"), x: 378.2400, y: 204.3867 },
    Point { id: "p56.neg_M.0", page: 56, text: "-M", left: Some("’1\"L2’6\"L3'4"), right: Some("\"C-D-N-A\"N1I"), x: 582.2400, y: 325.6667 },
    Point { id: "p56.H.0", page: 56, text: "H", left: Some("w-x■R■RO■HV■"), right: Some("-v■B\"R9■T\"J-"), x: 589.4400, y: 792.0667 },
    Point { id: "p56.8994.0", page: 56, text: "8994", left: Some("e22reaction."), right: Some("637112705117"), x: 362.2400, y: 429.9867 },
    Point { id: "p56.6371.0", page: 56, text: "6371", left: Some("eaction.8994"), right: Some("127051171914"), x: 362.2400, y: 467.2667 },
    Point { id: "p56.11719.0", page: 56, text: "11719", left: Some("994637112705"), right: Some("147667028458"), x: 424.0000, y: 539.2667 },
    Point { id: "p56.14766.0", page: 56, text: "14766", left: Some("711270511719"), right: Some("702845808280"), x: 455.2000, y: 575.1067 },
    Point { id: "p56.7028.0", page: 56, text: "7028", left: Some("051171914766"), right: Some("458082801392"), x: 517.2787, y: 610.9453 },
    Point { id: "p104.A_C.0", page: 104, text: "A,C", left: None, right: Some(":allhaplotyp"), x: 128.6400, y: 103.4267 },
    Point { id: "p104.B_D.0", page: 104, text: "B,D", left: Some("ucasusEurope"), right: Some(":informative"), x: 120.8000, y: 335.7453 },
    Point { id: "p104.modern.0", page: 104, text: "modern", left: Some("haplotypesA:"), right: Some("jnuflnUniiOB"), x: 168.6400, y: 113.9867 },
    Point { id: "p104.modern.1", page: 104, text: "modern", left: Some("otypesonlyB:"), right: Some("□UL01cl0<>->"), x: 224.1600, y: 335.7453 },
    Point { id: "p104.modern.2", page: 104, text: "modern", left: Some("edinselected"), right: Some("(A,B)andanci"), x: 303.5200, y: 598.7867 },
    Point { id: "p104.modern.3", page: 104, text: "modern", left: Some("leniOstrovin"), right: Some("populationsf"), x: 554.8787, y: 635.4253 },
    Point { id: "p104.modern.4", page: 104, text: "modern", left: Some("A,B).Foreach"), right: Some("population,t"), x: 147.2000, y: 671.9067 },
    Point { id: "p104.modern.5", page: 104, text: "modern", left: Some("dividedbythe"), right: Some("populationsa"), x: 301.1200, y: 690.1453 },];

/// A glyph of the rendered SVG: its character and origin.
struct Glyph {
    ch: char,
    x: Option<f64>,
    y: f64,
}

/// The glyphs of one SVG page, in document order, as the ledger reads them:
/// a `<text>` with one x per character gives each its own; one with a single x
/// gives it to the first character only.
fn glyphs(svg: &str) -> Vec<Glyph> {
    let tree = roxmltree::Document::parse(svg).expect("our SVG parses");
    let mut out = Vec::new();
    for node in tree.descendants().filter(|n| n.has_tag_name("text")) {
        let text: String = node.text().unwrap_or_default().to_owned();
        let xs: Vec<f64> = node
            .attribute("x")
            .unwrap_or("0")
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        let y: f64 = node
            .attribute("y")
            .and_then(|y| y.parse().ok())
            .unwrap_or(0.0);
        let count = text.chars().count();
        for (i, ch) in text.chars().enumerate() {
            let x = if xs.len() == count {
                xs.get(i).copied()
            } else if i == 0 {
                xs.first().copied()
            } else {
                None
            };
            out.push(Glyph { ch, x, y });
        }
    }
    out
}

/// Letters and digits only, lower-cased: how the ledger compares neighbourhoods.
fn normalized(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

fn compatible(reference: Option<&str>, actual: Option<&str>) -> bool {
    let reference = reference.map(normalized).filter(|n| !n.is_empty());
    let actual = actual.map(normalized).filter(|n| !n.is_empty());
    match (reference, actual) {
        (None, None) => true,
        (Some(a), Some(b)) if a == b => true,
        (Some(a), Some(b)) => a.len().min(b.len()) >= 3 && (a.starts_with(&b) || b.starts_with(&a)),
        _ => false,
    }
}

/// One whole occurrence of a key: its neighbourhood and its origin.
struct Hit {
    left: Option<String>,
    right: Option<String>,
    x: Option<f64>,
    y: f64,
}

/// Every whole occurrence of `key` with its neighbourhood and origin.
fn hits(glyphs: &[Glyph], key: &str) -> Vec<Hit> {
    // Underscores are skipped so split runs concatenate; spaces are boundaries.
    let kept: Vec<&Glyph> = glyphs.iter().filter(|g| g.ch != '_').collect();
    let chars: Vec<char> = kept
        .iter()
        .map(|g| if g.ch.is_whitespace() { ' ' } else { g.ch })
        .collect();
    let key: Vec<char> = key.chars().collect();
    let window = |range: Box<dyn Iterator<Item = usize>>, reverse: bool| -> Option<String> {
        let mut picked: Vec<char> = range
            .filter_map(|j| chars.get(j).copied())
            .filter(|c| *c != ' ')
            .take(NEIGHBOURHOOD)
            .collect();
        if reverse {
            picked.reverse();
        }
        (!picked.is_empty()).then(|| picked.into_iter().collect())
    };
    let mut out = Vec::new();
    for start in 0..chars.len() {
        let end = start + key.len();
        if chars.get(start..end) != Some(key.as_slice()) || chars[start..end].contains(&' ') {
            continue;
        }
        if key.len() < 3 {
            let before = start.checked_sub(1).and_then(|j| chars.get(j));
            let after = chars.get(end);
            let edge = |c: Option<&char>| c.is_none_or(|c| !c.is_alphanumeric());
            let first = key.first().is_some_and(|c| c.is_alphanumeric());
            let last = key.last().is_some_and(|c| c.is_alphanumeric());
            if (first && !edge(before)) || (last && !edge(after)) {
                continue;
            }
        }
        let left = window(Box::new((0..start).rev()), true);
        let right = window(Box::new(end..chars.len()), false);
        out.push(Hit {
            left,
            right,
            x: kept[start].x,
            y: kept[start].y,
        });
    }
    out
}

#[test]
fn clio_matches_wps_on_the_mandatory_points() {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "strict-ooxml-core",
        "tests",
        "docx",
        CLIO,
    ]
    .iter()
    .collect();
    if !path.exists() {
        eprintln!("skipped: {} is not in the local corpus", path.display());
        return;
    }
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(Arc::new(TransitionalNormalizer::new()));
    let document = StrictDocument::open_path(&path, &options).expect("Clio opens");
    let render = RenderOptions::default()
        .pages(PageSelection::Range {
            start: 54,
            end: 104,
        })
        .wps_times_calibration(true);
    let pages = document.render_svg(&render).expect("Clio renders");

    let mut failures = Vec::new();
    let mut tightest = (f64::INFINITY, "");
    for point in POINTS {
        let page = pages
            .iter()
            .find(|page| page.index + 1 == point.page)
            .unwrap_or_else(|| panic!("page {} rendered", point.page));
        let glyphs = glyphs(&page.svg);
        let candidates: Vec<_> = hits(&glyphs, point.text)
            .into_iter()
            .filter(|hit| {
                compatible(point.left, hit.left.as_deref())
                    && compatible(point.right, hit.right.as_deref())
            })
            .collect();
        let [Hit { x: Some(x), y, .. }] = candidates.as_slice() else {
            failures.push(format!(
                "{}: {} candidate(s) for {:?}",
                point.id,
                candidates.len(),
                point.text
            ));
            continue;
        };
        let (dx, dy) = (x - point.x, y - point.y);
        let worst = dx.abs().max(dy.abs());
        if TOLERANCE_PX - worst < tightest.0 {
            tightest = (TOLERANCE_PX - worst, point.id);
        }
        if worst > TOLERANCE_PX {
            failures.push(format!("{}: dx={dx:+.3} dy={dy:+.3} px", point.id));
        }
    }
    eprintln!("tightest margin: {:.3} px at {}", tightest.0, tightest.1);
    assert!(failures.is_empty(), "P1 geometry:\n{}", failures.join("\n"));
}
