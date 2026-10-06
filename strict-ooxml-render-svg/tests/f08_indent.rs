//! A20 / F08: a first-line indent moves the start of the line, not its end.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{place_pages, render, Item, RenderOptions};

const PAGE: &str = "<w:sectPr><w:pgSz w:w=\"9000\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\"/></w:sectPr>";

fn edges(body: &str) -> Vec<(f64, f64, String)> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let placed = place_pages(&doc, &RenderOptions::default(), None).expect("place");
    let mut out = Vec::new();
    for page in &placed {
        for item in &page.items {
            let Item::Text(text) = item else {
                continue;
            };
            out.push((text.x, text.x + text.width, text.text.clone()));
        }
    }
    out
}

/// 600 px container, firstLine 160 px. Every fragment stays inside 600 px,
/// and the first line actually wraps.
#[test]
fn f08_first_line_indent_keeps_right_boundary() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda \
mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
    let body = format!(
        "<w:p><w:pPr><w:ind w:firstLine=\"2400\"/></w:pPr><w:r><w:t>{words}</w:t></w:r></w:p>{PAGE}"
    );
    let placed = edges(&body);
    assert!(placed.len() > 1, "the first line must wrap: {placed:?}");
    for (x, right, text) in &placed {
        assert!(
            *right <= 600.25,
            "{text:?} ends at {right} px, past the 600 px edge (x={x})"
        );
    }
    let first = placed.first().expect("first");
    assert!(
        first.0 > 140.0 && first.0 < 180.0,
        "first line starts at the indent, got {}",
        first.0
    );
    let wrapped = placed.iter().find(|item| item.0 < 20.0);
    assert!(
        wrapped.is_some(),
        "a later line starts at the left edge: {placed:?}"
    );
}

/// Hanging indent lengthens the first line and keeps the same right edge.
#[test]
fn f08_hanging_indent_keeps_the_same_right_edge() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda \
mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega";
    let body = format!(
        "<w:p><w:pPr><w:ind w:start=\"2400\" w:hanging=\"2400\"/></w:pPr>\
<w:r><w:t>{words}</w:t></w:r></w:p>{PAGE}"
    );
    let placed = edges(&body);
    assert!(placed.len() > 1, "hanging text must wrap: {placed:?}");
    let first = placed.first().expect("first");
    assert!(
        first.0.abs() <= 0.25,
        "hanging first line starts at the left edge, got {}",
        first.0
    );
    for (x, right, text) in &placed {
        assert!(
            *right <= 600.25,
            "{text:?} ends at {right} px, past the 600 px edge (x={x})"
        );
    }
    let continued = placed.iter().find(|item| (item.0 - 160.0).abs() <= 1.0);
    assert!(
        continued.is_some(),
        "a later line starts at the start indent: {placed:?}"
    );
}

/// A token wider than the line breaks by characters and stays inside the edge.
#[test]
fn f08_long_token_breaks_inside_the_right_edge() {
    let word = "W".repeat(80);
    let body = format!(
        "<w:p><w:pPr><w:ind w:firstLine=\"2400\"/></w:pPr><w:r><w:t>{word}</w:t></w:r></w:p>{PAGE}"
    );
    let placed = edges(&body);
    let chars: usize = placed.iter().map(|item| item.2.chars().count()).sum();
    assert_eq!(chars, 80, "the token is wrapped, not clipped: {placed:?}");
    assert!(placed.len() > 1, "the token occupies more than one line");
    for (x, right, text) in &placed {
        assert!(
            *right <= 600.25,
            "{text:?} ends at {right} px, past the 600 px edge (x={x})"
        );
    }
    assert!(placed.first().expect("first").0 > 140.0);
    assert!(placed.iter().any(|item| item.0 < 20.0));
}

/// An indent wider than the container is reported. The glyph stays visible.
#[test]
fn f08_indent_past_the_edge_is_reported_not_clipped() {
    let body = format!(
        "<w:p><w:pPr><w:ind w:start=\"12000\"/></w:pPr><w:r><w:t>Hi</w:t></w:r></w:p>{PAGE}"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let pages = render(&doc, &RenderOptions::default()).expect("render");
    let page = pages.first().expect("page");
    assert!(
        page.svg.contains(">Hi<") || page.svg.contains(">H<"),
        "the glyph is painted, not clipped away"
    );
    assert!(
        page.warnings
            .iter()
            .any(|warning| warning.contains("render.line-overflow")
                || warning.contains("render.line-interval")),
        "overflow is reported: {:?}",
        page.warnings
    );
}

/// Exact line spacing keeps the declared pitch between baselines.
///
/// 240 twips is 16 px. The gap is that pitch, not a second copy of the line
/// and not the 0.8 ascent fraction.
#[test]
fn f08_exact_line_pitch_is_the_declared_line() {
    let body = format!(
        "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>alpha</w:t></w:r></w:p>\
<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"exact\"/></w:pPr>\
<w:r><w:t>beta</w:t></w:r></w:p>{PAGE}"
    );
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(&body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let placed = place_pages(&doc, &RenderOptions::default(), None).expect("place");
    let baseline = |needle: &str| {
        placed
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| {
                let Item::Text(text) = item else {
                    return None;
                };
                text.text.contains(needle).then_some(text.baseline)
            })
            .unwrap_or_else(|| panic!("{needle} missing"))
    };
    let alpha = baseline("alpha");
    let beta = baseline("beta");
    assert!(
        (beta - alpha - 16.0).abs() <= 0.25,
        "exact 240 twips must separate baselines by 16 px, got {alpha} then {beta}"
    );
}

#[test]
fn f08_justify_and_cell_indent_matrix() {
    let body = format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"6000\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:pPr><w:ind w:start=\"720\"/><w:jc w:val=\"both\"/></w:pPr>\
<w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>{PAGE}"
    );
    let rows = edges(&body);
    let cell = rows
        .iter()
        .find(|(_, _, text)| text.contains("cell"))
        .expect("cell");
    assert!(
        cell.0 >= 48.0 - 0.25,
        "start indent 720 twips is 48 px, got {}",
        cell.0
    );
}
