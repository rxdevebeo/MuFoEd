//! An unbreakable token wider than the line is cut into line pieces at
//! grapheme-cluster boundaries.
//!
//! It used to become one text item per `char`, each with a copy of the run: a
//! 1 MB token was a million items, and a combining mark could start a line
//! without its base letter.

#![allow(clippy::doc_markdown)]

mod common;

use common::render_body;

/// The text of every `<text>` element on every page.
fn texts(body: &str) -> Vec<String> {
    render_body(body)
        .iter()
        .flat_map(|page| {
            let document = roxmltree::Document::parse(&page.svg).expect("svg");
            document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "text")
                .map(|node| node.text().unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_long_token_is_cut_between_graphemes_into_few_pieces() {
    // 600 clusters of `e` + COMBINING ACUTE ACCENT, no break opportunity.
    let token = "e\u{301}".repeat(600);
    let body = format!("<w:p><w:r><w:t>{token}</w:t></w:r></w:p>");
    let pieces = texts(&body);
    assert!(pieces.len() > 1, "the token must wrap: {}", pieces.len());
    assert!(
        pieces.len() < 40,
        "one item per line piece, not per character: {}",
        pieces.len()
    );
    for piece in &pieces {
        assert!(
            !piece.starts_with('\u{301}'),
            "a combining mark was separated from its base: {piece:?}"
        );
    }
    // Whether the painter composes `e` + U+0301 into `é` is not this test's
    // business; that every cluster arrives once is.
    let clusters = pieces
        .concat()
        .chars()
        .filter(|ch| matches!(ch, 'e' | '\u{e9}'))
        .count();
    assert_eq!(clusters, 600, "nothing lost or duplicated");
}
