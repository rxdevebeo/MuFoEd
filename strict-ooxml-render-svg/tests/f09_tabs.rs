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

#[test]
fn f09_left_center_decimal_tabs_matrix() {
    let body = "<w:p><w:pPr><w:tabs>\
<w:tab w:val=\"left\" w:pos=\"1440\"/>\
<w:tab w:val=\"center\" w:pos=\"2880\"/>\
<w:tab w:val=\"decimal\" w:pos=\"4320\"/>\
</w:tabs></w:pPr>\
<w:r><w:tab/></w:r><w:r><w:t>L</w:t></w:r>\
<w:r><w:tab/></w:r><w:r><w:t>C</w:t></w:r>\
<w:r><w:tab/></w:r><w:r><w:t>1.5</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"10800\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let items = placed(body);
    let x_of = |needle: &str| {
        items.iter().find_map(|item| {
            let Item::Text(text) = item else {
                return None;
            };
            text.text.contains(needle).then_some(text.x)
        })
    };
    let left = x_of("L").expect("L");
    let center = x_of("C").expect("C");
    let decimal = x_of("1.5").expect("1.5");
    assert!((left - 96.0).abs() <= 1.0, "left tab {left}");
    assert!(center > left, "center tab {center} after left {left}");
    assert!(
        decimal > center,
        "decimal tab {decimal} after center {center}"
    );
}

/// Consecutive default tabs each move to the next default stop. With a left
/// margin that is no whole number of pixels (3 cm, Word's Russian default) a
/// tab ending exactly on a stop left `x - left` a hair under it, `floor` put
/// the next tab on that same stop, and every tab after the first vanished.
#[test]
fn f09_consecutive_default_tabs_each_advance_a_stop() {
    for tabs in 1..=5_u32 {
        let body = format!(
            "<w:p><w:r><w:t>a</w:t></w:r>{}<w:r><w:t>b</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
<w:pgMar w:top=\"1134\" w:right=\"850\" w:bottom=\"1134\" w:left=\"1701\" w:header=\"709\" w:footer=\"709\" w:gutter=\"0\"/>\
</w:sectPr>",
            "<w:r><w:tab/></w:r>".repeat(tabs as usize)
        );
        let items = placed(&body);
        let x_of = |word: &str| {
            items.iter().find_map(|item| match item {
                Item::Text(text) if text.text.trim() == word => Some(text.x),
                _ => None,
            })
        };
        let left = x_of("a").expect("a is placed");
        let b = x_of("b").expect("b is placed");
        // The default stop is 720 twips: 48 px at 96 dpi.
        let expected = left + 48.0 * f64::from(tabs);
        assert!(
            (b - expected).abs() <= 0.25,
            "{tabs} tab(s): b at {b}, expected {expected}"
        );
    }
}

/// Caret anchors are off unless asked for. With them, an empty paragraph's
/// line gets a zero-width empty item and a tab an empty item spanning it, and
/// nothing else moves.
#[test]
fn caret_anchors_mark_empty_lines_and_tabs_only_when_asked() {
    let body = "<w:p/><w:p><w:r><w:t>a</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>b</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"10800\" w:h=\"15840\"/>\
<w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>\
</w:sectPr>";
    let bytes = build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document(body).into_bytes()),
    ]);
    let (_package, doc) = open_bytes(bytes);
    let texts = |options: &RenderOptions| -> Vec<strict_ooxml_render_svg::TextItem> {
        place_pages(&doc, options, None)
            .expect("place")
            .into_iter()
            .flat_map(|page| page.items)
            .filter_map(|item| match item {
                Item::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    };
    let plain = texts(&RenderOptions::default());
    assert!(plain.iter().all(|text| !text.text.is_empty()));
    let anchored = texts(&RenderOptions::default().caret_anchors(true));
    let anchors: Vec<_> = anchored
        .iter()
        .filter(|text| text.text.is_empty())
        .collect();
    assert_eq!(anchors.len(), 2, "{anchors:?}");
    // The empty paragraph's anchor: zero width at the line start, above the
    // second paragraph.
    let a = plain.iter().find(|t| t.text == "a").expect("a");
    let b = plain.iter().find(|t| t.text == "b").expect("b");
    let empty = anchors
        .iter()
        .find(|t| t.width == 0.0)
        .expect("empty line anchor");
    assert!((empty.x - a.x).abs() < 1e-6 && empty.baseline < a.baseline);
    // The tab's anchor spans from the end of "a" to the start of "b".
    let tab = anchors.iter().find(|t| t.width > 0.0).expect("tab anchor");
    assert!(
        (tab.x - (a.x + a.width)).abs() < 1e-6,
        "{} {}",
        tab.x,
        a.x + a.width
    );
    assert!((tab.x + tab.width - b.x).abs() < 1e-6);
    // Everything else is where it was.
    let rest: Vec<_> = anchored
        .iter()
        .filter(|text| !text.text.is_empty())
        .collect();
    assert_eq!(rest.len(), plain.len());
    for (x, y) in rest.iter().zip(&plain) {
        assert_eq!((x.x, x.baseline, &x.text), (y.x, y.baseline, &y.text));
    }
}
