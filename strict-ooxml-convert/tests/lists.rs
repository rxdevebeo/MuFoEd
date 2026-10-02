//! Lists out of a marker the PDF drew as text (`P-8`).
//!
//! The fixture is built by hand, like the recovery fixture and for the same
//! reason: nothing in this workspace can *produce* a bulleted list inside a PDF
//! except the writer, and a fixture made by the writer would be a fixture made by
//! the code under test.
//!
//! What is at stake is stated in `src/lists.rs`: this is the only inference in the
//! converter that **moves** a character the reader drew. So the tests are mostly
//! about what must *not* happen — one bullet is not a list, a marker glued to its
//! word is not a marker, a numbered list is left alone — and the positive test
//! checks the two halves of "nothing was deleted": the character is in the
//! numbering definition, and it is not in the text.

#![allow(clippy::doc_markdown)]

use strict_ooxml_convert::{convert, ListRules, Mode, PdfOptions};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::numbering::NumberingTable;
use strict_ooxml_wml::model::Document;

/// The byte the fixture's `ToUnicode` maps to U+2022 BULLET.
///
/// A literal `•` in a PDF string is three UTF-8 bytes, and this fixture's font
/// maps one byte to one character; a code of its own is what a producer that
/// subsets a font actually emits, and it keeps the fixture's bytes printable.
const BULLET: u8 = 0x95;

/// One line of the fixture: a string at an x, and a baseline.
struct Piece {
    text: Vec<u8>,
    x: f64,
    baseline: f64,
}

impl Piece {
    fn at(text: &str, x: f64, baseline: f64) -> Self {
        Self {
            text: text.as_bytes().to_vec(),
            x,
            baseline,
        }
    }

    fn bullet(x: f64, baseline: f64) -> Self {
        Self {
            text: vec![BULLET],
            x,
            baseline,
        }
    }
}

/// A page whose content is `pieces`, one text run each, 12 pt type.
fn page_pdf(pieces: &[Piece]) -> Vec<u8> {
    let mut content = Vec::new();
    for piece in pieces {
        content.extend_from_slice(
            format!(
                "BT /F1 12 Tf 1 0 0 1 {} {} Tm <{}> Tj ET ",
                piece.x,
                piece.baseline,
                hex(&piece.text)
            )
            .as_bytes(),
        );
    }
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 \
             5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(&content),
        font(),
        stream(cmap().as_bytes()),
    ])
}

/// Three (or however many) numbered lines, 14 pt apart, the marker being 1..
fn bulleted(items: &[&str], marker_x: f64, text_x: f64) -> Vec<u8> {
    let pieces: Vec<Piece> = items
        .iter()
        .enumerate()
        .flat_map(|(index, item)| {
            // Integer baselines, and no cast: the fixture's numbers are whole points
            // and a fixture that had to convert one to the other would be the second
            // place in this file where a number is quietly rounded.
            let baseline = 40.0 + 14.0 * f64::from(u8::try_from(index).unwrap_or(u8::MAX));
            [
                Piece::bullet(marker_x, baseline),
                Piece::at(item, text_x, baseline),
            ]
        })
        .collect();
    page_pdf(&pieces)
}

/// Numbered lines, with `numbers[i]` the marker of item `i`.
///
/// The marker is its own run sharing the item's baseline, exactly as a bullet is,
/// because that is how a producer writes it: the number and the text are separated
/// by a real gap, and this reader splits a line at a gap.
fn numbered_items(items: &[&str], numbers: &[&str], marker_x: f64, text_x: f64) -> Vec<u8> {
    let pieces: Vec<Piece> = items
        .iter()
        .zip(numbers)
        .enumerate()
        .flat_map(|(index, (item, number))| {
            // Integer baselines, and no cast: the fixture's numbers are whole points
            // and a fixture that had to convert one to the other would be the second
            // place in this file where a number is quietly rounded.
            let baseline = 40.0 + 14.0 * f64::from(u8::try_from(index).unwrap_or(u8::MAX));
            [
                Piece::at(number, marker_x, baseline),
                Piece::at(item, text_x, baseline),
            ]
        })
        .collect();
    page_pdf(&pieces)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The fixture's font: Helvetica, a stated width for every code it uses, and a
/// `ToUnicode` CMap.
///
/// The widths are **stated** on purpose. A font that states a width for one code
/// and none for the rest leaves the reader to estimate, and the marker gap — the
/// one measurement the whole feature rests on — is then a function of the
/// estimator. 500/1000 em for every code makes it 6 pt at 12 pt type, which is a
/// number the test can state.
fn font() -> Vec<u8> {
    let mut out = String::from(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 126 \
         /Widths [",
    );
    for _ in 0..95 {
        out.push_str("500 ");
    }
    out.push_str("] /ToUnicode 6 0 R >>\n");
    out.into_bytes()
}

/// A `ToUnicode` CMap: printable ASCII one byte each, plus the bullet's own code.
///
/// Two `beginbfchar` sections because the bullet's code is outside the ASCII
/// codespace the first one declares, and a CMap that declared one codespace for
/// both is a CMap a reader is entitled to reject.
fn cmap() -> String {
    let mut ascii = String::from(" 95 beginbfchar ");
    for code in 0x20u8..=0x7e {
        use std::fmt::Write as _;
        let _ = write!(ascii, "<{code:02x}> <{code:02x}> ");
    }
    format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
         /CMapName /Adobe-Identity-UCS def /CMapType 2 def \
         1 begincodespacerange <20> <7e> endcodespacerange \
         {ascii}endbfchar \
         1 begincodespacerange <95> <95> endcodespacerange \
         1 beginbfchar <95> <2022> endbfchar endcmap \
         CMapName currentdict /CMap defineresource pop end end"
    )
}

fn text(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

fn stream(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Assembles a PDF from object bodies, with a cross-reference table.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        body.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        body.extend_from_slice(object);
        body.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(&body);
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in &offsets {
        use std::fmt::Write as _;
        let _ = write!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n0\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn convert_pdf(bytes: &[u8]) -> (Document, String) {
    let mut document = PdfDocument::open(bytes, PdfLimits::default()).expect("open");
    let converted = convert(&mut document, &PdfOptions::default()).expect("convert");
    (converted.document, converted.report.to_string())
}

/// The text of every paragraph, in order.
fn texts_of(document: &Document) -> Vec<String> {
    document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .map(|paragraph| {
            paragraph
                .inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Run(run) => Some(
                        run.content
                            .iter()
                            .filter_map(|content| match content {
                                RunContent::Text(node) => Some(node.text.as_str()),
                                _ => None,
                            })
                            .collect::<String>(),
                    ),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// The blocks that carry a `w:numPr`.
fn numbered_of(document: &Document) -> Vec<usize> {
    document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .enumerate()
        .filter(|(_, paragraph)| paragraph.props.numbering.is_some())
        .map(|(index, _)| index)
        .collect()
}

/// The `w:lvlText` of every level of every abstract definition.
fn level_texts(numbering: &NumberingTable) -> Vec<String> {
    numbering
        .abstracts()
        .flat_map(|abstract_num| abstract_num.levels.iter())
        .filter_map(|level| level.text.as_ref().map(ToString::to_string))
        .collect()
}

/// Three bulleted lines become three numbered paragraphs, and the bullet is in the
/// numbering definition rather than in the text.
#[test]
fn a_bulleted_run_becomes_a_list_and_the_marker_moves() {
    let (document, report) = convert_pdf(&bulleted(
        &["first item", "second item", "third item"],
        40.0,
        54.0,
    ));
    assert_eq!(
        texts_of(&document),
        vec![
            "first item".to_owned(),
            "second item".to_owned(),
            "third item".to_owned()
        ],
        "the marker is not in the text any more, and nothing else changed:\n{report}"
    );
    assert_eq!(numbered_of(&document), vec![0, 1, 2], "all three are items");
    assert_eq!(
        level_texts(&document.numbering),
        vec!["•".to_owned()],
        "and the character the reader drew is in the numbering definition"
    );
    assert!(report.contains("list.inferred"), "{report}");
}

/// **One** bullet is not a list. A page can carry a single `•` in the middle of a
/// sentence, and eating that character would be a deleted character for nothing.
#[test]
fn one_bullet_is_not_a_list() {
    let (document, report) = convert_pdf(&bulleted(&["alone"], 40.0, 54.0));
    assert!(
        numbered_of(&document).is_empty(),
        "a list of one is a bullet in a sentence:\n{report}"
    );
    assert_eq!(
        texts_of(&document),
        vec!["•alone".to_owned()],
        "and the text is intact"
    );
}

/// A marker with no gap is part of a word, not a marker.
#[test]
fn a_marker_glued_to_its_word_is_not_a_marker() {
    // `text_x` is 43, three points after the bullet's advance at 40: kerning, not
    // the half-line gap `ListRules::min_gap_ratio` asks for.
    let (document, report) = convert_pdf(&bulleted(&["first", "second"], 40.0, 47.0));
    assert!(
        numbered_of(&document).is_empty(),
        "the gap is below the floor:\n{report}"
    );
    let texts = texts_of(&document).join(" ");
    assert!(
        texts.contains("first") && texts.contains("second"),
        "and both items are still in the text: {texts}"
    );
}

/// Two runs separated by a paragraph are two lists: one `w:num` each, so Word
/// restarts the second one instead of continuing the first.
#[test]
fn two_runs_separated_by_a_paragraph_are_two_lists() {
    let pieces = vec![
        Piece::bullet(40.0, 180.0),
        Piece::at("one", 54.0, 180.0),
        Piece::bullet(40.0, 166.0),
        Piece::at("two", 54.0, 166.0),
        Piece::at("a plain line", 40.0, 152.0),
        Piece::bullet(40.0, 138.0),
        Piece::at("three", 54.0, 138.0),
        Piece::bullet(40.0, 124.0),
        Piece::at("four", 54.0, 124.0),
    ];
    let (document, report) = convert_pdf(&page_pdf(&pieces));
    assert_eq!(
        level_texts(&document.numbering).len(),
        2,
        "two definitions, so the second list restarts:\n{report}"
    );
    assert_eq!(
        numbered_of(&document),
        vec![0, 1, 3, 4],
        "the plain line in the middle is not an item"
    );
}

/// The number Word will draw for each numbered paragraph, counted from the
/// `w:num` definitions the document carries.
///
/// This is the whole point of the feature and it is worth computing rather than
/// asserting: a `w:numPr` asks Word to *count*, so a document whose PDF said
/// `1, 2, 3, 7, 8` and whose numbering says `1, 2, 3, 4, 5` looks perfectly
/// plausible to every test that checks "there is a `w:numPr`" and is silently
/// wrong to the reader. So this reads `startOverride` and counts.
fn drawn_numbers(document: &Document) -> Vec<u32> {
    let starts: std::collections::BTreeMap<u32, u32> = document
        .numbering
        .nums()
        .map(|num| {
            let start = num
                .overrides
                .iter()
                .find(|over| over.start_override.is_some())
                .and_then(|over| over.start_override)
                .unwrap_or(1);
            (num.num_id.0, start)
        })
        .collect();
    let mut drawn: Vec<u32> = Vec::new();
    let mut used: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::default();
    for block in &document.body.blocks {
        let Some(paragraph) = block.as_paragraph() else {
            continue;
        };
        let Some(num_id) = paragraph.props.numbering.as_ref().and_then(|n| n.num_id) else {
            continue;
        };
        let start = starts.get(&num_id.0).copied().unwrap_or(1);
        // Word's number for a paragraph is its position among the paragraphs that
        // share its `w:numId`, counted from that `w:num`'s start — so the count is
        // made here, over the document, exactly as Word makes it.
        let offset = used.entry(num_id.0).or_insert(0);
        drawn.push(start + *offset);
        *offset += 1;
    }
    drawn
}

/// A numbered run becomes a list, and **the number the PDF drew is the number Word
/// will draw** — including where the sequence skips.
#[test]
fn a_numbered_list_becomes_a_list_with_its_own_numbers() {
    let (document, report) = convert_pdf(&numbered_items(
        &["first", "second", "third"],
        &["1.", "2.", "3."],
        40.0,
        58.0,
    ));
    assert_eq!(
        drawn_numbers(&document),
        vec![1, 2, 3],
        "1, 2, 3 as drawn:\n{report}"
    );
    assert_eq!(numbered_of(&document), vec![0, 1, 2], "all three are items");
    assert_eq!(
        level_texts(&document.numbering),
        vec!["%1.".to_owned()],
        "and the marker the reader drew is a pattern Word counts, not a literal"
    );
    let texts = texts_of(&document).join(" ");
    assert!(
        !texts.contains('1'),
        "the marker is not in the text any more, so it is not printed twice: {texts}"
    );
    assert!(
        report.contains("pinned"),
        "and the report says why: {report}"
    );
}

/// **The test this feature exists for.** A PDF list that skips a number must come
/// back with that number, not with a count.
///
/// `1, 2, 3, 7, 8` is not a strange document: it is every list that continues an
/// earlier section, and it is the exact case the first version refused to touch
/// because Word would renumber it silently. Two `w:num`s with `startOverride` are
/// the answer, and this is the assertion that they work.
#[test]
fn a_skipped_number_is_pinned_rather_than_counted() {
    let (document, report) = convert_pdf(&numbered_items(
        &["first", "second", "third", "fourth", "fifth"],
        &["1.", "2.", "3.", "7.", "8."],
        40.0,
        58.0,
    ));
    let drawn = drawn_numbers(&document);
    assert_eq!(
        drawn,
        vec![1, 2, 3, 7, 8],
        "the jump is the whole point, and it must survive into the document:\n{report}"
    );
    // Two definitions, because one `w:num` cannot skip: three items under a `w:num`
    // that starts at one, then two under one that starts at seven.
    let starts: Vec<u32> = document
        .numbering
        .nums()
        .filter_map(|num| {
            num.overrides
                .iter()
                .find(|over| over.start_override.is_some())
                .and_then(|over| over.start_override)
        })
        .collect();
    assert_eq!(
        starts,
        vec![1, 7],
        "the skip is a second list, not a count that runs on:\n{report}"
    );
}

/// A list that **restarts** is the same problem: `1, 2` twice over.
#[test]
fn a_restarted_number_is_pinned_rather_than_counted() {
    let (document, report) = convert_pdf(&numbered_items(
        &["first", "second", "third", "fourth"],
        &["1.", "2.", "1.", "2."],
        40.0,
        58.0,
    ));
    assert_eq!(
        drawn_numbers(&document),
        vec![1, 2, 1, 2],
        "a restart is a second list, not a count that continues:\n{report}"
    );
}

/// Letters count the same way numbers do, and `c)` is three.
#[test]
fn a_lettered_list_counts_letters() {
    let (document, report) = convert_pdf(&numbered_items(
        &["first", "second", "third"],
        &["a)", "b)", "c)"],
        40.0,
        58.0,
    ));
    assert_eq!(
        drawn_numbers(&document),
        vec![1, 2, 3],
        "a, b, c is 1, 2, 3"
    );
    let formats: Vec<String> = document
        .numbering
        .abstracts()
        .flat_map(|abstract_num| abstract_num.levels.iter())
        .filter_map(|level| level.format.as_ref().map(ToString::to_string))
        .collect();
    assert_eq!(formats, vec!["lowerLetter".to_owned()], "{report}");
    assert_eq!(level_texts(&document.numbering), vec!["%1)".to_owned()]);
}

/// A number in a sentence is not a list item. `2024.` at the start of a line is a
/// year, and eating it into a numbering definition would be a deleted year.
#[test]
fn a_year_at_the_start_of_a_line_is_not_a_list_item() {
    let (document, report) = convert_pdf(&numbered_items(
        &["was written", "and revised"],
        &["2024.", "2025."],
        40.0,
        58.0,
    ));
    assert!(
        numbered_of(&document).is_empty(),
        "a three-digit number opens no list:\n{report}"
    );
    let texts = texts_of(&document).join(" ");
    assert!(texts.contains("2024."), "and it stays in the text: {texts}");
}

/// The indentation is the **text's** left edge with a hanging indent back to the
/// marker, because that is what a `w:numPr` paragraph means.
#[test]
fn a_list_item_carries_the_geometry_the_pdf_had() {
    let (document, report) = convert_pdf(&bulleted(&["first", "second"], 40.0, 54.0));
    let paragraphs: Vec<_> = document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter(|paragraph| paragraph.props.numbering.is_some())
        .collect();
    assert_eq!(paragraphs.len(), 2, "{report}");
    let indent = paragraphs[0]
        .props
        .indentation
        .as_ref()
        .expect("an item is indented");
    let start = indent.start.expect("a start indent").0;
    let hanging = indent.hanging.expect("a hanging indent").0;
    // 52 pt is 1040 twips, and the marker is 12 pt to its left, so the marker lands
    // at 40 pt — where the PDF drew it, and not one hanging indent further out.
    assert_eq!(start, 1080, "the text starts where the PDF put it");
    assert_eq!(hanging, 280, "and the marker hangs 14 pt to its left");
    assert_eq!(
        paragraphs[0]
            .props
            .numbering
            .as_ref()
            .and_then(|n| n.num_id),
        paragraphs[1]
            .props
            .numbering
            .as_ref()
            .and_then(|n| n.num_id),
        "both items are in the same list"
    );
}

/// The rules are the caller's, and a caller who wants longer lists than this gets
/// text.
#[test]
fn a_caller_who_wants_longer_lists_gets_text() {
    let bytes = bulleted(&["first", "second"], 40.0, 54.0);
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let converted = convert(
        &mut document,
        &PdfOptions {
            mode: Mode::Semantic,
            lists: ListRules {
                min_items: 3,
                ..ListRules::default()
            },
            ..PdfOptions::default()
        },
    )
    .expect("convert");
    assert!(
        numbered_of(&converted.document).is_empty(),
        "two items are below the caller's floor of three"
    );
    let report = converted.report.to_string();
    assert!(
        !report.contains("list.inferred"),
        "and nothing was reported: {report}"
    );
}
