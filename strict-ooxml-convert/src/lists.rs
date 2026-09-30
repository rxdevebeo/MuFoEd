//! Lists out of a marker the PDF drew as text (`P-8`).
//!
//! # What a bulleted item looks like to this reader
//!
//! Not like a paragraph. A `Do`-less PDF writes a bullet and its text as two text
//! runs with a **real gap** between them, and this reader's line grouping is
//! pen-continuity: a glyph that does not start where the previous advance ended
//! begins a new line. So an item arrives as **two lines on one baseline** — `•`
//! on one, `the text` on the other — and the paragraph rules, which break a
//! paragraph at an x jump of half a body size, make those two lines two
//! paragraphs. The first version of this feature assumed the marker was the first
//! glyphs of the item's line and found nothing: the gap that identifies a marker is
//! exactly the gap that splits the line.
//!
//! So the unit here is a **pair of lines sharing a baseline**, the left one nothing
//! but a marker, the right one the text.
//!
//! # Why this is the delicate inference in the converter
//!
//! Every other inference here *adds* something the PDF did not say: a paragraph
//! boundary, a table grid, a heading level. This one **moves** a character the
//! reader drew, and it also **joins two paragraphs into one**. A rule that misfires
//! does not add a wrong structure — it deletes a character and merges two pieces of
//! text that were not one. So the rule below is built to be wrong rarely, to say so
//! when it is wrong, and to refuse the half it cannot do safely.
//!
//! # What a marker has to look like
//!
//! Four measurements, no tastes:
//!
//! 1. **it is a bullet character** — a literal that `w:lvlText` reproduces exactly,
//!    so what Word draws is what the PDF drew. Numbers are *not* in this set: a
//!    numbered list needs Word to generate 1, 2, 3, and a PDF's list that restarts
//!    mid-page or skips a number would then be silently renumbered. Those are
//!    detected and reported (`list.numbered`) and left as text, which is a smaller
//!    claim than a wrong one;
//! 2. **it stands alone on its baseline** and the text follows to its right after a
//!    **gap** — half a body size at least, four body sizes at most. A marker glued
//!    to its word is part of a word, and a marker followed by a tab stop 200 pt away
//!    is a table of contents entry;
//! 3. **at least [`ListRules::min_items`] items in a row** share the character and
//!    the text position, with only continuation lines between them. One bullet on a
//!    page is a bullet in a sentence; three in a row at the same x are a list. This
//!    is the rule that does most of the work and the reason a run is required;
//! 4. **there is text after it**. A line that is only a marker is a separator.
//!
//! # What the output says
//!
//! One paragraph per item, carrying `w:numPr`; one `w:abstractNum` per run, with
//! `w:numFmt="bullet"`, a `w:lvlText` holding the character as drawn, and the
//! hanging indent the PDF had. The layout engine already evaluates all of that
//! (`render-svg`'s `numbering` module), so a rendered page keeps the marker, its
//! position and the text: the character moved from the run into the numbering, and
//! the report names every item that moved.

use std::collections::BTreeMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_wml::model::ids::{AbstractNumId, Ilvl, NumId};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, Num, NumberingTable};
use strict_ooxml_wml::model::props::ParagraphProperties;
use strict_ooxml_wml::model::values::{Indentation, Twips};

use crate::report::{ConversionReport, Severity};
use crate::to_twips;

/// The thresholds that turn a drawn marker into a list item.
///
/// Public and named, like [`crate::TableRules`], because a threshold nobody can
/// read is a threshold nobody can argue with. Every default carries its reason.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListRules {
    /// How many items in a row make a list. Default 2.
    ///
    /// One is not a list: a page can carry a single `•` in the middle of a
    /// sentence, in a table of contents, or as the only content of a callout. Two
    /// in a row at the same position is a list in every document this project has
    /// seen, and below two the cost of a false positive — a deleted character and
    /// two paragraphs merged — is higher than the cost of a false negative, which
    /// is text that stays text.
    pub min_items: usize,

    /// The shortest gap between a marker and its text, as a share of the body size.
    /// Default 0.5.
    ///
    /// Half a line's height is the smallest gap a typesetter leaves on purpose; a
    /// smaller one is kerning, and a marker with no gap is part of a word.
    pub min_gap_ratio: f64,

    /// The largest gap that still counts as one item, in body sizes. Default 4.
    ///
    /// A marker followed by a tab stop 200 pt away is a contents entry with a
    /// leader, not a list item.
    pub max_gap_ratio: f64,

    /// How far the text may start from one item to the next, in points. Default 3.
    ///
    /// Items of one list line up; if the text starts somewhere else they are two
    /// lists, and treating them as one would give the second a hanging indent the
    /// PDF never had.
    pub same_text_tolerance_pt: f64,
}

impl Default for ListRules {
    fn default() -> Self {
        Self {
            min_items: 2,
            min_gap_ratio: 0.5,
            max_gap_ratio: 4.0,
            same_text_tolerance_pt: 3.0,
        }
    }
}

/// The bullet characters this converter will move into a numbering definition.
///
/// A closed set, and every entry appears **alone** at the start of a line in real
/// documents. `:` and `,` are deliberately absent: `Q:` and `Note,` open lines in
/// every FAQ ever written, and a rule that ate them would eat real words.
const BULLETS: &[char] = &[
    '•', // •
    '‣', // ‣ triangular
    '▪', // ▪ small square
    '■', // ■ square
    '·', // · middle dot
    '–', // – en dash
    '—', // — em dash
    '-', // - hyphen
    '*', // * asterisk
    '+', // + plus
    'o', // o, the hollow bullet of a plain-text list
];

/// One candidate list item: the line that is nothing but a marker, and the line
/// that is its text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    /// Index of the marker's line.
    pub marker_line: usize,
    /// The marker, as drawn.
    pub text: String,
    /// Where the marker sits, points from the page's left.
    pub marker_x: f64,
    /// Where the text starts, points from the page's left.
    pub text_x: f64,
    /// The numbering instance this item belongs to, once the runs are numbered.
    pub num_id: NumId,
    /// Whether the item made it into a run.
    ///
    /// A candidate that failed the run test keeps its place in the map — the marker's
    /// line and its text are still one paragraph, which is how the converter behaved
    /// before lists existed — but gets no w:numPr, because a w:numId that
    /// resolves to nothing is a document Word repairs by guessing.
    pub numbered: bool,
}

/// The items of one stretch of a page's flow, by the index of their marker line.
pub(crate) fn items_of(lines: &[GlyphLine], body: f64, rules: &ListRules) -> BTreeMap<usize, Item> {
    let mut out = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(marker) = bare_marker(line) else {
            continue;
        };
        let text_index = index + 1;
        let Some(text) = lines.get(text_index) else {
            continue;
        };
        if !same_baseline(line, text) {
            continue;
        }
        let gap = text.x - (marker.x + marker.advance);
        if gap < rules.min_gap_ratio * body || gap > rules.max_gap_ratio * body {
            continue;
        }
        out.insert(
            text_index,
            Item {
                marker_line: index,
                text: marker.text,
                marker_x: marker.x,
                text_x: text.x,
                // Both are set by `apply`, once the run this item belongs to is
                // known — or found not to exist.
                num_id: NumId(0),
                numbered: false,
            },
        );
    }
    out
}

/// A line that is nothing but a run of one marker character.
struct BareMarker {
    text: String,
    x: f64,
    advance: f64,
}

fn bare_marker(line: &GlyphLine) -> Option<BareMarker> {
    let first = line.glyphs.first()?;
    let character = first.text.chars().next()?;
    if !BULLETS.contains(&character) {
        return None;
    }
    if line
        .glyphs
        .iter()
        .any(|glyph| !glyph.text.starts_with(character))
    {
        // `**bold**` at the start of a line is emphasis, and a line of five
        // asterisks is a rule.
        return None;
    }
    let last = line.glyphs.last()?;
    Some(BareMarker {
        text: character.to_string(),
        x: line.x,
        advance: last.x + last.width - line.x,
    })
}

fn same_baseline(left: &GlyphLine, right: &GlyphLine) -> bool {
    (left.baseline - right.baseline).abs() < 0.5
}

/// The runs of consecutive items, in page order, each with a `w:num` of its own.
///
/// Two items are in one run when nothing but **continuation lines** stands between
/// them: a wrapped item's second line starts at the item's own text x, and a
/// paragraph that starts anywhere else ends the list. That is the difference
/// between one list of four wrapped items and two lists of two, and the second
/// reading is the one that gives the reader the document the PDF drew.
pub(crate) fn runs_of(
    items: &BTreeMap<usize, Item>,
    lines: &[GlyphLine],
    rules: &ListRules,
) -> Vec<Vec<usize>> {
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for index in items.keys().copied() {
        let item = &items[&index];
        let continues = current.last().is_some_and(|last| {
            let before = &items[last];
            before.text == item.text
                && (before.text_x - item.text_x).abs() <= rules.same_text_tolerance_pt
                // The lines between the previous item's text and this item's
                // marker: `before.marker_line + 2` is the line after that text, and
                // `index - 1` is this item's marker line, which is not a
                // continuation and so is the exclusive end.
                && only_continuations(
                    lines,
                    before.marker_line + 2,
                    index.saturating_sub(1),
                    item.text_x,
                )
        });
        if !continues {
            if current.len() >= rules.min_items {
                runs.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
        current.push(index);
    }
    if current.len() >= rules.min_items {
        runs.push(current);
    }
    runs
}

/// Whether every line in `from..to` starts at `text_x`, i.e. is a wrapped
/// continuation of the item that ended at `from`.
fn only_continuations(lines: &[GlyphLine], from: usize, to: usize, text_x: f64) -> bool {
    lines
        .get(from..to)
        .unwrap_or_default()
        .iter()
        .all(|line| (line.x - text_x).abs() <= 3.0)
}

/// Fills each run's items with a `w:num` of its own, records what it did, and
/// **drops the items that are not in a run**.
///
/// Dropping is the point, not a tidy-up: a candidate that failed the run test —
/// one bullet on a page, two with different markers — must come out of the map
/// entirely, or the paragraph builder would give it a `w:num` pointing at a
/// definition nobody wrote. A `w:numId` that resolves to nothing is a document
/// Word repairs by guessing, and a document Word repairs by guessing is a document
/// nobody can check.
pub(crate) fn apply(
    runs: &[Vec<usize>],
    items: &mut BTreeMap<usize, Item>,
    numbering: &mut NumberingTable,
    report: &mut ConversionReport,
    page: usize,
) {
    let mut abstract_id = next_abstract_id(numbering);
    let mut num_id = next_num_id(numbering);
    for run in runs {
        let Some(first) = run.first().and_then(|index| items.get(index)) else {
            continue;
        };
        let marker = first.text.clone();
        let text_x = first.text_x;
        let marker_x = first.marker_x;
        let level = Level {
            format: Some(Arc::from("bullet")),
            text: Some(Arc::from(marker.as_str())),
            suffix: Some(Arc::from("space")),
            paragraph: ParagraphProperties {
                // The hanging indent the PDF had, so a rendered page keeps the
                // marker in the margin the first line opens instead of
                // re-flowing into a staircase.
                indentation: Some(Indentation {
                    start: Some(Twips(to_twips(text_x))),
                    hanging: Some(Twips(to_twips(text_x - marker_x))),
                    ..Indentation::default()
                }),
                ..ParagraphProperties::default()
            },
            ..Level::new(Ilvl(0))
        };
        numbering.insert_abstract(AbstractNum {
            id: AbstractNumId(abstract_id),
            multi_level_type: Some(Arc::from("singleLevel")),
            num_style_link: None,
            style_link: None,
            levels: vec![level],
            location: SourceLocation::unknown(),
        });
        numbering.insert_num(Num {
            num_id: NumId(num_id),
            abstract_num_id: AbstractNumId(abstract_id),
            overrides: Vec::new(),
            location: SourceLocation::unknown(),
        });
        for index in run {
            if let Some(item) = items.get_mut(index) {
                item.num_id = NumId(num_id);
                item.numbered = true;
            }
        }
        report.record(
            "list.inferred",
            Severity::Inferred,
            format!(
                "page {page}: {} item(s) marked with `{marker}` became one list, and the \
                 character moved out of the text into the numbering definition, so nothing is \
                 deleted and Word draws the same character",
                run.len()
            ),
        );
        abstract_id += 1;
        num_id += 1;
    }
}

/// Records the numbered lists this converter found and deliberately did not act on.
///
/// The rule is the same as for bullets — a run of lines opening with the same
/// token — but the token is a number or a letter, and acting on it would mean
/// asking Word to generate the sequence. Reported, not done, and the report says
/// why.
pub(crate) fn record_numbered(
    lines: &[GlyphLine],
    body: f64,
    rules: &ListRules,
    report: &mut ConversionReport,
    page: usize,
) {
    let mut index = 0usize;
    while index < lines.len() {
        let Some((token, text_line)) = numbered_token(lines, index, body, rules) else {
            index += 1;
            continue;
        };
        let mut run = vec![index];
        let mut next = text_line;
        while let Some((_, after)) = numbered_token(lines, next + 1, body, rules) {
            // The items' **text** positions, not their markers: comparing a marker
            // to the previous item's text measures the marker gap, which is the
            // one thing every item shares by construction.
            if (lines[after].x - lines[next].x).abs() > rules.same_text_tolerance_pt {
                break;
            }
            run.push(next + 1);
            next = after;
        }
        if run.len() >= rules.min_items {
            report.record(
                "list.numbered",
                Severity::Inferred,
                format!(
                    "page {page}: {} line(s) open with `{token}` and look like a numbered list, \
                     and the marker was left in the text: a `w:numPr` would ask Word to generate \
                     the sequence, and a list that restarts or skips a number would then be \
                     silently renumbered",
                    run.len()
                ),
            );
        }
        index = next + 1;
    }
}

/// A leading `1.` or `a)` and the index of the line its text starts on.
fn numbered_token(
    lines: &[GlyphLine],
    index: usize,
    body: f64,
    rules: &ListRules,
) -> Option<(String, usize)> {
    let line = lines.get(index)?;
    let first = line.glyphs.first()?;
    let head = first.text.chars().next()?;
    if !(head.is_ascii_digit() || head.is_ascii_alphabetic()) {
        return None;
    }
    let second = line.glyphs.get(1)?.text.chars().next()?;
    if second != '.' && second != ')' {
        return None;
    }
    if head.is_ascii_digit() {
        let digits = line
            .glyphs
            .iter()
            .take_while(|glyph| {
                glyph
                    .text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            })
            .count();
        if digits > 2 {
            // A three-digit number at the start of a line is a number in a
            // sentence, not an item number.
            return None;
        }
    }
    let text_line = index + 1;
    let text = lines.get(text_line)?;
    if !same_baseline(line, text) {
        return None;
    }
    let last = line.glyphs.get(1)?;
    let gap = text.x - (last.x + last.width);
    if gap < rules.min_gap_ratio * body || gap > rules.max_gap_ratio * body {
        return None;
    }
    let token: String = line.glyphs[..2]
        .iter()
        .filter_map(|glyph| glyph.text.chars().next())
        .collect();
    Some((token, text_line))
}

fn next_abstract_id(numbering: &NumberingTable) -> u32 {
    numbering
        .abstracts()
        .map(|abstract_num| abstract_num.id.0)
        .max()
        .unwrap_or(0)
        + 1
}

fn next_num_id(numbering: &NumberingTable) -> u32 {
    numbering.nums().map(|num| num.num_id.0).max().unwrap_or(0) + 1
}

/// Marks `paragraph` as an item of `item`'s list, with the geometry the PDF had.
///
/// The indent is the **text's** left edge with a hanging indent back to the marker,
/// because that is what a `w:numPr` paragraph means: the marker is drawn in the
/// margin the hanging indent opens, and the text starts where the PDF put it.
/// Putting the marker's x in `w:ind/@start` instead would draw the marker one
/// hanging indent too far left — the kind of half-point error a screenshot gate
/// finds and a unit test does not.
pub(crate) fn mark(paragraph: &mut strict_ooxml_wml::model::block::Paragraph, item: &Item) {
    use strict_ooxml_wml::model::props::NumPr;

    paragraph.props.numbering = Some(NumPr {
        num_id: Some(item.num_id),
        ilvl: Some(Ilvl(0)),
    });
    paragraph.props.indentation = Some(Indentation {
        start: Some(Twips(to_twips(item.text_x))),
        hanging: Some(Twips(to_twips(item.text_x - item.marker_x))),
        ..Indentation::default()
    });
}
