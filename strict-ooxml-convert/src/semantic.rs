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

    let scan_frame =
        scan_margins(pages, recovered).map(|margins| (scan_size(pages, recovered), margins));
    // The page before was set as a design, in anchored boxes that take no room
    // in the flow: this page starts a page of its own.
    let mut break_next = false;
    // A page rebuilt by a model was rebuilt because its own text layer could
    // not be read; that layer's lines in the flow as well put the page's
    // text in the document twice, the model's copy on a page of its own.
    let mut page_lines: Vec<Vec<GlyphLine>> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            if recovered.page(index).iter().any(|block| block.whole_page) {
                Vec::new()
            } else {
                join_fragments(strict_ooxml_pdf::text::lines(page.items()))
            }
        })
        .collect();
    // The running header and footer are the document's, not a page's: found
    // across the pages before any page is built, they leave every page's flow.
    let heights: Vec<f64> = pages.iter().map(|page| page.geometry.height).collect();
    let running = crate::running::detect(&page_lines, &heights);
    let decorations = crate::running::Decorations::build(&running, pages, &page_lines, report);
    for (index, page) in pages.iter().enumerate() {
        // A size or margin change closes the previous page before these blocks
        // are appended. Identical neighbours stay in the open section.
        let mut section = section_for(page, false);
        let rebuilt = recovered.page(index).iter().any(|block| block.whole_page);
        if let (true, Some((size, margins))) = (rebuilt, scan_frame.as_ref()) {
            section.page_size = Some(*size);
            section.page_margins = Some(*margins);
        }
        decorations.adjust(index, &mut section);
        let text_area = text_area(&section);
        let top_margin = section
            .page_margins
            .as_ref()
            .and_then(|margins| margins.top)
            .map(|top| f64::from(top.0) / 20.0);
        // An indent is measured from the left margin, and the margin is where
        // the page's ink starts: an indent of the line's x from the page's
        // edge put a paper's text a margin further in than it stood, and its
        // lines, narrowed by as much, each broke in two.
        let left_margin = section
            .page_margins
            .as_ref()
            .and_then(|margins| margins.left)
            .map_or(0.0, |left| f64::from(left.0) / 20.0);
        sections.begin_page(&mut blocks, section, false);
        let raw_lines = page_lines
            .get_mut(index)
            .map(std::mem::take)
            .unwrap_or_default();
        report.lines += raw_lines.len();
        let raw_lines: Vec<GlyphLine> = raw_lines
            .into_iter()
            .enumerate()
            .filter(|(number, line)| {
                !running.leaves_flow(index, *number, line, page.geometry.height)
            })
            .map(|(_, line)| line)
            .collect();
        let column_plan = crate::columns::plan(page, &raw_lines, report);
        let lines = margin_lines_in_place(
            crate::columns::apply(raw_lines, &column_plan),
            page.geometry.height,
        );
        let body = body_size(&lines);
        let pitch = body_pitch(&lines);

        // A rebuilt scan's pictures, each with the top of its box (points
        // from the page's top). On a single-column page they join the flow
        // where they stand: before the first line below their top. A
        // two-column page keeps them after its text, because "below" means
        // nothing across a gutter.
        let mut figures: Vec<(f64, Block)> = Vec::new();
        // A page's own pictures stand where the page put them, anchored to it:
        // a logo above the letterhead, two signatures and a stamp in one row
        // are not a column of paragraphs, and stacking them pushed a letter's
        // last lines onto a second page. A scan rebuilt from its layout keeps
        // the flow: its picture is the page, and anchored it would push the
        // text it was read into off the page.
        let mut anchored = Vec::new();
        if options.embed_images && rebuilt {
            figures.extend(images_of(page, &mut media, report, options));
        } else if options.embed_images {
            anchored = anchored_images_of(page, &lines, &mut media, report, options);
        }
        let page_start = blocks.len();
        // A page laid out as a design (side-by-side blocks, boxes, grids)
        // keeps its text where it stood, in text boxes; as one flow its
        // blocks stacked onto page after page.
        if !rebuilt
            && !matches!(column_plan, crate::columns::ColumnPlan::Two { .. })
            && crate::designed::is_designed(&lines, page.geometry.height)
        {
            let frames = crate::designed::text_frames(&lines, pitch / body, push_runs);
            report.record(
                "layout.designed",
                Severity::Inferred,
                format!(
                    "page {}: its text stands in blocks side by side rather than in one \
                     column, and is set as {} text box(es) at their places on the page",
                    index + 1,
                    frames.len()
                ),
            );
            anchored.splice(0..0, page_shapes(page, report, true));
            anchored.extend(frames);
            // Anchors take no room in the flow, so the page is a page of its
            // own: it starts one, and the page after it starts another.
            let mut host = empty_paragraph();
            if page_start > 0 {
                host.props.page_break_before = strict_ooxml_wml::model::values::TriState::On;
            }
            blocks.push(Block::Paragraph(host));
            hold_anchors(&mut blocks, page_start, anchored);
            break_next = true;
            for block in recovered.page(index) {
                blocks.extend(recovered_blocks(
                    block, index, &mut media, report, text_area,
                ));
            }
            continue;
        }
        // A running head, a footer, a page number: set where they stand, in
        // text boxes, out of the flow. In the flow the footer came last with
        // the page's whole foot above it, and the least excess of the text
        // over the source's carried it, and the page, onto the next page.
        let lines = {
            let height = page.geometry.height;
            let in_margin =
                |line: &GlyphLine| line.baseline < height * 0.1 || line.baseline > height * 0.9;
            let inked_margin = lines
                .iter()
                .filter(|line| in_margin(line) && !line.text().trim().is_empty())
                .count();
            if inked_margin == 0 || inked_margin > MAX_MARGIN_LINES {
                lines
            } else {
                let (margin, flow): (Vec<GlyphLine>, Vec<GlyphLine>) =
                    lines.into_iter().partition(|line| in_margin(line));
                anchored.extend(crate::designed::text_frames(
                    &margin,
                    pitch / body,
                    push_runs,
                ));
                flow
            }
        };
        let plan = tables::plan(
            page,
            index + 1,
            &lines,
            &options.tables,
            options.max_table_lines,
            options.max_table_cells,
            report,
        );
        // Drawn shapes are anchored to the page as well, behind its text, and
        // before its pictures (a PDF paints its backgrounds first): placed in
        // the flow, a multi-column page's shapes went after all of its text,
        // onto whatever page that text reached, and a dark box no longer lay
        // under its white words. Thin rules are a table's borders where the
        // page has tables.
        anchored.splice(0..0, page_shapes(page, report, plan.tables.is_empty()));
        let interleave = matches!(column_plan, crate::columns::ColumnPlan::Single);
        if interleave {
            figures.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let mut figures = figures.into_iter().peekable();

        // A table takes the place of the text inside it, and the page's own flow
        // runs around it: the paragraphs before it, the table, the paragraphs
        // after. The table sits where its first line was, which is the only
        // order the page states.
        let mut flow: Vec<GlyphLine> = Vec::new();
        for (number, line) in lines.iter().enumerate() {
            if interleave {
                let line_top = line.baseline - line.ascent;
                while let Some((_, figure)) = figures.next_if(|(top, _)| *top < line_top) {
                    flush_flow(
                        &mut blocks,
                        &mut flow,
                        body,
                        pitch,
                        left_margin,
                        options,
                        &mut numbering,
                        report,
                        index + 1,
                    );
                    blocks.push(figure);
                }
            }
            if let Some(table) = plan.starts_table(number) {
                // The paragraphs above the table are done; keeping them would put
                // the page's text in the document twice.
                flush_flow(
                    &mut blocks,
                    &mut flow,
                    body,
                    pitch,
                    left_margin,
                    options,
                    &mut numbering,
                    report,
                    index + 1,
                );
                blocks.push(table_block(table, body, pitch, options, report, index + 1));
                continue;
            }
            if plan.inside_table(number) {
                continue;
            }
            flow.push(line.clone());
        }
        flush_flow(
            &mut blocks,
            &mut flow,
            body,
            pitch,
            left_margin,
            options,
            &mut numbering,
            report,
            index + 1,
        );
        blocks.extend(figures.map(|(_, figure)| figure));
        // Only where the page starts at the top: the document's first page, or
        // one after a hard break. In a flow a page's first line follows the
        // last one before it.
        if index == 0 || break_next {
            if let (Some(top), Some(first)) = (top_margin, lines.first()) {
                if plan.starts_table(0).is_none() {
                    place_first_line(&mut blocks, page_start, first, top, pitch / body);
                }
            }
        }
        hold_anchors(&mut blocks, page_start, anchored);
        if break_next {
            break_page(&mut blocks, page_start);
        } else if index > 0 {
            mark_page_start(&mut blocks, page_start);
        }
        // A page whose text stops well above its foot ended where its author
        // broke the page (a slide, a chapter's last page): the next page
        // starts a page of its own rather than running on under it.
        break_next = ends_short(&lines, page.geometry.height);
        // A blank page is a page: with nothing in the flow it vanished, and
        // every page after it was one page early.
        if blocks.len() == page_start && recovered.page(index).is_empty() {
            let mut blank = empty_paragraph();
            if page_start > 0 {
                blank.props.page_break_before = strict_ooxml_wml::model::values::TriState::On;
            }
            blocks.push(Block::Paragraph(blank));
            break_next = true;
        }
        // Text a model read off a picture-only region comes after the page's
        // own content, indented to the region's left edge.
        for block in recovered.page(index) {
            // Blocks set where they stood take no room in the flow: the page
            // after them starts a page of its own.
            if block
                .items
                .iter()
                .any(|item| matches!(item, crate::recover::RecoveredItem::Placed { .. }))
            {
                break_next = true;
            }
            let mut placed = recovered_blocks(block, index, &mut media, report, text_area);
            if block.whole_page && page_start > 0 {
                if blocks.len() > page_start {
                    // The page is open already - its pictures and shapes hold
                    // its first paragraph - and the rebuilt text runs on it: a
                    // break before the text put a slide's background on one
                    // page and its words on the next.
                    break_page(&mut blocks, page_start);
                } else {
                    start_page(&mut placed);
                }
            }
            blocks.extend(placed);
        }
    }

    let mut document = crate::empty_document();
    document.body = strict_ooxml_wml::model::document::Body { blocks };
    document.sections = sections.finish();
    decorations.attach(&mut document);
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
    // The lowest baseline the flow has reached. A paragraph's room above is
    // measured from it, not from the line before: a page read column by
    // column climbs back to the top at each column, and gaps measured from
    // the line before added each column's height again - a two-page brochure
    // took eleven.
    let mut furthest: Option<f64> = None;
    let edges = text_edges(lines);
    // The open paragraph's line while it has only one.
    let mut single: Option<&GlyphLine> = None;
    let starts: Vec<f64> = lines
        .iter()
        .filter(|line| !line.text().trim().is_empty())
        .map(|line| line.x)
        .collect();
    for (at, line) in lines.iter().enumerate() {
        // Spaces on the baseline of the line before show nothing, and as a
        // paragraph of their own each took a line's height: a letter's row of
        // signature gaps pushed its names three lines down.
        if line.text().trim().is_empty()
            && previous.is_some_and(|previous| (previous.baseline - line.baseline).abs() < 0.5)
        {
            continue;
        }
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
        // A piece of the line before, further along its baseline (a table of
        // contents' page number, a form's value after its label, a second
        // signatory's name): the same line, reached by a tab. As a paragraph
        // of its own it stood a line lower, and a contents page took two.
        if marker_item.is_none() && !is_item_text {
            if let (Some(before), Some(open)) = (previous, paragraph.as_mut()) {
                if (before.baseline - line.baseline).abs() < 0.5
                    && line.x > before.x + before.width
                    && !line.text().trim().is_empty()
                {
                    tab_to(open, line, origin, edges);
                    push_runs_from(open, line, 0);
                    single = None;
                    previous = Some(line);
                    continue;
                }
            }
        }
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
                        // An empty line is the empty paragraph a producer typed
                        // between two others: it ends the one above and is not
                        // the start of the one below.
                        || is_blank(previous)
                        || is_blank(line)
                        || ended_short(previous, line, edges)
                }));
        if starts_new || paragraph.is_none() {
            if let Some(mut finished) = paragraph.take() {
                give_room(&mut finished, single, edges);
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
            align_paragraph(&mut fresh, line, edges, &starts);
            if let Some(furthest) = furthest {
                set_paragraph_gap(&mut fresh, line, line.baseline - furthest, pitch / body);
            }
            if let Some(item) = marker_item.filter(|item| item.numbered) {
                crate::lists::mark(&mut fresh, item, origin);
                // The number is drawn with the paragraph mark's properties;
                // without them it was the default black, invisible on a dark
                // slide. The marker's own glyph says how it looked.
                if let Some(glyph) = line
                    .glyphs
                    .iter()
                    .find(|glyph| !glyph.text.trim().is_empty())
                {
                    fresh.props.run_props = Some(run_properties(glyph));
                }
            }
            paragraph = Some(fresh);
            single = Some(line);
        } else if let Some(paragraph) = paragraph.as_mut() {
            single = None;
            if let Some(previous) = previous {
                set_line_gap(paragraph, line, line.baseline - previous.baseline);
            }
            // A line that runs on is a word apart from the one before. Word
            // writes the space at the line's end; LaTeX, browsers and design
            // tools do not, and every line break glued two words together.
            if needs_space(paragraph, line) {
                if let Some(glyph) = line.glyphs.first() {
                    paragraph
                        .inlines
                        .push(run(run_properties(glyph), " ".to_owned()));
                }
            }
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
        furthest =
            Some(furthest.map_or(line.baseline, |furthest: f64| furthest.max(line.baseline)));
    }
    if let Some(mut finished) = paragraph {
        give_room(&mut finished, single, edges);
        out.push(finished);
    }
    out
}

/// A line with no ink.
fn is_blank(line: &GlyphLine) -> bool {
    line.text().trim().is_empty()
}

/// Whether `previous` ended its paragraph: the first word of `line` would have
/// fitted in the room left at its end, so a producer that wrapped there was
/// not wrapping.
///
/// The gap and indent rules cannot see this. A letter's signature block, the
/// name, title and department lines of a contact, a list of short entries one
/// line pitch apart all have ordinary gaps and one left edge, and joined they
/// became one run-on paragraph as soon as the pitch was measured right (P1,
/// `PDF-IMPORT-COMPARISON-2026-10-19.md`). One and a half sizes are allowed
/// for the space between the words and the slack of a ragged right edge, so a
/// short ragged block measured against its own longest line still runs on.
fn ended_short(previous: &GlyphLine, line: &GlyphLine, edges: Option<(f64, f64)>) -> bool {
    let Some((_, right)) = edges else {
        return false;
    };
    let Some(end) = previous
        .glyphs
        .iter()
        .rev()
        .find(|glyph| !glyph.text.trim().is_empty())
        .map(|glyph| glyph.x + glyph.width)
    else {
        return false;
    };
    let mut word = line
        .glyphs
        .iter()
        .skip_while(|glyph| glyph.text.trim().is_empty())
        .take_while(|glyph| !glyph.text.trim().is_empty());
    let Some(first) = word.next() else {
        return false;
    };
    let word_end = word.last().unwrap_or(first);
    let word_width = word_end.x + word_end.width - first.x;
    right - end > word_width + line.size * 1.5
}

/// Adds a tab stop at a piece's place on its line (at its end when it ends
/// at the text's right edge, which is where a page number is aligned) and the
/// tab that reaches it.
fn tab_to(paragraph: &mut Paragraph, line: &GlyphLine, origin: f64, edges: Option<(f64, f64)>) {
    use strict_ooxml_wml::model::values::{TabAlignment, TabStop};
    let end = line.x + line.width;
    let at_edge = edges.is_some_and(|(_, right)| (end - right).abs() <= 2.0);
    let (alignment, position) = if at_edge {
        (TabAlignment::End, end)
    } else {
        (TabAlignment::Start, line.x)
    };
    paragraph.props.tabs.push(TabStop {
        position: Twips(to_twips(position - origin)),
        alignment,
        leader: None,
    });
    paragraph.inlines.push(Inline::Run(Run {
        props: RunProperties::default(),
        content: vec![RunContent::Tab],
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }));
}

/// Lets a paragraph of one source line run past the text's edge by as much
/// as a substitute font a little wider than the source's needs (15% of the
/// line), so the line does not break: the source set it as one line, and a
/// right-aligned cover title that broke in two pushed the rest of the cover
/// onto a second page.
fn give_room(paragraph: &mut Paragraph, single: Option<&GlyphLine>, edges: Option<(f64, f64)>) {
    let (Some(line), Some((left, right))) = (single, edges) else {
        return;
    };
    if line.text().trim().is_empty() {
        return;
    }
    let wanted = line.width * 1.15;
    let alignment = paragraph.props.alignment;
    let room = match alignment {
        Some(Justification::End | Justification::Center) => right - left,
        _ => right - line.x,
    };
    let need = wanted - room;
    if !(need.is_finite() && need > 0.5) {
        return;
    }
    let need = Twips(-to_twips(need));
    let half = Twips(need.0 / 2);
    let indentation = paragraph
        .props
        .indentation
        .get_or_insert_with(Indentation::default);
    match alignment {
        Some(Justification::End) => indentation.start = Some(need),
        Some(Justification::Center) => {
            indentation.start = Some(half);
            indentation.end = Some(half);
        }
        _ => indentation.end = Some(need),
    }
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
        .into_iter()
        .map(|(_, block)| block)
        .collect()
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

/// Fragments of one line joined. The reader's lines are runs of glyphs that
/// follow each other exactly; Word justifies by setting each word in a text
/// object of its own at an absolute position, so a justified line arrives as
/// one run per word and every word became a paragraph of its own, indented to
/// where it stood (a bank's one-page letter came out as four pages of
/// staircases). Consecutive runs on one baseline, of one size, with a gap of
/// at most 2.5 times the size between them - a stretched space - are one line;
/// a space is put between them unless one is there. Wider gaps (a tab stop, a
/// table column, a second text column) still end the line.
fn join_fragments(lines: Vec<GlyphLine>) -> Vec<GlyphLine> {
    // Rows: fragments on one baseline and of one size. A producer may draw a
    // line's bold words or another font's after the rest, so a fragment is
    // matched with the last few rows, not only the one before it.
    const LOOKBACK: usize = 4;
    let mut rows: Vec<Vec<GlyphLine>> = Vec::new();
    for line in lines {
        let found = rows.iter().rev().take(LOOKBACK).position(|row| {
            row.first().is_some_and(|first| {
                let size = first.size.max(line.size);
                (first.baseline - line.baseline).abs() < size * 0.1
                    && (first.size - line.size).abs() < size * 0.2
            })
        });
        match found.and_then(|back| rows.len().checked_sub(back + 1)) {
            Some(index) => {
                if let Some(row) = rows.get_mut(index) {
                    row.push(line);
                }
            }
            None => rows.push(vec![line]),
        }
    }
    let mut out: Vec<GlyphLine> = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut current: Option<GlyphLine> = None;
        for line in row {
            let Some(previous) = current.as_mut() else {
                current = Some(line);
                continue;
            };
            // The space a producer draws after a list's marker, in a font
            // of its own, belongs to neither: it is dropped, and the marker
            // stays a marker.
            if is_marker(previous) && line.text().trim().is_empty() {
                continue;
            }
            let end = previous.x + previous.width;
            let gap = line.x - end;
            // A list's marker and its text are two runs on one baseline, and
            // the lists read them as such (`lists`): a marker stays its own.
            let marker = is_marker(previous) && gap >= previous.size * 0.5;
            if !marker && gap > -0.5 && gap <= previous.size.max(line.size) * 2.5 {
                let spaced = previous
                    .glyphs
                    .last()
                    .is_some_and(|glyph| glyph.text.ends_with(' '))
                    || line
                        .glyphs
                        .first()
                        .is_some_and(|glyph| glyph.text.starts_with(' '));
                if !spaced {
                    if let Some(template) = previous.glyphs.last().cloned() {
                        previous.glyphs.push(strict_ooxml_pdf::Glyph {
                            text: " ".to_owned(),
                            x: end,
                            width: gap.max(0.0),
                            ..template
                        });
                    }
                }
                previous.width = (line.x + line.width - previous.x).max(previous.width);
                previous.glyphs.extend(line.glyphs);
            } else if let Some(done) = current.replace(line) {
                out.push(done);
            }
        }
        out.extend(current);
    }
    out
}

/// The page's running header first and its footer last, whatever order they
/// were drawn in: Word draws a page's footer before its body, and the
/// footer's lines opened the converted page. A line in the top tenth of the
/// page goes before the others, one in the bottom tenth after; each group
/// keeps its order.
fn margin_lines_in_place(lines: Vec<GlyphLine>, height: f64) -> Vec<GlyphLine> {
    let (mut top, mut body, mut bottom) = (Vec::new(), Vec::new(), Vec::new());
    for line in lines {
        if line.baseline < height * 0.1 {
            top.push(line);
        } else if line.baseline > height * 0.9 {
            bottom.push(line);
        } else {
            body.push(line);
        }
    }
    top.extend(body);
    top.extend(bottom);
    top
}

/// Whether a run is nothing but a list marker: one bullet character, or a
/// number or letter closed by `.` or `)` (`1.`, `a)`, `iv.`).
fn is_marker(line: &GlyphLine) -> bool {
    let text = line.text();
    let text = text.trim();
    let mut chars = text.chars();
    if let (Some(only), None) = (chars.next(), chars.next()) {
        return matches!(
            only,
            '•' | '‣' | '▪' | '■' | '·' | '–' | '—' | '-' | '*' | '+' | 'o' | '◦' | '►' | '▶'
        );
    }
    let Some(stem) = text.strip_suffix('.').or_else(|| text.strip_suffix(')')) else {
        return false;
    };
    let digits = !stem.is_empty() && stem.len() <= 3 && stem.chars().all(|ch| ch.is_ascii_digit());
    let letter = stem.chars().count() == 1 && stem.chars().all(char::is_alphabetic);
    let roman = !stem.is_empty()
        && stem.len() <= 5
        && stem
            .chars()
            .all(|ch| matches!(ch.to_ascii_lowercase(), 'i' | 'v' | 'x' | 'l' | 'c'));
    digits || letter || roman
}

/// The distance between consecutive baselines of the body text, in points.
///
/// Measured between lines **of the body size** only, as the smallest gap that
/// occurs more than once. It used to be the smallest gap between any two
/// baselines on the page, and a footer's two 9-point lines 5 pt apart made the
/// pitch of a 10.6-point page 5.3 pt: every ordinary line break then looked
/// like a paragraph break, and each line of the page became a paragraph of its
/// own, which reflowed into a line and a stray word as soon as the face was a
/// little wider (P1, `PDF-IMPORT-COMPARISON-2026-10-19.md`). Gaps under 0.8 of
/// the size are not line pitch at all — a superscript, two columns' lines
/// interleaved by baseline — and are left out; a gap that occurs once is a
/// paragraph's spacing as often as a line's.
#[must_use]
fn body_pitch(lines: &[GlyphLine]) -> f64 {
    let body = body_size(lines);
    let fallback = lines.first().map_or(12.0, |_| body * 1.2);
    let mut baselines: Vec<f64> = lines
        .iter()
        .filter(|line| (line.size - body).abs() <= body * 0.05)
        .map(|line| line.baseline)
        .collect();
    baselines.sort_by(f64::total_cmp);
    baselines.dedup_by(|next, previous| (*next - *previous).abs() <= 0.1);
    // Gaps bucketed to a quarter point, so 12.72 and 12.96 of one paragraph's
    // rounding count as the same pitch.
    let mut buckets: std::collections::BTreeMap<i64, (usize, f64)> =
        std::collections::BTreeMap::new();
    for (previous, next) in baselines.iter().zip(baselines.iter().skip(1)) {
        let gap = next - previous;
        if gap < body * 0.8 {
            continue;
        }
        let entry = buckets
            .entry((gap * 4.0).round() as i64)
            .or_insert((0, gap));
        entry.0 += 1;
        entry.1 = entry.1.min(gap);
    }
    // Neighbouring buckets are one pitch split by rounding: count them together.
    let keys: Vec<i64> = buckets.keys().copied().collect();
    keys.iter()
        .find_map(|key| {
            let (count, gap) = buckets.get(key).copied()?;
            let neighbour = buckets.get(&(key + 1)).map_or(0, |(count, _)| *count);
            (count + neighbour >= 2).then_some(gap)
        })
        .unwrap_or(fallback)
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
/// baseline rather than on the first one's: the distance between the two
/// baselines, exactly. It was the line's size less the body's - nothing for
/// running text, and below zero for a footer set smaller than the body, whose
/// lines then printed over each other. Two lines on one baseline (a marker and
/// its text) or a gap a paragraph would not hold set nothing.
fn set_line_gap(paragraph: &mut Paragraph, line: &GlyphLine, pitch: f64) {
    if !(pitch >= line.size * 0.8 && pitch <= line.size * 3.0) {
        return;
    }
    let spacing = paragraph.props.spacing.get_or_insert_with(Spacing::default);
    spacing.line = Some(Twips(to_twips(pitch)));
    spacing.line_rule = Some(strict_ooxml_wml::model::values::LineSpacingRule::Exact);
}

/// A page's drawn shapes as anchored drawings.
fn page_shapes(page: &PdfPage, report: &mut ConversionReport, rules: bool) -> Vec<Inline> {
    crate::vectors::placed_blocks_for(page, report, rules)
        .into_iter()
        .flat_map(|(_, block)| match block {
            Block::Paragraph(paragraph) => paragraph.inlines,
            _ => Vec::new(),
        })
        .collect()
}

/// The left and right edges of the lines' text.
fn text_edges(lines: &[GlyphLine]) -> Option<(f64, f64)> {
    let inked = lines.iter().filter(|line| !line.text().trim().is_empty());
    let left = inked.clone().map(|line| line.x).reduce(f64::min)?;
    let right = inked.map(|line| line.x + line.width).reduce(f64::max)?;
    (right - left > 1.0).then_some((left, right))
}

/// Aligns a paragraph whose first line stands at the right edge or in the
/// middle of the text, rather than indenting it to where it starts: a
/// substitute font a little wider than the source's then ran past the right
/// margin, and a cover's right-aligned title broke word by word and its date
/// letter by letter.
fn align_paragraph(
    paragraph: &mut Paragraph,
    line: &GlyphLine,
    edges: Option<(f64, f64)>,
    starts: &[f64],
) {
    let Some((left, right)) = edges else {
        return;
    };
    if line.text().trim().is_empty() {
        return;
    }
    let span = right - left;
    let indent = line.x - left;
    // Lines that share a start are a column's edge or an indent, not text
    // pushed to the right: a two-column page's right column ends at the right
    // edge too.
    if starts
        .iter()
        .filter(|start| (**start - line.x).abs() <= 1.5)
        .count()
        >= 3
    {
        return;
    }
    let end = line.x + line.width;
    let alignment = if (end - right).abs() <= (span * 0.004).max(1.5) && indent > span * 0.15 {
        Justification::End
    } else if (f64::midpoint(line.x, end) - f64::midpoint(left, right)).abs() <= span * 0.01 + 1.0
        && indent > span * 0.08
    {
        Justification::Center
    } else {
        return;
    };
    paragraph.props.alignment = Some(alignment);
    if let Some(indentation) = paragraph.props.indentation.as_mut() {
        indentation.start = None;
    }
}

/// Sets the room above a paragraph, so its first baseline lands where the PDF
/// put it: `gap` is the distance from the baseline above. The line is given
/// its own height exactly (the page's tightest pitch for its size, `ratio`
/// times it) and the rest of the gap goes above it. Word's space before and
/// after, and the room an inline picture took, were lost without it, and a
/// letter's text ran up under its letterhead and its names over its stamp.
/// A line above this one's baseline (another column, a margin line moved)
/// sets nothing.
fn set_paragraph_gap(paragraph: &mut Paragraph, line: &GlyphLine, gap: f64, ratio: f64) {
    let ratio = if ratio.is_finite() {
        ratio.clamp(1.0, 1.6)
    } else {
        1.2
    };
    let height = line.size * ratio;
    if !(gap.is_finite() && gap >= line.size * 0.8) || line.size <= 0.0 {
        return;
    }
    let exact = gap.min(height);
    let spacing = paragraph.props.spacing.get_or_insert_with(Spacing::default);
    spacing.line = Some(Twips(to_twips(exact)));
    spacing.line_rule = Some(strict_ooxml_wml::model::values::LineSpacingRule::Exact);
    let before = gap - exact;
    if before > 0.5 {
        spacing.before = Some(Twips(to_twips(before)));
    }
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
pub(crate) fn run_properties(glyph: &Glyph) -> RunProperties {
    let [red, green, blue] = glyph.color.to_rgb8();
    let face = FontName::of(&glyph.font);
    let on = |set: bool| {
        if set {
            strict_ooxml_wml::model::values::TriState::On
        } else {
            strict_ooxml_wml::model::values::TriState::Absent
        }
    };
    RunProperties {
        fonts: Some(Fonts {
            ascii: Some(Arc::from(face.family.as_str())),
            h_ansi: Some(Arc::from(face.family.as_str())),
            ..Fonts::default()
        }),
        bold: on(face.bold),
        italic: on(face.italic),
        size: Some(HalfPoints(to_half_points(glyph.size))),
        size_cs: Some(HalfPoints(to_half_points(glyph.size))),
        color: Some(Color::new(format!("{red:02X}{green:02X}{blue:02X}"))),
        ..RunProperties::default()
    }
}

/// A PDF font's name as a family and a style. `BCDFEE+Amalia-Bold` is the
/// family Amalia, bold: the six letters and `+` name a subset, and the style
/// is a suffix. Kept whole, the name was a font nobody has, drawn in a
/// regular substitute - every bold word of a Word PDF lost its weight.
pub(crate) struct FontName {
    pub(crate) family: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

impl FontName {
    pub(crate) fn of(name: &str) -> Self {
        let name = name.trim();
        // A subset tag: six capitals and `+`.
        let name = match name.split_once('+') {
            Some((tag, rest))
                if tag.len() == 6 && tag.chars().all(|ch| ch.is_ascii_uppercase()) =>
            {
                rest
            }
            _ => name,
        };
        let (base, style) = match name.find(['-', ',']) {
            Some(at) => (
                name.get(..at).unwrap_or(name),
                name.get(at + 1..).unwrap_or(""),
            ),
            None => (name, ""),
        };
        let style = style.to_ascii_lowercase();
        let lower = name.to_ascii_lowercase();
        let bold = ["bold", "black", "heavy", "semibold", "demibold"]
            .iter()
            .any(|word| style.contains(word))
            || lower.ends_with("bold");
        let italic = ["italic", "oblique"]
            .iter()
            .any(|word| style.contains(word))
            || lower.ends_with("italic");
        // `ArialMT`, `TimesNewRomanPSMT`, `Arial-BoldMT`: the PostScript
        // suffix is no part of the family.
        let base = base
            .strip_suffix("PSMT")
            .or_else(|| base.strip_suffix("MT"))
            .or_else(|| base.strip_suffix("PS"))
            .unwrap_or(base);
        // `TimesNewRoman` is the family Times New Roman: a space before each
        // capital that follows a small letter.
        let mut family = String::with_capacity(base.len() + 4);
        let mut previous: Option<char> = None;
        for ch in base.chars() {
            if ch.is_uppercase() && previous.is_some_and(char::is_lowercase) {
                family.push(' ');
            }
            family.push(ch);
            previous = Some(ch);
        }
        Self {
            family: if family.is_empty() {
                name.to_owned()
            } else {
                family
            },
            bold,
            italic,
        }
    }
}

/// Whether a line joining a paragraph needs a space before it: the text so
/// far ends in a letter or a mark (not a space, nor a hyphen that breaks a
/// word), and neither side is CJK, which sets no spaces between words.
fn needs_space(paragraph: &Paragraph, line: &GlyphLine) -> bool {
    let last = paragraph
        .inlines
        .iter()
        .rev()
        .find_map(|inline| match inline {
            Inline::Run(run) => run.content.iter().rev().find_map(|content| match content {
                RunContent::Text(node) => node.text.chars().last(),
                RunContent::Tab => Some('\t'),
                _ => None,
            }),
            _ => None,
        });
    let Some(last) = last else {
        return false;
    };
    let Some(first) = line.text().chars().find(|c| !c.is_whitespace()) else {
        return false;
    };
    let cjk = |c: char| matches!(u32::from(c), 0x3000..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0xAC00..=0xD7AF);
    !(last.is_whitespace()
        || matches!(last, '-' | '\u{AD}' | '\u{2010}' | '/')
        || line.text().starts_with(char::is_whitespace)
        || cjk(last)
        || cjk(first))
}

/// One run with its text.
pub(crate) fn run(properties: RunProperties, text: String) -> Inline {
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
/// Closes the open text flow into paragraphs (with its list items) and empties it.
#[allow(clippy::too_many_arguments)]
fn flush_flow(
    blocks: &mut Vec<Block>,
    flow: &mut Vec<GlyphLine>,
    body: f64,
    pitch: f64,
    origin: f64,
    options: &PdfOptions,
    numbering: &mut strict_ooxml_wml::model::numbering::NumberingTable,
    report: &mut ConversionReport,
    page: usize,
) {
    if flow.is_empty() {
        return;
    }
    let list_items = list_items_of(flow, body, options, numbering, report, page);
    push_paragraphs(
        blocks,
        flow,
        body,
        pitch,
        options,
        origin,
        true,
        report,
        page,
        &list_items,
    );
    flow.clear();
}

/// The page's pictures as image paragraphs, each with the top of its box in
/// points from the page's top.
///
/// `PlacedImage` keeps PDF's y-up lower edge (unlike glyphs, which the
/// interpreter reflects), so the top is `page height - (y + height)`.
fn images_of(
    page: &PdfPage,
    media: &mut MediaCollector,
    report: &mut ConversionReport,
    options: &PdfOptions,
) -> Vec<(f64, Block)> {
    stored_images(page, media, report, options)
        .into_iter()
        .map(|(image, part, description)| {
            let top = page.geometry.height - (image.y + image.height);
            (
                top,
                Block::Paragraph(image_paragraph(image, &part, description)),
            )
        })
        .collect()
}

/// The page's pictures as page-anchored drawings, each wrapped by what the
/// page sets around it: text over a picture puts it behind the text, text
/// beside it wraps square, and otherwise it stands in front and wraps
/// nothing. The flow keeps the room a picture stands in (each paragraph's
/// space before is the PDF's), so a picture that pushed text as well moved a
/// letter's names a line below their signatures and its footer onto a second
/// page.
fn anchored_images_of(
    page: &PdfPage,
    lines: &[GlyphLine],
    media: &mut MediaCollector,
    report: &mut ConversionReport,
    options: &PdfOptions,
) -> Vec<Inline> {
    stored_images(page, media, report, options)
        .into_iter()
        .filter_map(|(image, part, description)| {
            let top = page.geometry.height - (image.y + image.height);
            let (left, right, bottom) = (image.x, image.x + image.width, top + image.height);
            let mut wrap = PictureWrap::Over;
            for line in lines {
                let (line_top, line_bottom) = (line.baseline - line.ascent, line.baseline);
                let shared = line_bottom.min(bottom) - line_top.max(top);
                if shared <= line.size * 0.25 {
                    continue;
                }
                if line.text().trim().is_empty() {
                    continue;
                }
                if line.x < right && line.x + line.width > left {
                    wrap = PictureWrap::Behind;
                    break;
                }
                wrap = PictureWrap::Square;
            }
            let mut inline = anchored_image(image, &part, description, top, wrap)?;
            // Stacked by where the page painted it among its shapes too, not
            // only among its pictures: a card painted over a photograph was
            // drawn under it.
            let painted = page.items().iter().position(|item| {
                matches!(item, strict_ooxml_pdf::Item::Image(placed) if std::ptr::eq(placed, image))
            });
            if let (
                Some(index),
                Inline::Drawing(Drawing {
                    kind: DrawingKind::Anchor(anchor),
                    ..
                }),
            ) = (painted, &mut inline)
            {
                anchor.relative_height = Some(paint_height(index));
            }
            Some(inline)
        })
        .collect()
}

/// The `relativeHeight` of the page's `index`th painted item: one scale for
/// its pictures and its shapes, in the order the page painted them. It starts
/// at 2, because 1 is what a drawing without a place in that order carries.
pub(crate) fn paint_height(index: usize) -> u32 {
    u32::try_from(index)
        .unwrap_or(u32::MAX - 2)
        .saturating_add(2)
}

/// How a page-anchored picture meets the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PictureWrap {
    /// Text set over it: the picture is behind the text and wraps nothing.
    Behind,
    /// No text across it: in front, and it wraps nothing.
    Over,
    /// Text beside it.
    Square,
}

/// Puts a page's anchored drawings, in paint order, into its first paragraph, which is on the
/// page they belong to whatever the text before it does; a page without one
/// gets a paragraph of their own.
fn hold_anchors(blocks: &mut Vec<Block>, page_start: usize, mut anchors: Vec<Inline>) {
    if anchors.is_empty() {
        return;
    }
    // Stacked in the order the page painted them: with one height for all, a
    // page's background box could be drawn over the picture set on it. A
    // drawing that already has its paint height keeps it; the rest follow.
    let mut height = anchors
        .iter()
        .filter_map(|inline| match inline {
            Inline::Drawing(Drawing {
                kind: DrawingKind::Anchor(anchor),
                ..
            }) if anchor.relative_height.is_some_and(|height| height > 1) => anchor.relative_height,
            _ => None,
        })
        .max()
        .unwrap_or(0);
    for inline in &mut anchors {
        if let Inline::Drawing(Drawing {
            kind: DrawingKind::Anchor(anchor),
            ..
        }) = inline
        {
            if anchor.relative_height.is_none_or(|height| height <= 1) {
                height = height.saturating_add(1);
                anchor.relative_height = Some(height);
            }
        }
    }
    // And written in that order: WPS stacks drawings behind the text by their
    // order in the document rather than by `relativeHeight`, and drew a
    // page's pictures over the cards painted on them.
    anchors.sort_by_key(|inline| match inline {
        Inline::Drawing(Drawing {
            kind: DrawingKind::Anchor(anchor),
            ..
        }) => anchor.relative_height.unwrap_or(0),
        _ => 0,
    });
    let first = blocks
        .iter_mut()
        .skip(page_start)
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        });
    if let Some(paragraph) = first {
        paragraph.inlines.splice(0..0, anchors);
        return;
    }
    let mut paragraph = empty_paragraph();
    paragraph.inlines = anchors;
    blocks.push(Block::Paragraph(paragraph));
}

/// Sets the room above a page's first paragraph, so its first baseline lands
/// where the PDF put it rather than on the top margin. The margin is the
/// page's least, and a cover's title set a third of the way down stood at
/// the top of the page.
fn place_first_line(
    blocks: &mut [Block],
    page_start: usize,
    line: &GlyphLine,
    top: f64,
    ratio: f64,
) {
    let Some(Block::Paragraph(paragraph)) = blocks.get_mut(page_start) else {
        return;
    };
    let ratio = if ratio.is_finite() {
        ratio.clamp(1.0, 1.6)
    } else {
        1.2
    };
    let spacing = paragraph.props.spacing.get_or_insert_with(Spacing::default);
    if spacing.before.is_some() {
        return;
    }
    let height =
        if let (Some(line), Some(strict_ooxml_wml::model::values::LineSpacingRule::Exact)) =
            (spacing.line, spacing.line_rule)
        {
            f64::from(line.0) / 20.0
        } else {
            let height = line.size * ratio;
            spacing.line = Some(Twips(to_twips(height)));
            spacing.line_rule = Some(strict_ooxml_wml::model::values::LineSpacingRule::Exact);
            height
        };
    // An exact line's baseline is 0.8 of the way down its box.
    let before = line.baseline - top - 0.8 * height;
    if before > 0.5 {
        spacing.before = Some(Twips(to_twips(before)));
    }
}

/// The most lines a page's top and bottom tenths may hold and still be its
/// running head and footer; more is body text set to the paper's edge.
const MAX_MARGIN_LINES: usize = 8;

/// The share of a page below its last line of body text from which the page
/// ended short.
const SHORT_PAGE_SHARE: f64 = 0.3;

/// Whether the page's body text (its foot's margin lines aside) stops more
/// than [`SHORT_PAGE_SHARE`] of the page above its bottom.
fn ends_short(lines: &[GlyphLine], page_height: f64) -> bool {
    if !(page_height.is_finite() && page_height > 0.0) {
        return false;
    }
    let foot = page_height * 0.9;
    let last = lines
        .iter()
        .filter(|line| !line.text().trim().is_empty() && line.baseline < foot)
        .map(|line| line.baseline)
        .reduce(f64::max);
    last.is_some_and(|last| last < page_height * (1.0 - SHORT_PAGE_SHARE))
}

/// An empty paragraph.
fn empty_paragraph() -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: Vec::new(),
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }
}

/// Starts the page's first paragraph on a new page.
fn break_page(blocks: &mut [Block], page_start: usize) {
    if let Some(paragraph) = blocks
        .iter_mut()
        .skip(page_start)
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
    {
        paragraph.props.page_break_before = strict_ooxml_wml::model::values::TriState::On;
    }
}

/// Marks where a PDF page begins in the flow with `w:lastRenderedPageBreak`,
/// Word's own note that a page broke there when the document was last laid
/// out - which for a PDF it did. The flow runs on without a hard break, and
/// the note is how a reader (and the scan-beside-result comparison) finds the
/// source's pages again.
fn mark_page_start(blocks: &mut [Block], page_start: usize) {
    let Some(paragraph) = blocks
        .iter_mut()
        .skip(page_start)
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
    else {
        return;
    };
    if paragraph.props.page_break_before == strict_ooxml_wml::model::values::TriState::On {
        return;
    }
    paragraph.inlines.insert(
        0,
        Inline::Run(Run {
            props: RunProperties::default(),
            content: vec![RunContent::LastRenderedPageBreak],
            revision: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        }),
    );
}

/// The page's pictures stored as media parts, each with what a model said of
/// it.
fn stored_images<'p>(
    page: &'p PdfPage,
    media: &mut MediaCollector,
    report: &mut ConversionReport,
    options: &PdfOptions,
) -> Vec<(
    &'p strict_ooxml_pdf::PlacedImage,
    strict_ooxml_core::part::PartId,
    Option<String>,
)> {
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
        out.push((image, part, description));
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
    let mut paragraph = sized_image_paragraph(image.width, image.height, part, description);
    if let Some(Inline::Drawing(Drawing {
        kind: DrawingKind::Inline(inline),
        ..
    })) = paragraph.inlines.first_mut()
    {
        crop_picture(inline.graphic.as_mut(), image.crop);
    }
    paragraph
}

/// The part of a picture a rectangular clip lets show, as `a:srcRect` (G1):
/// the reader has already shrunk the box to that part, and the crop says
/// which part of the picture fills it.
fn crop_picture(graphic: &mut Graphic, crop: Option<[f64; 4]>) {
    let (Graphic::Picture(picture), Some([left, top, right, bottom])) = (graphic, crop) else {
        return;
    };
    // Thousandths of a percent.
    let unit = |fraction: f64| (fraction.clamp(0.0, 1.0) * 100_000.0).round() as i32;
    picture.src_rect = Some(strict_ooxml_wml::model::drawing::SrcRect {
        left: unit(left),
        top: unit(top),
        right: unit(right),
        bottom: unit(bottom),
    });
}

/// The margins of a scanned book's pages rebuilt from their layout: the
/// medians of where their ink lies, so every page has the frame the book has
/// (one section, no breaks between pages that differ by a few points).
fn scan_margins(
    pages: &[PdfPage],
    recovered: &crate::recover::Recovered,
) -> Option<strict_ooxml_wml::model::props::PageMargins> {
    let mut sides: [Vec<f64>; 4] = Default::default();
    for (index, page) in pages.iter().enumerate() {
        let (width, height) = page.geometry.displayed();
        for block in recovered.page(index) {
            let Some([left, top, right, bottom]) = block.extent.filter(|_| block.whole_page) else {
                continue;
            };
            for (side, value) in sides
                .iter_mut()
                .zip([left, top, width - right, height - bottom])
            {
                side.push(value);
            }
        }
    }
    let median = |values: &mut Vec<f64>| -> Option<f64> {
        values.sort_by(f64::total_cmp);
        values.get(values.len() / 2).copied()
    };
    let [left, top, right, bottom] = sides.each_mut().map(median);
    let twips = |points: Option<f64>| -> Option<Twips> {
        points.map(|points| Twips(to_twips(points.clamp(18.0, 108.0).floor())))
    };
    left?;
    Some(strict_ooxml_wml::model::props::PageMargins {
        top: twips(top),
        right: twips(right),
        bottom: twips(bottom),
        left: twips(left),
        header: None,
        footer: None,
        gutter: None,
    })
}

/// The page size of a scanned book's rebuilt pages: the medians of their
/// shown sizes. Scans of one book differ by a point or two, and each
/// difference would be a section break.
fn scan_size(
    pages: &[PdfPage],
    recovered: &crate::recover::Recovered,
) -> strict_ooxml_wml::model::props::PageSize {
    let (mut widths, mut heights) = (Vec::new(), Vec::new());
    for (index, page) in pages.iter().enumerate() {
        if recovered.page(index).iter().any(|block| block.whole_page) {
            let (width, height) = page.geometry.displayed();
            widths.push(width);
            heights.push(height);
        }
    }
    let median = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values.get(values.len() / 2).copied().unwrap_or(0.0)
    };
    strict_ooxml_wml::model::props::PageSize {
        width: Some(Twips(to_twips(median(widths).round()))),
        height: Some(Twips(to_twips(median(heights).round()))),
        orientation: None,
        code: None,
    }
}

/// The width and height between a section's margins, in points.
fn text_area(section: &strict_ooxml_wml::model::props::SectionProperties) -> Option<(f64, f64)> {
    let size = section.page_size.as_ref()?;
    let margins = section.page_margins.as_ref()?;
    let points = |twips: Option<Twips>| f64::from(twips.map_or(0, |twips| twips.0)) / 20.0;
    let width = points(size.width) - points(margins.left) - points(margins.right);
    let height = points(size.height) - points(margins.top) - points(margins.bottom);
    (width > 0.0 && height > 0.0).then_some((width, height))
}

/// Makes `blocks` begin a new page: the first paragraph breaks before
/// itself, or an empty paragraph that does is put in front of a table.
fn start_page(blocks: &mut Vec<Block>) {
    if let Some(Block::Paragraph(first)) = blocks.first_mut() {
        first.props.page_break_before = strict_ooxml_wml::model::values::TriState::On;
        return;
    }
    blocks.insert(
        0,
        Block::Paragraph(Paragraph {
            props: ParagraphProperties {
                page_break_before: strict_ooxml_wml::model::values::TriState::On,
                ..ParagraphProperties::default()
            },
            inlines: Vec::new(),
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            revision: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        }),
    );
}

/// The blocks a recovered block places: its paragraphs, and its pictures
/// stored as media (a picture that cannot be stored is reported lost).
pub(crate) fn recovered_blocks(
    block: &crate::recover::RecoveredBlock,
    index: usize,
    media: &mut crate::media::MediaCollector,
    report: &mut ConversionReport,
    fit: Option<(f64, f64)>,
) -> Vec<Block> {
    use crate::recover::RecoveredItem;
    let mut out = Vec::with_capacity(block.items.len());
    // Blocks set where they stood: their pictures under their text, all held
    // by one paragraph at the page's top, taking no room in the flow.
    let (mut pictures, mut boxes) = (Vec::new(), Vec::new());
    for item in &block.items {
        match item {
            RecoveredItem::Placed {
                area,
                items,
                turned,
            } => {
                if let [RecoveredItem::Picture(picture)] = items.as_slice() {
                    pictures.extend(picture_at(picture, *area, index, media, report));
                } else {
                    let content: Vec<Block> = items
                        .iter()
                        .flat_map(|item| {
                            item_blocks(item, index, media, report, Some((area[2], area[3])))
                        })
                        .collect();
                    if !content.is_empty() {
                        let name = Arc::from(format!("ocr-block-{}", boxes.len()));
                        boxes.push(crate::designed::text_box_at(
                            name, *area, content, "square", *turned,
                        ));
                    }
                }
            }
            _ => out.extend(item_blocks(item, index, media, report, fit)),
        }
    }
    if !pictures.is_empty() || !boxes.is_empty() {
        let mut anchors = pictures;
        anchors.extend(boxes);
        let mut held = vec![Block::Paragraph(empty_paragraph())];
        hold_anchors(&mut held, 0, anchors);
        out.splice(0..0, held);
    }
    out
}

/// A picture cut from a scan, anchored at its place on the page.
fn picture_at(
    picture: &crate::recover::RecoveredPicture,
    area: [f64; 4],
    index: usize,
    media: &mut crate::media::MediaCollector,
    report: &mut ConversionReport,
) -> Option<Inline> {
    let part = match media.add(index, &picture.image) {
        Ok(part) => part,
        Err(reason) => {
            report.record(
                "image.lost",
                Severity::Lost,
                format!("page {}: a picture cut from the scan: {reason}", index + 1),
            );
            return None;
        }
    };
    let Paragraph { mut inlines, .. } =
        sized_image_paragraph(area[2], area[3], &part, picture.description.clone());
    let Some(Inline::Drawing(Drawing {
        kind: DrawingKind::Inline(inline),
        location,
    })) = inlines.pop()
    else {
        return None;
    };
    Some(anchor_at(
        inline,
        location,
        area[0],
        area[1],
        PictureWrap::Over,
    ))
}

/// The blocks one recovered item places; `fit` is the room it has, in points.
fn item_blocks(
    item: &crate::recover::RecoveredItem,
    index: usize,
    media: &mut crate::media::MediaCollector,
    report: &mut ConversionReport,
    fit: Option<(f64, f64)>,
) -> Vec<Block> {
    use crate::recover::RecoveredItem;
    match item {
        RecoveredItem::Paragraph(paragraph) => vec![Block::Paragraph(paragraph.as_ref().clone())],
        RecoveredItem::Table(table) => vec![Block::Table(table.as_ref().clone())],
        RecoveredItem::Columns { widths, cells } => {
            let height = fit.map(|(_, height)| height);
            let contents = widths
                .iter()
                .zip(cells)
                .map(|(width, items)| {
                    // A cell's own margins take a few points of its width.
                    let room = height.map(|height| ((width - 11.0).max(18.0), height));
                    items
                        .iter()
                        .flat_map(|item| item_blocks(item, index, media, report, room))
                        .collect()
                })
                .collect();
            vec![Block::Table(tables::columns_table(widths, contents))]
        }
        // A placed block met inside another is set in its flow.
        RecoveredItem::Placed { items, .. } => items
            .iter()
            .flat_map(|item| item_blocks(item, index, media, report, fit))
            .collect(),
        RecoveredItem::Picture(picture) => match media.add(index, &picture.image) {
            Ok(part) => {
                // A picture cut from a scan is as large as the page it
                // was on, and the page has margins: it is scaled to fit.
                let shrink = fit.map_or(1.0, |(width, height)| {
                    (width / picture.width)
                        .min(height * 0.98 / picture.height)
                        .min(1.0)
                });
                vec![Block::Paragraph(sized_image_paragraph(
                    picture.width * shrink,
                    picture.height * shrink,
                    &part,
                    picture.description.clone(),
                ))]
            }
            Err(reason) => {
                report.record(
                    "image.lost",
                    Severity::Lost,
                    format!("page {}: a picture cut from the scan: {reason}", index + 1),
                );
                Vec::new()
            }
        },
    }
}

/// A paragraph holding one picture of `width` x `height` points.
fn sized_image_paragraph(
    width: f64,
    height: f64,
    part: &strict_ooxml_core::part::PartId,
    description: Option<String>,
) -> Paragraph {
    let emu = |points: f64| Emu((points * 12_700.0).round() as i64);
    let extent = Extent {
        cx: emu(width),
        cy: emu(height),
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
                    hidden: None,
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
                        nv_id: None,
                        bw_mode: None,
                        blip: Some(strict_ooxml_wml::model::drawing::BlipRef {
                            embed: None,
                            link: None,
                            resolved: Some(part.clone()),
                            cstate: None,
                            location: location.clone(),
                        }),
                        extent: Some(extent),
                        src_rect: None,
                        xfrm: None,
                        markup: strict_ooxml_wml::model::drawing::PictureMarkup::default(),
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

/// How a picture's matrix turns and mirrors it, when it does.
#[derive(Clone, Copy, Debug)]
struct Turn {
    /// The picture's own width and height, in points.
    width: f64,
    height: f64,
    /// Clockwise, as DrawingML turns, in degrees `0..360`.
    degrees: f64,
    /// Mirrored top to bottom before the turn.
    mirrored: bool,
}

impl Turn {
    fn of(ctm: strict_ooxml_pdf::content::Matrix) -> Option<Self> {
        let width = ctm.a.hypot(ctm.b);
        let height = ctm.c.hypot(ctm.d);
        let det = ctm.a * ctm.d - ctm.b * ctm.c;
        if !(width > 0.0 && height > 0.0 && det.is_finite() && det.abs() > f64::EPSILON) {
            return None;
        }
        // PDF turns anticlockwise with y up; DrawingML clockwise with y down,
        // which on the page is the same turn the other way round.
        let degrees = (-ctm.b.atan2(ctm.a).to_degrees()).rem_euclid(360.0);
        let degrees = if (360.0 - degrees) < 0.01 {
            0.0
        } else {
            degrees
        };
        let mirrored = det < 0.0;
        (degrees > 0.01 || mirrored).then_some(Self {
            width,
            height,
            degrees,
            mirrored,
        })
    }
}

/// A picture anchored to the page at its place on it.
fn anchored_image(
    image: &strict_ooxml_pdf::PlacedImage,
    part: &strict_ooxml_core::part::PartId,
    description: Option<String>,
    top: f64,
    wrap: PictureWrap,
) -> Option<Inline> {
    let turn = Turn::of(image.ctm);
    // A turned or mirrored picture is its own size, turned about the centre
    // of the box it covers: the box alone set a turned photograph upright
    // and stretched, and a mirrored one the right way round.
    let (width, height, left, top) = match turn {
        Some(turn) => (
            turn.width,
            turn.height,
            image.x + image.width / 2.0 - turn.width / 2.0,
            top + image.height / 2.0 - turn.height / 2.0,
        ),
        None => (image.width, image.height, image.x, top),
    };
    let Paragraph { mut inlines, .. } = sized_image_paragraph(width, height, part, description);
    let Some(Inline::Drawing(Drawing {
        kind: DrawingKind::Inline(mut inline),
        location,
    })) = inlines.pop()
    else {
        return None;
    };
    crop_picture(inline.graphic.as_mut(), image.crop);
    if let (Some(turn), Graphic::Picture(picture)) = (turn, inline.graphic.as_mut()) {
        picture.xfrm = Some(strict_ooxml_wml::model::drawing::Xfrm {
            offset: None,
            rot: (turn.degrees.abs() > 0.01).then(|| (turn.degrees * 60_000.0).round() as i32),
            flip_h: false,
            flip_v: turn.mirrored,
        });
    }
    Some(anchor_at(inline, location, left, top, wrap))
}

/// An inline drawing anchored to the page at `left`, `top` (points from the
/// page's corner), wrapped as `wrap` says.
fn anchor_at(
    inline: InlineDrawing,
    location: strict_ooxml_core::error::SourceLocation,
    left: f64,
    top: f64,
    wrap: PictureWrap,
) -> Inline {
    use strict_ooxml_wml::model::drawing::{AnchorDrawing, Position, Wrap, WrapKind};
    let emu = |points: f64| Emu((points * 12_700.0).round() as i64);
    let position = |offset: f64| Position {
        relative_from: Some(Arc::from("page")),
        align: None,
        offset: Some(emu(offset)),
        percent_offset: None,
    };
    let kind = match wrap {
        PictureWrap::Behind | PictureWrap::Over => WrapKind::None,
        PictureWrap::Square => WrapKind::Square,
    };
    Inline::Drawing(Drawing {
        kind: DrawingKind::Anchor(AnchorDrawing {
            extent: inline.extent,
            effect_extent: None,
            doc_pr: inline.doc_pr,
            simple_pos: false,
            simple_pos_point: None,
            hidden: None,
            position_h: Some(position(left)),
            position_v: Some(position(top)),
            wrap: Some(Wrap {
                kind,
                wrap_text: if kind == WrapKind::Square {
                    Some(Arc::from("bothSides"))
                } else {
                    None
                },
                dist_left: None,
                dist_right: None,
                dist_top: None,
                dist_bottom: None,
                polygon: Vec::new(),
                polygon_edited: None,
            }),
            behind_doc: wrap == PictureWrap::Behind,
            relative_height: Some(1),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            allow_overlap: true,
            layout_in_cell: true,
            locked: false,
            graphic_uri: inline.graphic_uri,
            graphic: inline.graphic,
            size_rel_h: None,
            size_rel_v: None,
            location: location.clone(),
        }),
        location,
    })
}

#[cfg(test)]
mod fragment_tests {
    use super::join_fragments;
    use strict_ooxml_pdf::text::GlyphLine;
    use strict_ooxml_pdf::{Glyph, RenderMode, Rgb};

    fn run(text: &str, x: f64, width: f64) -> GlyphLine {
        let glyph = Glyph {
            text: text.to_owned(),
            x,
            y: 100.0,
            width,
            ascent: 7.0,
            descent: 2.0,
            size: 9.0,
            font: "Amalia".to_owned(),
            color: Rgb(0.0, 0.0, 0.0),
            render_mode: RenderMode::Fill,
            mapped: true,
        };
        GlyphLine {
            x,
            baseline: 100.0,
            size: 9.0,
            ascent: 7.0,
            width,
            glyphs: vec![glyph],
        }
    }

    fn line_at(baseline: f64, size: f64) -> GlyphLine {
        let mut line = run("text", 36.0, 300.0);
        line.baseline = baseline;
        line.size = size;
        line
    }

    /// P1: a footer's two small lines 5 pt apart set the pitch of the whole
    /// page, and every line of its body became a paragraph.
    #[test]
    fn the_pitch_is_the_body_texts_not_a_footers() {
        let mut lines: Vec<GlyphLine> = [215.8, 228.7, 241.4, 267.1, 279.8, 292.6, 305.5]
            .into_iter()
            .map(|baseline| line_at(baseline, 10.86))
            .collect();
        lines.push(line_at(771.4, 9.12));
        lines.push(line_at(776.6, 9.12));
        let pitch = super::body_pitch(&lines);
        assert!((12.5..13.1).contains(&pitch), "pitch {pitch}");
    }

    /// A gap that occurs once is not a pitch, and with none repeated the
    /// estimate is the size's ordinary leading.
    #[test]
    fn a_single_gap_is_not_a_pitch() {
        let lines = [line_at(100.0, 10.0), line_at(130.0, 10.0)];
        assert!((super::body_pitch(&lines) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_paragraph_keeps_the_room_above_it() {
        use strict_ooxml_wml::model::values::{LineSpacingRule, Twips};
        let line = run("Names", 72.0, 30.0);
        let spacing = |gap: f64| {
            let mut paragraph = super::new_paragraph(&line, None, 0.0);
            super::set_paragraph_gap(&mut paragraph, &line, gap, 1.2);
            paragraph.props.spacing
        };
        // 9 pt at 1.2 is a 10.8 pt line; the other 39.2 pt of a 50 pt gap
        // (the room a row of signatures took) go above it.
        let wide = spacing(50.0).expect("spacing");
        assert_eq!(wide.line, Some(Twips(216)));
        assert_eq!(wide.line_rule, Some(LineSpacingRule::Exact));
        assert_eq!(wide.before, Some(Twips(784)));
        // A tight gap is the line's own height, and nothing goes above.
        let tight = spacing(10.0).expect("spacing");
        assert_eq!((tight.line, tight.before), (Some(Twips(200)), None));
        // A line above this one (another column) sets nothing.
        assert!(spacing(-300.0).is_none());
    }

    #[test]
    fn a_pdf_font_name_is_a_family_and_a_style() {
        let name = super::FontName::of("BCDFEE+Amalia-Bold");
        assert_eq!(
            (name.family.as_str(), name.bold, name.italic),
            ("Amalia", true, false)
        );
        let name = super::FontName::of("TimesNewRomanPS-BoldItalicMT");
        assert_eq!(
            (name.family.as_str(), name.bold, name.italic),
            ("Times New Roman", true, true)
        );
        let name = super::FontName::of("ArialMT");
        assert_eq!((name.family.as_str(), name.bold), ("Arial", false));
        let name = super::FontName::of("Calibri,Italic");
        assert_eq!((name.family.as_str(), name.italic), ("Calibri", true));
    }

    #[test]
    fn a_marker_drops_the_space_drawn_after_it() {
        let lines = join_fragments(vec![
            run("\u{2022}", 96.3, 4.1),
            run(" ", 100.3, 2.5),
            run("Naknada za poslove", 114.3, 80.0),
        ]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "\u{2022}");
    }

    #[test]
    fn a_footer_drawn_first_ends_the_page() {
        let mut footer = run("Raiffeisen banka a.d. Beograd", 78.0, 115.0);
        footer.baseline = 793.0;
        let body = run("Predmet:", 78.0, 40.0);
        let lines = super::margin_lines_in_place(vec![footer, body], 842.0);
        assert_eq!(lines[0].text(), "Predmet:");
    }

    #[test]
    fn a_list_marker_stays_apart_from_its_text() {
        let lines = join_fragments(vec![
            run("\u{2022}", 96.7, 3.2),
            run("Naknada za poslove", 115.0, 80.0),
        ]);
        assert_eq!(lines.len(), 2);
        let lines = join_fragments(vec![run("1.", 90.0, 6.0), run("Mi", 104.0, 9.0)]);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn a_bold_word_drawn_after_its_line_joins_it() {
        let mut bold = run("15.12.2026.", 300.0, 45.0);
        bold.glyphs[0].font = "Amalia-Bold".to_owned();
        let lines = join_fragments(vec![
            run("stupaju na snagu ", 230.0, 68.0),
            run("godine.", 348.0, 30.0),
            bold,
        ]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "stupaju na snagu 15.12.2026. godine.");
    }

    #[test]
    fn a_justified_line_of_word_runs_is_one_line() {
        // (opcija OUR) - ostale valute, each word placed on its own.
        let lines = join_fragments(vec![
            run("(opcija OUR) ", 235.0, 48.0),
            run("\u{2013}", 285.0, 4.5),
            run("ostale", 292.0, 25.0),
            // A tab stop's distance: a separate line.
            run("5.900,00 RSD", 424.6, 50.0),
        ]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "(opcija OUR) \u{2013} ostale");
        assert_eq!(lines[1].text(), "5.900,00 RSD");
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
