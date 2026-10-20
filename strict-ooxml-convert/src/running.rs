//! Running headers and footers, found across the document rather than on a page.
//!
//! A page alone cannot tell its running head from its first line: both are
//! text at the top. The document can. A line that stands in the same band at
//! the top or the foot of page after page, at the same height, with the same
//! text up to its digits, is the page's header or footer, and its digits, when
//! they count with the page, are the page number. Left in the body, such a line
//! was a text box or a paragraph on each page, and as soon as the body reflowed
//! a page's head and number landed in the middle of the page before (H1,
//! `PDF-IMPORT-COMPARISON-2026-10-19.md`). Found here, it leaves the flow and
//! becomes one header or footer part that every page shows, with a `PAGE` field
//! where the number was.

use std::collections::BTreeSet;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::rels::RelId;
use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::text::GlyphLine;
use strict_ooxml_pdf::{Glyph, PdfPage};
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::inline::{Field, Inline, Run, RunContent};
use strict_ooxml_wml::model::props::{
    HeaderFooterKind, HeaderFooterRef, PageMargins, PageNumberType, ParagraphProperties,
    RunProperties, SectionProperties,
};
use strict_ooxml_wml::model::values::{
    Indentation, Justification, LineSpacingRule, Rsids, Spacing, TabAlignment, TabStop, Twips,
};
use strict_ooxml_wml::model::{Document, HeaderFooter};

use crate::report::{ConversionReport, Severity};
use crate::to_twips;

/// The share of the page's height at its top and at its foot where a running
/// header or footer stands.
const BAND: f64 = 0.12;
/// How far into the page a running block may reach from its band: a
/// letterhead's fourth line stands below the top eighth, and is running only
/// when it joins a running line in the band, a line or two away.
const OUTER_BAND: f64 = 0.2;
/// How far, in points, one page's copy of a line may stand from another's.
const SAME_Y: f64 = 2.0;
/// Where the renderer puts an exact line's baseline, as a share of the line
/// (the same rule the designed pages' text boxes use).
const EXACT_LINE_ASCENT: f64 = 0.8;
/// The height of a part's first line, as a share of its size.
const FIRST_LINE: f64 = 1.2;
/// Longer digit runs are not counted: no page number has ten digits.
const MAX_COUNTED_DIGITS: usize = 9;

/// Which edge of the page a running line stands at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Band {
    /// The top: a header.
    Top,
    /// The foot: a footer.
    Bottom,
}

/// What one digit run of a running line does from page to page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Digits {
    /// The same on every page: a date, a version, `TAR(5)`.
    Fixed,
    /// The page number: page `n` shows `n + offset`.
    Counts,
}

/// One running line: its copies on the pages it repeats on.
#[derive(Clone, Debug)]
struct Cluster {
    band: Band,
    /// The text with every digit run replaced by `#`.
    key: String,
    /// The first copy's baseline.
    y: f64,
    /// The first copy's size.
    size: f64,
    /// Whether the first copy stands in [`BAND`] rather than beyond it.
    inner: bool,
    /// `(page, line)` of each copy, by page; at most one per page.
    copies: Vec<(usize, usize)>,
    /// What each of the text's digit runs does.
    digits: Vec<Digits>,
    /// `value - page number` of the counted runs.
    offset: Option<i64>,
}

impl Cluster {
    fn pages(&self) -> impl Iterator<Item = usize> + '_ {
        self.copies.iter().map(|(page, _)| *page)
    }
}

/// The running lines of a document.
#[derive(Clone, Debug, Default)]
pub(crate) struct Running {
    clusters: Vec<Cluster>,
    /// Pages in the document.
    pages: usize,
}

/// The text with its digit runs replaced by `#`, and the runs themselves.
fn split_digits(text: &str) -> (String, Vec<String>) {
    let mut key = String::with_capacity(text.len());
    let mut runs: Vec<String> = Vec::new();
    let mut in_run = false;
    for word in text.split_whitespace() {
        if !key.is_empty() {
            key.push(' ');
            in_run = false;
        }
        for ch in word.chars() {
            if ch.is_ascii_digit() {
                if !in_run {
                    key.push('#');
                    runs.push(String::new());
                    in_run = true;
                }
                if let Some(run) = runs.last_mut() {
                    run.push(ch);
                }
            } else {
                in_run = false;
                key.push(ch);
            }
        }
    }
    (key, runs)
}

/// Which band a line stands in, if any.
fn band_of(line: &GlyphLine, height: f64) -> Option<Band> {
    band_within(line, height, BAND)
}

/// Which band of `share` of the page's height a line stands in, if any.
fn band_within(line: &GlyphLine, height: f64, share: f64) -> Option<Band> {
    if line.baseline < height * share {
        Some(Band::Top)
    } else if line.baseline > height * (1.0 - share) {
        Some(Band::Bottom)
    } else {
        None
    }
}

/// What each digit run of a cluster does, and the page number's offset; `None`
/// when a run changes from page to page without counting with it (a chapter's
/// title, a date per page): that is no running line.
fn digits_of(copies: &[(usize, Vec<String>)]) -> Option<(Vec<Digits>, Option<i64>)> {
    let first = &copies.first()?.1;
    let mut digits = Vec::with_capacity(first.len());
    let mut offset: Option<i64> = None;
    for (index, value) in first.iter().enumerate() {
        let same = copies
            .iter()
            .all(|(_, runs)| runs.get(index).is_some_and(|run| run == value));
        if same {
            digits.push(Digits::Fixed);
            continue;
        }
        let mut run_offset: Option<i64> = None;
        for (page, runs) in copies {
            let run = runs.get(index)?;
            if run.len() > MAX_COUNTED_DIGITS {
                return None;
            }
            let value: i64 = run.parse().ok()?;
            let number = i64::try_from(*page).ok()? + 1;
            let here = value - number;
            if run_offset.is_some_and(|existing| existing != here) {
                return None;
            }
            run_offset = Some(here);
        }
        if offset.is_some_and(|existing| Some(existing) != run_offset) {
            return None;
        }
        offset = run_offset;
        digits.push(Digits::Counts);
    }
    Some((digits, offset))
}

/// Finds the running lines of a document from its pages' lines.
///
/// A line in the top or bottom [`BAND`] of its page is a running line when a
/// line of the same band, at the same height within [`SAME_Y`], with the same
/// text once digits are set aside, stands on at least two pages and on at
/// least half of them. Its digits must be the same on every page or count with
/// the page; a document's page numbers count with one offset. Lines on pages of
/// one parity only, in a document of four pages or more, are a book's left or
/// right heads and are left alone: one part on every page would put them on
/// the wrong side half the time.
#[must_use]
pub(crate) fn detect(lines: &[Vec<GlyphLine>], heights: &[f64]) -> Running {
    let pages = lines.len();
    let mut candidates: Vec<Cluster> = Vec::new();
    let mut runs_of: Vec<Vec<(usize, Vec<String>)>> = Vec::new();
    for (page, (page_lines, height)) in lines.iter().zip(heights).enumerate() {
        // A page whose only text is at its top or foot has no body for a
        // header to run above: that text is the page.
        let has_body = page_lines
            .iter()
            .any(|line| band_of(line, *height).is_none() && !line.text().trim().is_empty());
        if !has_body {
            continue;
        }
        for (index, line) in page_lines.iter().enumerate() {
            let Some(band) = band_within(line, *height, OUTER_BAND) else {
                continue;
            };
            let (key, runs) = split_digits(&line.text());
            if key.is_empty() {
                continue;
            }
            let found = candidates.iter().position(|cluster| {
                cluster.band == band
                    && cluster.key == key
                    && (cluster.y - line.baseline).abs() <= SAME_Y
                    && cluster.copies.last().is_some_and(|(last, _)| *last != page)
            });
            let slot = found.and_then(|at| candidates.get_mut(at).zip(runs_of.get_mut(at)));
            if let Some((cluster, runs_here)) = slot {
                cluster.copies.push((page, index));
                runs_here.push((page, runs));
            } else {
                candidates.push(Cluster {
                    band,
                    key,
                    y: line.baseline,
                    size: line.size,
                    inner: band_of(line, *height).is_some(),
                    copies: vec![(page, index)],
                    digits: Vec::new(),
                    offset: None,
                });
                runs_of.push(vec![(page, runs)]);
            }
        }
    }
    let required = pages.div_ceil(2).max(2);
    let mut valid: Vec<Cluster> = Vec::new();
    for (mut cluster, runs) in candidates.into_iter().zip(runs_of) {
        if cluster.copies.len() < required {
            continue;
        }
        if pages >= 4 {
            let parity = cluster.copies.first().map_or(0, |(page, _)| page % 2);
            if cluster.pages().all(|page| page % 2 == parity) {
                continue;
            }
        }
        let Some((digits, own)) = digits_of(&runs) else {
            continue;
        };
        cluster.digits = digits;
        cluster.offset = own;
        valid.push(cluster);
    }
    // The band's lines first, then the lines beyond it that join them, a
    // line pitch at a time.
    let mut accepted = vec![false; valid.len()];
    let mut order: Vec<usize> = Vec::new();
    loop {
        let mut grew = false;
        for index in 0..valid.len() {
            if accepted.get(index).copied().unwrap_or(true) {
                continue;
            }
            let Some(cluster) = valid.get(index) else {
                continue;
            };
            let joins = cluster.inner
                || order.iter().filter_map(|at| valid.get(*at)).any(|other| {
                    other.band == cluster.band
                        && (other.y - cluster.y).abs() <= other.size.max(cluster.size) * 2.5
                });
            if joins {
                if let Some(slot) = accepted.get_mut(index) {
                    *slot = true;
                }
                order.push(index);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut offset: Option<i64> = None;
    for index in order {
        let Some(cluster) = valid.get(index) else {
            continue;
        };
        if let Some(own) = cluster.offset {
            // Page 1 shows `1 + offset`, and a section cannot start below zero.
            if own < -1 || offset.is_some_and(|existing| existing != own) {
                continue;
            }
            offset = Some(own);
        }
        clusters.push(cluster.clone());
    }
    Running { clusters, pages }
}

impl Running {
    /// Whether nothing repeats.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }

    /// Whether line `line` of page `page` is a running line.
    #[must_use]
    pub(crate) fn is_running(&self, page: usize, line: usize) -> bool {
        self.clusters
            .iter()
            .any(|cluster| cluster.copies.contains(&(page, line)))
    }

    /// Whether a page's line leaves its flow: a running line, or a blank one
    /// in a band whose running lines left. The blank lines a producer sets
    /// around its header and footer went with them into the page's margin;
    /// left in the flow, they were a page's worth of empty paragraphs at the
    /// foot, and the next page's text started a page late.
    #[must_use]
    pub(crate) fn leaves_flow(
        &self,
        page: usize,
        number: usize,
        line: &GlyphLine,
        height: f64,
    ) -> bool {
        if self.is_running(page, number) {
            return true;
        }
        line.text().trim().is_empty()
            && band_of(line, height).is_some_and(|band| self.has(page, band))
    }

    /// Whether page `page` lost a running line of `band` to a part.
    #[must_use]
    fn has(&self, page: usize, band: Band) -> bool {
        self.clusters
            .iter()
            .any(|cluster| cluster.band == band && cluster.pages().any(|here| here == page))
    }

    /// The page number of the document's first page, when it is not 1.
    fn first_number(&self) -> Option<u32> {
        let offset = self.clusters.iter().find_map(|cluster| cluster.offset)?;
        if offset == 0 {
            return None;
        }
        u32::try_from(offset + 1).ok()
    }
}

/// The parts the running lines became, and the room they take.
#[derive(Clone, Debug, Default)]
pub(crate) struct Decorations {
    parts: Vec<HeaderFooter>,
    /// Header part ids: every page's, and the first page's when it differs.
    header: Option<(PartId, Option<PartId>)>,
    footer: Option<(PartId, Option<PartId>)>,
    /// Points from the page's top to the header's top, and to its foot.
    header_room: Option<(f64, f64)>,
    /// Points from the page's foot to the footer's foot, and to its top.
    footer_room: Option<(f64, f64)>,
    page_start: Option<u32>,
    /// Which pages lost header and footer lines: their margins are the parts'.
    top_pages: BTreeSet<usize>,
    bottom_pages: BTreeSet<usize>,
}

/// One piece of a running row: the cluster's first copy.
struct Piece<'l> {
    cluster: &'l Cluster,
    line: &'l GlyphLine,
}

impl Decorations {
    /// Builds the header and footer parts of the running lines.
    pub(crate) fn build(
        running: &Running,
        pages: &[PdfPage],
        lines: &[Vec<GlyphLine>],
        report: &mut ConversionReport,
    ) -> Self {
        let mut decorations = Self::default();
        if running.is_empty() {
            return decorations;
        }
        for cluster in &running.clusters {
            let text = cluster
                .copies
                .first()
                .and_then(|(page, line)| lines.get(*page)?.get(*line))
                .map(|line| line.text().trim().to_owned())
                .unwrap_or_default();
            let (id, place) = match cluster.band {
                Band::Top => ("layout.running-header", "top"),
                Band::Bottom => ("layout.running-footer", "foot"),
            };
            report.record(
                id,
                Severity::Inferred,
                format!(
                    "'{text}' repeats at the {place} of {} of {} pages and is set as the page's {}",
                    cluster.copies.len(),
                    running.pages,
                    if cluster.band == Band::Top {
                        "header"
                    } else {
                        "footer"
                    },
                ),
            );
            if cluster.digits.contains(&Digits::Counts) {
                report.record(
                    "layout.page-number",
                    Severity::Inferred,
                    format!("the number in '{text}' counts with the page and is a PAGE field"),
                );
            }
        }
        decorations.page_start = running.first_number();
        for band in [Band::Top, Band::Bottom] {
            let clusters: Vec<&Cluster> = running
                .clusters
                .iter()
                .filter(|cluster| cluster.band == band)
                .collect();
            let Some(first) = clusters.first() else {
                continue;
            };
            let Some(page) = first.copies.first().and_then(|(page, _)| pages.get(*page)) else {
                continue;
            };
            let frame = text_frame(page);
            let pieces = |only_first: bool| -> Vec<Piece<'_>> {
                clusters
                    .iter()
                    .filter(|cluster| !only_first || cluster.pages().any(|page| page == 0))
                    .filter_map(|cluster| {
                        let (page, line) = cluster.copies.first()?;
                        Some(Piece {
                            cluster,
                            line: lines.get(*page)?.get(*line)?,
                        })
                    })
                    .collect()
            };
            let every = pieces(false);
            let first_page = pieces(true);
            let (blocks, room) = part_blocks(&every, frame);
            let height = page.geometry.height;
            let pages_here: BTreeSet<usize> = (0..running.pages)
                .filter(|page| running.has(*page, band))
                .collect();
            let (role, kind) = match band {
                Band::Top => ("header", true),
                Band::Bottom => ("footer", false),
            };
            let number = decorations
                .parts
                .iter()
                .filter(|part| part.is_header == kind)
                .count()
                + 1;
            let part = PartId::new(format!("/word/{role}{number}.xml").as_str());
            decorations.parts.push(HeaderFooter {
                part: part.clone(),
                is_header: kind,
                blocks,
                location: SourceLocation::unknown(),
            });
            // The first page shows only what the first page had: a cover with
            // no running head, a letter whose page number starts on page 2.
            let first_part = (first_page.len() != every.len()).then(|| {
                let (blocks, _) = part_blocks(&first_page, frame);
                let part = PartId::new(format!("/word/{role}{}.xml", number + 1).as_str());
                decorations.parts.push(HeaderFooter {
                    part: part.clone(),
                    is_header: kind,
                    blocks,
                    location: SourceLocation::unknown(),
                });
                part
            });
            if first_part.is_some() {
                report.record(
                    "layout.first-page-decoration",
                    Severity::Inferred,
                    format!("the first page lacks some of the running {role}, and has a {role} of its own"),
                );
            }
            match band {
                Band::Top => {
                    decorations.header = Some((part, first_part));
                    decorations.header_room = Some(room);
                    decorations.top_pages = pages_here;
                }
                Band::Bottom => {
                    decorations.footer = Some((part, first_part));
                    decorations.footer_room = Some((height - room.1, height - room.0));
                    decorations.bottom_pages = pages_here;
                }
            }
        }
        decorations
    }

    /// Gives a page's section the room its header and footer take: a page
    /// that lost its running lines has its margins where they ended, which is
    /// where the body's ink began before (the ink bounds put the margin at the
    /// header's top).
    pub(crate) fn adjust(&self, page: usize, section: &mut SectionProperties) {
        if self.parts.is_empty() {
            return;
        }
        let margins = section
            .page_margins
            .get_or_insert_with(PageMargins::default);
        if let Some((top, foot)) = self.header_room {
            margins.header = Some(Twips(to_twips(top.max(0.0).floor())));
            if self.top_pages.contains(&page) {
                margins.top = Some(Twips(to_twips(foot.ceil())));
            }
        }
        if let Some((foot, top)) = self.footer_room {
            margins.footer = Some(Twips(to_twips(foot.max(0.0).floor())));
            if self.bottom_pages.contains(&page) {
                margins.bottom = Some(Twips(to_twips(top.ceil())));
            }
        }
    }

    /// Moves the parts into the document and points every section at them.
    pub(crate) fn attach(self, document: &mut Document) {
        if self.parts.is_empty() {
            return;
        }
        // The writer writes every section but the last from the paragraph that
        // ends it, and the last from `sections`; both copies say the same, and
        // only the document's first section has the first page.
        let mut first = true;
        for block in &mut document.body.blocks {
            if let Block::Paragraph(paragraph) = block {
                if let Some(section) = paragraph.props.section.as_mut() {
                    self.apply(section, first);
                    first = false;
                }
            }
        }
        for (index, section) in document.sections.iter_mut().enumerate() {
            self.apply(&mut section.properties, index == 0);
        }
        document.headers_footers.extend(self.parts);
    }

    /// Points one section at the parts.
    fn apply(&self, section: &mut SectionProperties, first: bool) {
        let title = first
            && [&self.header, &self.footer]
                .iter()
                .any(|part| part.as_ref().is_some_and(|(_, own)| own.is_some()));
        section.headers.clear();
        section.footers.clear();
        for (part, references) in [
            (&self.header, &mut section.headers),
            (&self.footer, &mut section.footers),
        ] {
            let Some((every, own)) = part else {
                continue;
            };
            references.push(reference(HeaderFooterKind::Default, every));
            if title {
                references.push(reference(
                    HeaderFooterKind::First,
                    own.as_ref().unwrap_or(every),
                ));
            }
        }
        section.title_page = title;
        if let (true, Some(start)) = (first, self.page_start) {
            section
                .page_number
                .get_or_insert(PageNumberType {
                    format: None,
                    start: None,
                    chapter_style: None,
                    chapter_separator: None,
                })
                .start = Some(start);
        }
    }
}

fn reference(kind: HeaderFooterKind, part: &PartId) -> HeaderFooterRef {
    HeaderFooterRef {
        kind,
        rel_id: RelId::new(part.as_str().rsplit('/').next().unwrap_or("hf")),
        part: Some(part.clone()),
    }
}

/// The page's text area: its left and right margins, in points from the
/// page's left edge.
fn text_frame(page: &PdfPage) -> (f64, f64) {
    let section = crate::geometry::section_for(page, false);
    let margins = section.page_margins.unwrap_or_default();
    let left = margins.left.map_or(0.0, |left| f64::from(left.0) / 20.0);
    let right = margins.right.map_or(0.0, |right| f64::from(right.0) / 20.0);
    (left, page.geometry.width - right)
}

/// A part's paragraphs, one per row of pieces on one baseline, and the room
/// they take: points from the page's top to the part's top and to its foot.
fn part_blocks(pieces: &[Piece<'_>], frame: (f64, f64)) -> (Vec<Block>, (f64, f64)) {
    let mut rows: Vec<Vec<&Piece<'_>>> = Vec::new();
    let mut sorted: Vec<&Piece<'_>> = pieces.iter().collect();
    sorted.sort_by(|a, b| a.line.baseline.total_cmp(&b.line.baseline));
    for piece in sorted {
        match rows.last_mut() {
            Some(row)
                if row.first().is_some_and(|first| {
                    (first.line.baseline - piece.line.baseline).abs() <= SAME_Y
                }) =>
            {
                row.push(piece);
            }
            _ => rows.push(vec![piece]),
        }
    }
    let mut blocks = Vec::with_capacity(rows.len().max(1));
    let mut top = None;
    let mut height = 0.0;
    let mut previous: Option<f64> = None;
    for row in &mut rows {
        row.sort_by(|a, b| a.line.x.total_cmp(&b.line.x));
        let baseline = row.first().map_or(0.0, |piece| piece.line.baseline);
        let size = row.iter().map(|piece| piece.line.size).fold(0.0, f64::max);
        let line = match previous {
            Some(above) if baseline - above >= size * 0.8 => baseline - above,
            _ => size * FIRST_LINE,
        };
        if top.is_none() {
            top = Some(baseline - line * EXACT_LINE_ASCENT);
        }
        height += line;
        previous = Some(baseline);
        blocks.push(Block::Paragraph(row_paragraph(row, frame, line)));
    }
    if blocks.is_empty() {
        blocks.push(Block::Paragraph(paragraph(ParagraphProperties::default())));
    }
    let top = top.unwrap_or(0.0);
    (blocks, (top, top + height))
}

/// Where a piece stands on its row.
enum Stop {
    /// At the text's left edge.
    Left,
    /// Centred on the text.
    Center,
    /// Ending at the text's right edge.
    Right,
    /// Starting at an x of its own, points from the left margin.
    At(f64),
}

/// A stretch of a running line between two wide gaps: the reader joins a
/// running head's left, middle and right parts into one line when they stand
/// close, and each part has a place of its own.
struct Segment<'l> {
    cluster: &'l Cluster,
    glyphs: &'l [Glyph],
    x: f64,
    end: f64,
    /// The digit runs of the line before this segment's.
    first_run: usize,
}

/// A blank wider than this share of the size parts a line's segments.
const WIDE_GAP: f64 = 1.0;

/// The digit runs a glyph's text starts, given whether the text before it
/// ended in a digit; returns whether it ends in one.
fn count_runs(text: &str, runs: &mut usize, mut in_run: bool) -> bool {
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if !in_run {
                *runs += 1;
                in_run = true;
            }
        } else {
            in_run = false;
        }
    }
    in_run
}

fn segments<'l>(piece: &Piece<'l>) -> Vec<Segment<'l>> {
    let glyphs = piece.line.glyphs.as_slice();
    let mut out = Vec::new();
    let mut runs = 0usize;
    let mut in_run = false;
    let mut start: Option<(usize, usize)> = None;
    let close = |out: &mut Vec<Segment<'l>>, from: usize, to: usize, first_run: usize| {
        let Some(slice) = glyphs.get(from..to) else {
            return;
        };
        let mut inked = slice.iter().filter(|glyph| !glyph.text.trim().is_empty());
        let Some(first) = inked.next() else {
            return;
        };
        let last = inked.next_back().unwrap_or(first);
        out.push(Segment {
            cluster: piece.cluster,
            glyphs: slice,
            x: first.x,
            end: last.x + last.width,
            first_run,
        });
    };
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.text.trim().is_empty() && glyph.width > piece.line.size * WIDE_GAP {
            if let Some((from, first_run)) = start.take() {
                close(&mut out, from, index, first_run);
            }
            in_run = false;
            continue;
        }
        if start.is_none() {
            start = Some((index, runs));
        }
        in_run = count_runs(&glyph.text, &mut runs, in_run);
    }
    if let Some((from, first_run)) = start {
        close(&mut out, from, glyphs.len(), first_run);
    }
    out
}

fn stop_of(segment: &Segment<'_>, (left, right): (f64, f64)) -> Stop {
    let slack = ((right - left) * 0.02).max(3.0);
    if (segment.x - left).abs() <= 3.0 {
        Stop::Left
    } else if (f64::midpoint(segment.x, segment.end) - f64::midpoint(left, right)).abs() <= slack {
        Stop::Center
    } else if (segment.end - right).abs() <= 3.0 {
        Stop::Right
    } else {
        Stop::At(segment.x - left)
    }
}

fn paragraph(props: ParagraphProperties) -> Paragraph {
    Paragraph {
        props,
        inlines: Vec::new(),
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: SourceLocation::unknown(),
    }
}

fn tab_run() -> Inline {
    Inline::Run(Run {
        props: RunProperties::default(),
        content: vec![RunContent::Tab],
        revision: None,
        location: SourceLocation::unknown(),
    })
}

/// One row of a part: a single segment aligned where it stood, several
/// reached by tabs - a man page's `TAR(5)`, title and `TAR(5)` at the left,
/// the middle and the right.
fn row_paragraph(row: &[&Piece<'_>], frame: (f64, f64), line: f64) -> Paragraph {
    let mut paragraph = paragraph(ParagraphProperties {
        spacing: Some(Spacing {
            before: Some(Twips(0)),
            after: Some(Twips(0)),
            line: Some(Twips(to_twips(line))),
            line_rule: Some(LineSpacingRule::Exact),
            ..Spacing::default()
        }),
        ..ParagraphProperties::default()
    });
    let (left, right) = frame;
    let mut parts: Vec<Segment<'_>> = row.iter().flat_map(|piece| segments(piece)).collect();
    parts.sort_by(|a, b| a.x.total_cmp(&b.x));
    if let [only] = parts.as_slice() {
        match stop_of(only, frame) {
            Stop::Left => {}
            Stop::Center => paragraph.props.alignment = Some(Justification::Center),
            Stop::Right => paragraph.props.alignment = Some(Justification::End),
            Stop::At(x) => {
                paragraph.props.indentation = Some(Indentation {
                    start: Some(Twips(to_twips(x))),
                    ..Indentation::default()
                });
            }
        }
        push_segment(&mut paragraph, only);
        return paragraph;
    }
    let mut reached = f64::NEG_INFINITY;
    for (index, segment) in parts.iter().enumerate() {
        let tab = match stop_of(segment, frame) {
            Stop::Left if index == 0 => None,
            Stop::Left => Some((TabAlignment::Start, segment.x - left)),
            Stop::Center => Some((TabAlignment::Center, (right - left) / 2.0)),
            Stop::Right => Some((TabAlignment::End, right - left)),
            Stop::At(x) => Some((TabAlignment::Start, x)),
        };
        match tab {
            // Two pieces aimed at one stop, or out of order: a space keeps
            // them apart, where a tab would have jumped to the next stop.
            Some((_, position)) if position <= reached + 1.0 => paragraph.inlines.push(
                crate::semantic::run(RunProperties::default(), " ".to_owned()),
            ),
            Some((alignment, position)) => {
                paragraph.props.tabs.push(TabStop {
                    position: Twips(to_twips(position)),
                    alignment,
                    leader: None,
                });
                reached = position;
                paragraph.inlines.push(tab_run());
            }
            None => reached = 0.0,
        }
        push_segment(&mut paragraph, segment);
    }
    paragraph
}

/// The open run of a segment: the digit run it is a field for, its
/// properties, its text.
type OpenRun = (Option<usize>, RunProperties, String);

fn flush(paragraph: &mut Paragraph, open: Option<OpenRun>) {
    let Some((field, props, text)) = open else {
        return;
    };
    let run = crate::semantic::run(props, text);
    paragraph.inlines.push(match field {
        Some(_) => Inline::Field(Field {
            instruction: Some(Arc::from(" PAGE ")),
            inlines: vec![run],
            location: SourceLocation::unknown(),
        }),
        None => run,
    });
}

/// A segment's glyphs as runs, its counted digits as a `PAGE` field whose
/// cached result is the number the first copy showed.
fn push_segment(paragraph: &mut Paragraph, segment: &Segment<'_>) {
    let mut runs = segment.first_run;
    let mut in_run = false;
    let mut current: Option<OpenRun> = None;
    for glyph in segment.glyphs {
        let before = runs;
        let had_digit = glyph.text.chars().any(|ch| ch.is_ascii_digit());
        let continued = in_run && glyph.text.starts_with(|ch: char| ch.is_ascii_digit());
        in_run = count_runs(&glyph.text, &mut runs, in_run);
        // The run a digit glyph belongs to: the one it continues, or the
        // first it starts.
        let touched = had_digit.then(|| if continued { before - 1 } else { before });
        let field = touched.filter(|run| segment.cluster.digits.get(*run) == Some(&Digits::Counts));
        let props = crate::semantic::run_properties(glyph);
        match current.as_mut() {
            Some((open, style, text)) if *open == field && *style == props => {
                text.push_str(&glyph.text);
            }
            _ => {
                flush(paragraph, current.take());
                current = Some((field, props, glyph.text.clone()));
            }
        }
    }
    flush(paragraph, current);
}

#[cfg(test)]
mod tests {
    use super::{detect, split_digits, Band};
    use strict_ooxml_pdf::text::GlyphLine;
    use strict_ooxml_pdf::{Glyph, RenderMode, Rgb};

    const HEIGHT: f64 = 800.0;

    fn line(text: &str, x: f64, baseline: f64) -> GlyphLine {
        let glyphs: Vec<Glyph> = text
            .chars()
            .zip(0u32..)
            .map(|(ch, index)| Glyph {
                text: ch.to_string(),
                x: x + f64::from(index) * 5.0,
                y: baseline,
                width: 5.0,
                ascent: 8.0,
                descent: 2.0,
                size: 10.0,
                font: "Helvetica".to_owned(),
                color: Rgb(0.0, 0.0, 0.0),
                render_mode: RenderMode::Fill,
                mapped: true,
            })
            .collect();
        GlyphLine {
            x,
            baseline,
            size: 10.0,
            ascent: 8.0,
            width: glyphs.iter().map(|glyph| glyph.width).sum(),
            glyphs,
        }
    }

    /// Pages with a body line each and the given extra lines.
    fn pages(extra: &[Vec<GlyphLine>]) -> (Vec<Vec<GlyphLine>>, Vec<f64>) {
        let lines: Vec<Vec<GlyphLine>> = extra
            .iter()
            .enumerate()
            .map(|(page, extra)| {
                let mut lines = vec![line(&format!("body text of page {page}"), 72.0, 400.0)];
                lines.extend(extra.iter().cloned());
                lines
            })
            .collect();
        let heights = vec![HEIGHT; lines.len()];
        (lines, heights)
    }

    #[test]
    fn digits_are_set_aside() {
        let (key, runs) = split_digits("  Page 12 of  30 ");
        assert_eq!(key, "Page # of #");
        assert_eq!(runs, ["12", "30"]);
    }

    #[test]
    fn a_line_repeated_at_the_top_is_a_header() {
        let head = line("Annual report", 72.0, 40.0);
        let (lines, heights) = pages(&[vec![head.clone()], vec![head.clone()], vec![head]]);
        let running = detect(&lines, &heights);
        assert!((0..3).all(|page| running.is_running(page, 1)));
        assert!((0..3).all(|page| !running.is_running(page, 0)));
        assert_eq!(running.clusters.first().map(|c| c.band), Some(Band::Top));
    }

    #[test]
    fn a_number_that_counts_with_the_page_is_the_page_number() {
        let (lines, heights) = pages(&[
            vec![line("Page 1 of 3", 280.0, 770.0)],
            vec![line("Page 2 of 3", 280.0, 770.0)],
            vec![line("Page 3 of 3", 281.0, 771.5)],
        ]);
        let running = detect(&lines, &heights);
        let cluster = running.clusters.first().expect("a footer");
        assert_eq!(cluster.band, Band::Bottom);
        assert_eq!(
            cluster.digits,
            [super::Digits::Counts, super::Digits::Fixed]
        );
        assert_eq!(cluster.offset, Some(0));
        assert_eq!(running.first_number(), None);
    }

    #[test]
    fn a_numbering_that_starts_late_is_kept() {
        let (lines, heights) =
            pages(&[vec![line("5", 300.0, 770.0)], vec![line("6", 300.0, 770.0)]]);
        let running = detect(&lines, &heights);
        assert_eq!(running.first_number(), Some(5));
    }

    #[test]
    fn a_number_that_does_not_count_is_no_running_line() {
        let (lines, heights) = pages(&[
            vec![line("Chapter 1", 72.0, 40.0)],
            vec![line("Chapter 4", 72.0, 40.0)],
            vec![line("Chapter 9", 72.0, 40.0)],
        ]);
        assert!(detect(&lines, &heights).is_empty());
    }

    #[test]
    fn a_line_out_of_the_bands_or_at_another_height_is_not_running() {
        let (lines, heights) = pages(&[
            vec![line("Note", 72.0, 300.0), line("Head", 72.0, 40.0)],
            vec![line("Note", 72.0, 300.0), line("Head", 72.0, 46.0)],
        ]);
        assert!(detect(&lines, &heights).is_empty());
    }

    #[test]
    fn a_letterhead_line_below_the_band_joins_the_lines_in_it() {
        // 96 and 108 are past 12% of 800 (96); 150 is too far from the rest.
        let block = [
            line("Company", 300.0, 72.0),
            line("Address", 300.0, 84.0),
            line("City", 300.0, 96.0),
            line("Country", 300.0, 108.0),
            line("Stray", 300.0, 150.0),
        ];
        let (lines, heights) = pages(&[block.to_vec(), block.to_vec()]);
        let running = detect(&lines, &heights);
        let mut keys: Vec<&str> = running.clusters.iter().map(|c| c.key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["Address", "City", "Company", "Country"]);
    }

    #[test]
    fn a_line_on_one_page_of_many_is_not_running() {
        let head = line("Confidential", 72.0, 40.0);
        let (lines, heights) = pages(&[
            vec![head.clone()],
            vec![head],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ]);
        // Two of five pages is less than half.
        assert!(detect(&lines, &heights).is_empty());
    }

    #[test]
    fn heads_on_one_side_of_a_book_are_left_alone() {
        let head = line("Chapter title", 72.0, 40.0);
        let (lines, heights) = pages(&[
            vec![head.clone()],
            Vec::new(),
            vec![head.clone()],
            Vec::new(),
            vec![head],
            Vec::new(),
        ]);
        assert!(detect(&lines, &heights).is_empty());
    }

    #[test]
    fn a_page_with_text_only_at_its_top_has_no_header() {
        let head = line("Same", 72.0, 40.0);
        let lines = vec![vec![head.clone()], vec![head.clone()], vec![head]];
        let heights = vec![HEIGHT; 3];
        assert!(detect(&lines, &heights).is_empty());
    }

    #[test]
    fn a_single_page_has_no_running_lines() {
        let (lines, heights) = pages(&[vec![line("Head", 72.0, 40.0)]]);
        assert!(detect(&lines, &heights).is_empty());
    }
}
