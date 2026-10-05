//! A19 / F09: a right tab ends the following segment on the stop.

#![allow(clippy::expect_used, clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use common::{build_docx, content_types, document, open_bytes, root_rels};
use strict_ooxml_render_svg::{place_pages, Item, RenderOptions};

fn placed(body: &str) -> Vec<Item> {
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let pages = place_pages(&doc, &RenderOptions::default(), None).expect("place");
    pages.into_iter().flat_map(|page| page.items).collect()
}

/// Stop at 320 px. `12` ends there, and the leader is not part of the text.
#[test]
fn f09_right_tab_aligns_segment_end() {
    let body = "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"4800\"/></w:tabs></w:pPr>\
<w:r><w:t>Title</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>12</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"10800\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let items = placed(body);
    let number = items.iter().find_map(|item| {
        let Item::Text(text) = item else {
            return None;
        };
        (text.text.trim() == "12").then_some((text.x, text.x + text.width))
    });
    let (start, end) = number.expect("the number is placed");
    assert!(
        (end - 320.0).abs() <= 0.25,
        "12 ends at {end} (starts {start}), expected 320"
    );
    assert!(start < 320.0, "12 must end on the stop, not start on it");
    let dots = items
        .iter()
        .filter(|item| matches!(item, Item::Rect(_)))
        .count();
    assert!(dots > 0, "a dot leader is drawn in the gap");
    let words: String = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(words.replace(' ', ""), "Title12");
}

/// An underscore leader is a line in the gap. The following word starts on the stop.
#[test]
fn f09_underscore_leader_does_not_move_the_word() {
    let body = "<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:leader=\"underscore\" w:pos=\"4800\"/></w:tabs></w:pPr>\
<w:r><w:t>Positions</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>15996</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"10800\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let items = placed(body);
    let word = items.iter().find_map(|item| {
        let Item::Text(text) = item else {
            return None;
        };
        text.text.contains("15996").then_some(text.x)
    });
    let word = word.expect("the word after the leader");
    assert!(
        (word - 320.0).abs() <= 0.25,
        "15996 starts at the 320 px stop, got {word}"
    );
    let leader = items.iter().any(|item| {
        let Item::Line(line) = item else {
            return false;
        };
        line.x1 < word - 4.0 && (line.x2 - word).abs() <= 8.0
    });
    assert!(
        leader,
        "the underscore leader occupies the gap before the word"
    );
    let words: String = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !words.contains('_'),
        "the leader is not painted as characters: {words}"
    );
}
