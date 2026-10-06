//! The semantic mode: reconstruct what the document *is*.
//!
//! # The chain of inferences
//!
//! ```text
//! glyphs -> lines -> paragraphs -> runs -> (heading | body)
//! ```
//!
//! Every arrow is a judgement, and each one below names the number it uses:
//!
//! 1. **glyphs to lines** — exact. The reader groups by baseline and by pen
//!    continuity, both of which the PDF states.
//! 2. **lines to paragraphs** — a line starts a new paragraph when the gap to
//!    the previous one exceeds [`ParagraphRules::paragraph_gap_ratio`] times the
//!    body line pitch, or when the left edge moves by more than
//!    [`ParagraphRules::indent_ratio`] times the body size, or when the line
//!    carries different formatting. This is the judgement a reader of the
//!    original would most want to check, so the thresholds are public.
//! 3. **paragraphs to runs** — exact. A run is a maximal set of consecutive
//!    glyphs with the same font, size and colour, all of which the PDF states.
//! 4. **paragraphs to headings** — a paragraph whose size exceeds
//!    [`ParagraphRules::heading_ratio`] times the body size becomes a heading.
//!    Recorded as [`Severity::Inferred`] every time, because a large first line
//!    is a title at least as often as it is a heading.
//!
//! # Columns
//!
//! Two-column pages are reordered when a **measured gutter** separates two
//! vertically overlapping clusters (`columns` / AUD-85). Without that gap the
//! page stays in draw order. Three-plus columns, uneven widths, and a floating
//! picture in the column band are recorded as [`Severity::Unsupported`].
//!
//! Absolute positioning and vertical alignment are not reconstructed.
//!
//! Lists too, with a twist: in a PDF a bullet *is* text.
//! Turning it into a `w:numPr` would mean deleting the character the reader drew
//! and writing another one in its place, so the marker stays in the text and the
//! report says nothing was inferred. A later increment may take that on with a
//! rule for what a marker looks like; until then the text is the truth.

use std::sync::Arc;

use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_pdf::{Glyph, PdfPage};
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::drawing::{Drawing, DrawingKind, Extent, Graphic, InlineDrawing};
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::props::{ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::values::Spacing;
use strict_ooxml_wml::model::values::{
    Color, Emu, Fonts, HalfPoints, Indentation, Justification, Rsids, Space, Twips,
};
use strict_ooxml_wml::model::Document;

use crate::geometry::section_for;
use crate::media::MediaCollector;
use crate::recover::Recovered;
use crate::report::{ConversionReport, Severity};
use crate::sections::SectionAccumulator;
use crate::tables;
use crate::{to_half_points, to_twips, PdfOptions};

/// The thresholds that turn geometry into structure.
///
/// Public, because a threshold is a claim about what documents look like, and a
/// claim nobody can change is a claim nobody can check.
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphRules {
    /// A vertical gap above this multiple of the body line pitch starts a new
    /// paragraph. Default 1.35: a larger leading gap is usually paragraph
    /// spacing, and a smaller one is usually leading.
    pub paragraph_gap_ratio: f64,
    /// A horizontal move of more than this multiple of the body size starts a new
    /// paragraph. Default 0.6: a smaller move is usually a centre or a tab stop
    /// inside the same paragraph.
    pub indent_ratio: f64,
    /// A paragraph larger than this multiple of the body size is a heading.
    /// Default 1.15.
    pub heading_ratio: f64,
}

impl Default for ParagraphRules {
    fn default() -> Self {
        Self {
            paragraph_gap_ratio: 1.35,
            indent_ratio: 0.6,
            heading_ratio: 1.15,
        }
    }
}

/// The list items of one stretch of a page's flow, by the index of their marker
/// line.
///
/// Both kinds are decided here and both belong to the same place: a bulleted run
/// becomes one numbering definition per run, and a **numbered** run becomes one
/// definition per unbroken piece of it, with the number pinned
/// (`w:startOverride`) so that a restart or a skip in the PDF cannot come back
/// renumbered. A stretch rather than a page, because the flow is cut at every
/// table and a list cannot cross one.
fn list_items_of(
    flow: &[GlyphLine],
    body: f64,
    options: &PdfOptions,
    numbering: &mut strict_ooxml_wml::model::numbering::NumberingTable,
    report: &mut ConversionReport,
    page: usize,
) -> std::collections::BTreeMap<usize, crate::lists::Item> {
    let mut items = crate::lists::items_of(flow, body, &options.lists);
    let runs = crate::lists::runs_of(&items, flow, &options.lists);
    crate::lists::apply(&runs, &mut items, numbering, report, page);
    crate::lists::apply_numbered(
        flow,
        body,
        &options.lists,
        &mut items,
        numbering,
        report,
        page,
    );
    items
}

/// Builds the semantic document.
pub(crate) fn build(
    pages: &[PdfPage],
    recovered: &Recovered,
    options: &PdfOptions,
    report: &mut ConversionReport,
) -> (Document, Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>) {
    let mut media = MediaCollector::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut sections = SectionAccumulator::new();
    // Lists the pages' markers added. The table is filled while the blocks are
    // built and moves into the document at the end, because a `w:numPr` paragraph
    // and the `numbering.xml` that defines its marker have to arrive together.
    let mut numbering = strict_ooxml_wml::model::numbering::NumberingTable::new();

    for (index, page) in pages.iter().enumerate() {
        // A size or margin change closes the previous page before these blocks
        // are appended. Identical neighbours stay in the open section.
        sections.begin_page(&mut blocks, section_for(page, false), false);
        let raw_lines = strict_ooxml_pdf::text::lines(page.items());
        report.lines += raw_lines.len();
        let column_plan = crate::columns::plan(page, &raw_lines, report);
        let lines = crate::columns::apply(raw_lines, &column_plan);
        let body = body_size(&lines);
        let pitch = body_pitch(&lines);
        let plan = tables::plan(
            page,
            index + 1,
            &lines,
            &options.tables,
            options.max_table_lines,
            options.max_table_cells,
            report,
        );

        // A table takes the place of the text inside it, and the page's own flow
        // runs around it: the paragraphs before it, the table, the paragraphs
        // after. The table sits where its first line was, which is the only
        // order the page states.
        let mut flow: Vec<GlyphLine> = Vec::new();
        for (number, line) in lines.iter().enumerate() {
            if let Some(table) = plan.starts_table(number) {
                let list_items =
                    list_items_of(&flow, body, options, &mut numbering, report, index + 1);
                push_paragraphs(
                    &mut blocks,
                    &flow,
                    body,
                    pitch,
                    options,
                    0.0,
                    true,
                    report,
                    index + 1,
                    &list_items,
                );
                // The paragraphs above the table are done; keeping them would put
                // the page's text in the document twice.
                flow.clear();
                blocks.push(table_block(
                    &plan.tables[table],
                    body,
                    pitch,
                    options,
                    report,
                    index + 1,
                ));
                continue;
            }
            if plan.inside_table(number) {
                continue;
            }
            flow.push(line.clone());
        }
        let list_items = list_items_of(&flow, body, options, &mut numbering, report, index + 1);
        push_paragraphs(
            &mut blocks,
            &flow,
            body,
            pitch,
            options,
            0.0,
            true,
            report,
            index + 1,
            &list_items,
        );

        if options.embed_images {
            blocks.extend(images_of(page, &mut media, report, options));
        }
        blocks.extend(crate::vectors::blocks_for(page, report));
        // A page a model read stands where the region it came from was: after
        // whatever the PDF itself gave (nothing, or the caption under the
        // picture) and before the pictures, indented to the region's left edge.
        for block in recovered.page(index) {
            for paragraph in &block.paragraphs {
                blocks.push(Block::Paragraph(paragraph.clone()));
            }
        }
    }

    let mut document = crate::empty_document();
    document.body = strict_ooxml_wml::model::document::Body { blocks };
    document.sections = sections.finish();
    media.register(&mut document.media);
    // The numbering the lists asked for, with the `w:numPr` paragraphs above.
    document.numbering = numbering;
    report.paragraphs = document
        .body
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Paragraph(_)))
        .count();
    report.images = media.len();
    (document, media.into_parts())
}

/// Turns a run of lines into paragraphs.
///
/// The page's flow and a table cell's content go through this one function, so a
/// cell is a small document rather than a special case: the same gap, indent and
/// format rules decide where its paragraphs begin.
///
/// `origin` is the x a paragraph's indent is measured from - the page's left
/// edge in the page's flow, the cell's left edge inside a cell, where an indent
/// means nothing without it. `allow_headings` is false in a cell: a large first
/// line in a table is a label, and a heading inside a cell is a claim no PDF
/// supports.
#[allow(clippy::too_many_arguments)]
fn paragraphs_of(
    lines: &[GlyphLine],
    body: f64,
    pitch: f64,
    options: &PdfOptions,
    origin: f64,
    allow_headings: bool,
    report: &mut ConversionReport,
    page: usize,
    items: &std::collections::BTreeMap<usize, crate::lists::Item>,
) -> Vec<Paragraph> {
    let mut out: Vec<Paragraph> = Vec::new();
    let mut paragraph: Option<Paragraph> = None;
    let mut previous: Option<&GlyphLine> = None;
    for (at, line) in lines.iter().enumerate() {
        // Is this line nothing but the marker of an item whose text is the next
        // line? And is this line the text of the marker above it? The two
        // questions have different keys, and the difference is the whole feature:
        // the gap that makes a marker recognisable is also the gap that splits the
        // item's line in two, so an item is a **pair of lines on one baseline** and
        // this is where they become one paragraph again.
        let marker_item = items.get(&(at + 1)).filter(|item| item.marker_line == at);
        let is_marker = marker_item.is_some();
        let is_item_text = items.get(&at).is_some();
        let with_its_marker = is_item_text
            && previous.is_some_and(|previous| (previous.baseline - line.baseline).abs() < 0.5);
        // A marker's line always begins a paragraph, whatever the gap to the line
        // above says: it is a structural boundary, and a paragraph that swallowed
        // one would end up holding two items.
        let starts_new = is_marker
            || (!with_its_marker
                && previous.is_some_and(|previous| {
                    let gap = line.baseline - previous.baseline;
                    let moved = (line.x - previous.x).abs();
                    gap > pitch * options.paragraphs.paragraph_gap_ratio
                        || moved > body * options.paragraphs.indent_ratio
                        || !same_format(previous, line)
                }));
        if starts_new || paragraph.is_none() {
            if let Some(finished) = paragraph.take() {
                out.push(finished);
            }
            let heading = if allow_headings {
                heading_level(line, body, &options.paragraphs)
            } else {
                None
            };
            if let Some(level) = heading {
                report.record(
                    "heading.inferred",
                    Severity::Inferred,
                    format!(
                        "page {page}: {:.1} pt text on a {:.1} pt body became heading {}",
                        line.size,
                        body,
                        level + 1
                    ),
                );
            }
            let mut fresh = new_paragraph(line, heading, origin);
            if let Some(item) = marker_item.filter(|item| item.numbered) {
                crate::lists::mark(&mut fresh, item);
            }
            paragraph = Some(fresh);
        } else if let Some(paragraph) = paragraph.as_mut() {
            set_line_gap(paragraph, line, body);
        }
        // The marker's line is *all* marker, and the character is in the
        // numbering definition now — see lists for why that moves a character
        // rather than deleting one. Nothing of that line is pushed as text.
        // Only an accepted item's marker leaves the text; a rejected candidate keeps
        // its character, because the run test said «this is not a list» and that is
        // the whole of what it said.
        let skip = match marker_item {
            Some(item) if item.numbered => line.glyphs.len(),
            _ => 0,
        };
        if let Some(paragraph) = paragraph.as_mut() {
            push_runs_from(paragraph, line, skip);
        }
        previous = Some(line);
    }
    if let Some(finished) = paragraph {
        out.push(finished);
    }
    out
}

/// Appends a run of lines to a block list.
#[allow(clippy::too_many_arguments)]
fn push_paragraphs(
    blocks: &mut Vec<Block>,
    lines: &[GlyphLine],
    body: f64,
    pitch: f64,
    options: &PdfOptions,
    origin: f64,
    allow_headings: bool,
    report: &mut ConversionReport,
    page: usize,
    items: &std::collections::BTreeMap<usize, crate::lists::Item>,
) {
    blocks.extend(
        paragraphs_of(
            lines,
            body,
            pitch,
            options,
            origin,
            allow_headings,
            report,
            page,
            items,
        )
        .into_iter()
        .map(Block::Paragraph),
    );
}

/// Builds a table's cells into the model, each cell's text as paragraphs.
fn table_block(
    planned: &tables::PlannedTable,
    body: f64,
    pitch: f64,
    options: &PdfOptions,
    report: &mut ConversionReport,
    page: usize,
) -> Block {
    let paragraphs = planned
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    paragraphs_of(
                        &cell.lines,
                        body,
                        pitch,
                        options,
                        cell.left,
                        false,
                        report,
                        page,
                        &std::collections::BTreeMap::new(),
                    )
                })
                .collect()
        })
        .collect();
    Block::Table(tables::table_of(planned, paragraphs))
}

/// The size most of a page's text is set in. Shared with the visual mode, which
/// needs the same answer to the same question.
#[must_use]
pub(crate) fn body_size_pub(lines: &[GlyphLine]) -> f64 {
    body_size(lines)
}

/// The line pitch of a page, shared with the visual mode.
#[must_use]
pub(crate) fn body_pitch_pub(lines: &[GlyphLine]) -> f64 {
    body_pitch(lines)
}

/// Appends a line's glyphs as runs, shared with the visual mode: a run is a
/// maximal set of consecutive glyphs with the same font, size and colour, and
/// that rule does not depend on which mode asked for it.
pub(crate) fn push_runs_public(paragraph: &mut Paragraph, line: &GlyphLine) {
    push_runs(paragraph, line);
}

/// A page's images, shared with the visual mode.
pub(crate) fn images_pub(
    page: &PdfPage,
    media: &mut MediaCollector,
    report: &mut ConversionReport,
    options: &PdfOptions,
) -> Vec<Block> {
    images_of(page, media, report, options)
}

/// The size most of a page's text is set in.
///
/// A page whose first line is a title is the case this gets wrong, and the
/// answer is not "the first line": a title is a minority of the page, so the
/// *commonest* size is the body.
#[must_use]
fn body_size(lines: &[GlyphLine]) -> f64 {
    if lines.is_empty() {
        return 12.0;
    }
    let mut sizes: Vec<(u64, usize)> = Vec::new();
    for line in lines {
        let key = (line.size * 10.0).round() as u64;
        match sizes.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 += 1,
            None => sizes.push((key, 1)),
        }
    }
    // Most common first. `Reverse` makes the intent ("descending") part of the
    // key instead of a comparison that has to be read backwards.
    sizes.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let key = sizes.first().map_or(0, |(key, _)| *key);
    lines
        .iter()
        .find(|line| (line.size * 10.0).round() as u64 == key)
        .map_or(12.0, |line| line.size)
}

/// The distance between consecutive baselines, in points.
#[must_use]
fn body_pitch(lines: &[GlyphLine]) -> f64 {
    if lines.len() < 2 {
        return lines.first().map_or(12.0, |line| line.size * 1.2);
    }
    let mut sorted: Vec<f64> = lines.iter().map(|line| line.baseline).collect();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mut gaps: Vec<f64> = sorted
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap > 0.1)
        .collect();
    gaps.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    gaps.first().copied().unwrap_or(12.0)
}

/// Whether two lines are set the same way, which is a paragraph boundary when
/// it is not.
fn same_format(left: &GlyphLine, right: &GlyphLine) -> bool {
    let font = |line: &GlyphLine| line.glyphs.first().map(|glyph| glyph.font.clone());
    let colour = |line: &GlyphLine| line.glyphs.first().map(|glyph| glyph.color.to_rgb8());
    (left.size - right.size).abs() < 0.01
        && font(left) == font(right)
        && colour(left) == colour(right)
}

/// The heading level a line implies, if any.
fn heading_level(line: &GlyphLine, body: f64, rules: &ParagraphRules) -> Option<u8> {
    if body <= 0.0 || line.size <= body * rules.heading_ratio {
        return None;
    }
    // The bands are the ones Word's built-in heading styles use, so a converted
    // outline opens with the right levels selected.
    let ratio = line.size / body;
    if ratio >= 1.8 {
        Some(0)
    } else if ratio >= 1.45 {
        Some(1)
    } else {
        Some(2)
    }
}

/// A paragraph with no runs yet, carrying the paragraph's own indentation.
///
/// `origin` is what the indent is measured from: the page's left edge in the
/// page's flow, a cell's left edge inside one. An indent from the page's edge
/// inside a cell would push the text twice as far right as the PDF put it.
fn new_paragraph(line: &GlyphLine, heading: Option<u8>, origin: f64) -> Paragraph {
    Paragraph {
        props: ParagraphProperties {
            // An indent is the only paragraph-level geometry a PDF states, and
            // keeping it is what stops a converted page from re-flowing.
            indentation: Some(Indentation {
                start: Some(Twips(to_twips(line.x - origin))),
                ..Indentation::default()
            }),
            outline_level: heading,
            keep_next: if heading.is_some() {
                strict_ooxml_wml::model::values::TriState::On
            } else {
                strict_ooxml_wml::model::values::TriState::Absent
            },
            ..ParagraphProperties::default()
        },
        inlines: Vec::new(),
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }
}

/// Sets the leading a wrapped line needs, so the second line lands on the PDF's
/// baseline rather than on the first one's.
fn set_line_gap(paragraph: &mut Paragraph, line: &GlyphLine, body: f64) {
    let pitch = line.size - body;
    paragraph.props.spacing = Some(Spacing {
        line: Some(Twips(to_twips(pitch))),
        ..Spacing::default()
    });
}

/// Appends a line's glyphs as runs.
fn push_runs(paragraph: &mut Paragraph, line: &GlyphLine) {
    push_runs_from(paragraph, line, 0);
}

/// The line's runs, minus its first skip glyphs.
///
/// skip is where a list item's marker goes: the character is not lost, it is in
/// the numbering definition, and pushing it here as well would draw it twice.
fn push_runs_from(paragraph: &mut Paragraph, line: &GlyphLine, skip: usize) {
    let mut current: Option<(RunProperties, String)> = None;
    for glyph in line.glyphs.iter().skip(skip) {
        let properties = run_properties(glyph);
        match current.as_mut() {
            Some((existing, text)) if *existing == properties => text.push_str(&glyph.text),
            _ => {
                if let Some((properties, text)) = current.take() {
                    paragraph.inlines.push(run(properties, text));
                }
                current = Some((properties, glyph.text.clone()));
            }
        }
    }
    if let Some((properties, text)) = current {
        paragraph.inlines.push(run(properties, text));
    }
}

/// The run properties a glyph implies.
#[must_use]
fn run_properties(glyph: &Glyph) -> RunProperties {
    let [red, green, blue] = glyph.color.to_rgb8();
    RunProperties {
        fonts: Some(Fonts {
            ascii: Some(Arc::from(glyph.font.as_str())),
            h_ansi: Some(Arc::from(glyph.font.as_str())),
            ..Fonts::default()
        }),
        size: Some(HalfPoints(to_half_points(glyph.size))),
        size_cs: Some(HalfPoints(to_half_points(glyph.size))),
        color: Some(Color::new(format!("{red:02X}{green:02X}{blue:02X}"))),
        ..RunProperties::default()
    }
}

/// One run with its text.
fn run(properties: RunProperties, text: String) -> Inline {
    // A run whose text begins or ends with a space needs `xml:space`, or the
    // XML parser strips it and the line comes back with its words stuck
    // together - the one way a faithful conversion can still lose text.
    let space = if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
        Space::Preserve
    } else {
        Space::default()
    };
    Inline::Run(Run {
        props: properties,
        content: vec![RunContent::Text(TextNode { text, space })],
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    })
}

/// A page's images, one centred paragraph each.
///
/// When a classifier is supplied, each region is described before it is placed
/// and the description goes into `wp:docPr/@descr`, which is the field a screen
/// reader reads. Everything the model said lands in the report first: a
/// description in a document that the report does not mention is a caption
/// nobody can audit.
fn images_of(
    page: &PdfPage,
    media: &mut MediaCollector,
    report: &mut ConversionReport,
    options: &PdfOptions,
) -> Vec<Block> {
    let page_number = page.number;
    let mut out = Vec::new();
    for item in page.items() {
        let strict_ooxml_pdf::Item::Image(image) = item else {
            continue;
        };
        if image.width < 1.0 || image.height < 1.0 {
            continue;
        }
        let Some(encoded) = image.image.as_ref() else {
            if let Some(missing) = &image.missing {
                report.record("image.lost", Severity::Lost, missing.clone());
            }
            continue;
        };
        // The description comes first: it only needs the image, while storing it
        // needs an encoding, and a failed encoding must not cost the model's
        // words as well as the picture.
        let description = describe_region(image, options, report, page_number);
        let part = match media.add(out.len(), encoded) {
            Ok(part) => part,
            Err(reason) => {
                report.record("image.lost", Severity::Lost, reason.to_owned());
                continue;
            }
        };
        out.push(Block::Paragraph(image_paragraph(image, &part, description)));
    }
    out
}

/// Asks the model what a region is, and records whatever it says.
///
/// Four outcomes, four report entries: described, refused, not configured, and
/// the answer had no usable kind. The last is the one that would be easiest to
/// miss - a caption that says nothing is not a caption.
fn describe_region(
    image: &strict_ooxml_pdf::PlacedImage,
    options: &PdfOptions,
    report: &mut ConversionReport,
    page: usize,
) -> Option<String> {
    let Some(classifier) = options.figure_classifier.as_ref() else {
        // No model: the picture is still placed, and the fact that it was not
        // described is not recorded on every page - it is a property of the
        // conversion, stated once by the caller.
        return None;
    };
    let Some(encoded) = image.image.as_ref() else {
        report.record(
            "ocr.figure",
            Severity::Lost,
            format!(
                "page {page}: a region had no decodable image, so nothing could be asked about it"
            ),
        );
        return None;
    };
    let Ok(bytes) = encoded.to_png() else {
        report.record(
            "ocr.figure",
            Severity::Lost,
            format!("page {page}: the region could not be re-encoded for a model"),
        );
        return None;
    };
    let request = strict_ooxml_ocr::Image::png(bytes);
    match classifier.describe(&request, &format!("page {page}")) {
        Ok(answer) => {
            let kind = strict_ooxml_ocr::traits::FigureKind::parse(&answer.text);
            if kind == strict_ooxml_ocr::traits::FigureKind::Unknown {
                report.record(
                    "ocr.figure",
                    Severity::Unsupported,
                    format!(
                        "page {page}: the model's answer named no known kind: {}",
                        answer.text
                    ),
                );
                return None;
            }
            // The first line is the kind, which the caller already used; the
            // description that goes into the document is the rest.
            let description = answer
                .text
                .split_once('\n')
                .map_or(answer.text.as_str(), |(_, rest)| rest.trim())
                .to_owned();
            report.record(
                "ocr.figure",
                Severity::Inferred,
                format!(
                    "page {page}: {} {}, described as \\\"{description}\\\" (model {} {})",
                    kind.as_str(),
                    if kind == strict_ooxml_ocr::traits::FigureKind::Scan {
                        "which has no text layer"
                    } else {
                        "recognised by a model"
                    },
                    answer.model,
                    answer.version
                ),
            );
            Some(description)
        }
        Err(error) => {
            report.record(
                "ocr.figure",
                Severity::Lost,
                format!("page {page}: {error}"),
            );
            None
        }
    }
}

/// A paragraph holding one image, sized in EMU.
///
/// A PDF's image is a box; a WordprocessingML one is an inline drawing with an
/// extent, and EMU are the unit the model uses (914 400 per inch).
fn image_paragraph(
    image: &strict_ooxml_pdf::PlacedImage,
    part: &strict_ooxml_core::part::PartId,
    description: Option<String>,
) -> Paragraph {
    let emu = |points: f64| Emu((points * 12_700.0).round() as i64);
    let extent = Extent {
        cx: emu(image.width),
        cy: emu(image.height),
    };
    let location = strict_ooxml_core::error::SourceLocation::unknown();
    Paragraph {
        props: ParagraphProperties {
            alignment: Some(Justification::Center),
            ..ParagraphProperties::default()
        },
        inlines: vec![Inline::Drawing(Drawing {
            kind: DrawingKind::Inline(InlineDrawing {
                extent: Some(extent),
                effect_extent: None,
                doc_pr: Some(strict_ooxml_wml::model::drawing::DocPr {
                    id: None,
                    name: Some(Arc::from("Picture")),
                    // The field a screen reader reads, and the only place a
                    // model's words enter the document.
                    descr: description.clone().map(Arc::from),
                    title: None,
                }),
                dist_top: None,
                dist_bottom: None,
                dist_left: None,
                dist_right: None,
                graphic_uri: None,
                graphic: Box::new(Graphic::Picture(
                    strict_ooxml_wml::model::drawing::Picture {
                        name: Some(Arc::from("Picture")),
                        descr: description.map(Arc::from),
                        blip: Some(strict_ooxml_wml::model::drawing::BlipRef {
                            embed: None,
                            link: None,
                            resolved: Some(part.clone()),
                            location: location.clone(),
                        }),
                        extent: Some(extent),
                        src_rect: None,
                        xfrm: None,
                    },
                )),
                location: location.clone(),
            }),
            location: location.clone(),
        })],
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location,
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use strict_ooxml_wml::model::inline::RunContent;
    use strict_ooxml_wml::model::props::RunProperties;
    use strict_ooxml_wml::model::values::Space;

    /// A run whose text begins or ends with a space must say so, or the XML
    /// parser strips it and the line comes back with its words stuck together.
    #[test]
    fn a_run_preserves_a_space_at_either_end() {
        for text in [" lead", "trail ", "both ", "word"] {
            let inline = run(RunProperties::default(), text.to_owned());
            let strict_ooxml_wml::model::inline::Inline::Run(run) = inline else {
                panic!("expected a run");
            };
            let Some(RunContent::Text(node)) = run.content.first() else {
                panic!("expected text");
            };
            let expected =
                if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
                    Space::Preserve
                } else {
                    Space::default()
                };
            assert_eq!(node.space, expected, "{text:?}");
        }
    }
}
