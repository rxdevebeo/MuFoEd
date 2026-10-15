//! A scanned page rebuilt from its blocks: headings, paragraphs and pictures,
//! with the alignment, size and weight the pixels show.
//!
//! Sent whole to a model, a scan comes back as lines of text and nothing else.
//! Asked for its layout ([`TextRecovery::recover_layout`]), a model names the
//! blocks - title, paragraph, figure - with a box and the text. The model is
//! good at *what* a block is and what it says, and poor at *where* it is and
//! *how* it is set: boxes drift, especially on pictures, and bold goes
//! unnoticed. So every box is checked against the page's own ink before
//! anything is built from it, and the geometry comes from the ink:
//!
//! - a box is snapped to the ink inside it (a little margin allowed), and one
//!   with no ink is dropped;
//! - its text lines are where the ink rows run; their pitch gives the type
//!   size, their edges the alignment, the thickness of their strokes the
//!   weight;
//! - a figure is cut out of the page and placed as a picture, the text the
//!   model read inside it becoming its description;
//! - a page that is mostly not paper - a cover, a full-page photograph - is
//!   kept whole as a picture, its text as the description, because rebuilding
//!   it as paragraphs loses everything that made it what it is.
//!
//! The model's words are repaired by the language's word list and tagged with
//! the language, as for every recovered paragraph.

// Pixel counts and coordinates become points: precision far beyond a pixel.
#![allow(clippy::cast_precision_loss)]

use std::sync::Arc;

use strict_ooxml_ocr::{Align, BlockKind, LayoutBlock, TextRecovery};
use strict_ooxml_pdf::raster::Pixels;
use strict_ooxml_pdf::Encoded;
use strict_ooxml_wml::model::block::Paragraph;
use strict_ooxml_wml::model::ids::StyleId;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::props::{Language, ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::values::{
    HalfPoints, Indentation, Justification, Rsids, Space, Spacing, TriState, Twips,
};

use super::policy::{language_of, RECOVERED_STYLE_ID};
use super::{RecoveredItem, RecoveredPicture};
use crate::report::{ConversionReport, Severity};
use crate::PdfOptions;

/// Share of a page that is not paper past which the page is kept as a picture.
const PICTURE_PAGE_RATIO: f64 = 0.5;

/// Stroke-to-band ratio over the page's body text past which a heading is
/// bold. Only headings: at the 144 dpi a model reads, a body stroke is 2-3
/// pixels, and a paragraph with a few bold words measures like a bold one.
const BOLD_RATIO: f64 = 1.15;

/// The page's pixels classified: ink (text and line art) and paper.
pub(crate) struct Ink {
    width: usize,
    height: usize,
    /// Type and line art: what is clearly darker than the page's typical tone,
    /// or clearly coloured and darker than it (light-blue headings).
    dark: Vec<bool>,
    /// Pixels that are neither white paper nor near it: colour, grey, ink.
    unpaper: Vec<bool>,
}

impl Ink {
    /// Classifies RGBA pixels. Ink is what stands out from the page's typical
    /// tone - so purple type on a yellow cover is ink and the yellow is not.
    pub(crate) fn of(pixels: &Pixels) -> Self {
        let width = pixels.size.width as usize;
        let height = pixels.size.height as usize;
        let tone = |pixel: &[u8]| -> (i32, i32) {
            match pixel {
                [r, g, b, _] => {
                    let luma =
                        0.299 * f64::from(*r) + 0.587 * f64::from(*g) + 0.114 * f64::from(*b);
                    let chroma = i32::from((*r).max(*g).max(*b)) - i32::from((*r).min(*g).min(*b));
                    (luma.round() as i32, chroma)
                }
                _ => (255, 0),
            }
        };
        let mut histogram = [0usize; 256];
        for pixel in pixels.rgba.chunks_exact(4) {
            let (luma, _) = tone(pixel);
            if let Some(slot) =
                histogram.get_mut(usize::try_from(luma.clamp(0, 255)).unwrap_or(255))
            {
                *slot += 1;
            }
        }
        let half = width * height / 2;
        let mut seen = 0;
        let mut median = 255i32;
        for (value, count) in histogram.iter().enumerate() {
            seen += count;
            if seen > half {
                median = i32::try_from(value).unwrap_or(255);
                break;
            }
        }
        let threshold = (median - 70).clamp(60, 170);
        let mut dark = Vec::with_capacity(width * height);
        let mut unpaper = Vec::with_capacity(width * height);
        for pixel in pixels.rgba.chunks_exact(4) {
            let (luma, chroma) = tone(pixel);
            let brightest = pixel.iter().take(3).copied().max().unwrap_or(255);
            dark.push(luma < threshold || (chroma > 60 && luma < median - 20));
            unpaper.push(brightest < 215 || chroma > 40);
        }
        Self {
            width,
            height,
            dark,
            unpaper,
        }
    }

    fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && self.dark.get(y * self.width + x).copied().unwrap_or(false)
    }

    /// The share of the page that is not paper.
    pub(crate) fn unpaper_ratio(&self) -> f64 {
        let total = (self.width * self.height).max(1);
        self.unpaper.iter().filter(|unpaper| **unpaper).count() as f64 / total as f64
    }

    /// Ink pixels in row `y` between `left` and `right` (exclusive).
    fn row_ink(&self, y: usize, left: usize, right: usize) -> usize {
        (left..right).filter(|x| self.is_dark(*x, y)).count()
    }

    fn column_ink(&self, x: usize, top: usize, bottom: usize) -> usize {
        (top..bottom).filter(|y| self.is_dark(x, *y)).count()
    }

    /// Solid areas of colour or tone on a paper page - a sticker, a coloured
    /// panel, a photograph: cells of 16 px that are mostly not paper, joined,
    /// at least 1% of the page. Thin strips along the page (a scan's dark
    /// edge, a coloured thumb index) are not pictures.
    pub(crate) fn filled_regions(&self) -> Vec<PixelBox> {
        const CELL: usize = 16;
        let (columns, rows) = (self.width / CELL, self.height / CELL);
        if columns == 0 || rows == 0 {
            return Vec::new();
        }
        let mut filled = vec![false; columns * rows];
        for row in 0..rows {
            for column in 0..columns {
                let mut count = 0usize;
                for y in row * CELL..(row + 1) * CELL {
                    let start = y * self.width + column * CELL;
                    count += self
                        .unpaper
                        .get(start..start + CELL)
                        .unwrap_or_default()
                        .iter()
                        .filter(|unpaper| **unpaper)
                        .count();
                }
                if let Some(cell) = filled.get_mut(row * columns + column) {
                    *cell = count * 2 > CELL * CELL;
                }
            }
        }
        let mut seen = vec![false; filled.len()];
        let mut regions = Vec::new();
        for start in 0..filled.len() {
            if !filled.get(start).copied().unwrap_or(false)
                || seen.get(start).copied().unwrap_or(true)
            {
                continue;
            }
            let (mut left, mut top, mut right, mut bottom, mut cells) =
                (usize::MAX, usize::MAX, 0, 0, 0);
            let mut stack = vec![start];
            if let Some(mark) = seen.get_mut(start) {
                *mark = true;
            }
            while let Some(index) = stack.pop() {
                let (row, column) = (index / columns, index % columns);
                cells += 1;
                left = left.min(column);
                right = right.max(column + 1);
                top = top.min(row);
                bottom = bottom.max(row + 1);
                let mut visit = |next: usize| {
                    if filled.get(next).copied().unwrap_or(false)
                        && !seen.get(next).copied().unwrap_or(true)
                    {
                        if let Some(mark) = seen.get_mut(next) {
                            *mark = true;
                        }
                        stack.push(next);
                    }
                };
                if column > 0 {
                    visit(index - 1);
                }
                if column + 1 < columns {
                    visit(index + 1);
                }
                if row > 0 {
                    visit(index - columns);
                }
                if row + 1 < rows {
                    visit(index + columns);
                }
            }
            let thin = (right - left) * 100 < columns * 6 || (bottom - top) * 100 < rows * 6;
            if cells * 100 >= columns * rows && !thin {
                regions.push(PixelBox {
                    left: left * CELL,
                    top: top * CELL,
                    right: right * CELL,
                    bottom: bottom * CELL,
                });
            }
        }
        regions
    }
}

/// A box in pixels: left, top, right, bottom (exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PixelBox {
    pub(crate) left: usize,
    pub(crate) top: usize,
    pub(crate) right: usize,
    pub(crate) bottom: usize,
}

impl PixelBox {
    fn width(&self) -> usize {
        self.right.saturating_sub(self.left)
    }

    fn height(&self) -> usize {
        self.bottom.saturating_sub(self.top)
    }
}

/// The model's box snapped to the ink inside it, with a margin of 1% of the
/// page around it for a box that cut a line short. `None` when there is no ink.
pub(crate) fn snap(ink: &Ink, bbox: [f64; 4]) -> Option<PixelBox> {
    let (width, height) = (ink.width as f64, ink.height as f64);
    let margin = (width.max(height) * 0.01).round();
    let edge = |value: f64, limit: usize| value.round().clamp(0.0, limit as f64) as usize;
    let [x1, y1, x2, y2] = bbox;
    let outer = PixelBox {
        left: edge(x1 * width - margin, ink.width),
        top: edge(y1 * height - margin, ink.height),
        right: edge(x2 * width + margin, ink.width),
        bottom: edge(y2 * height + margin, ink.height),
    };
    if outer.width() == 0 || outer.height() == 0 {
        return None;
    }
    // A speck is not ink: a row or column needs a few dark pixels.
    let noise = 1 + outer.width().max(outer.height()) / 400;
    let rows: Vec<usize> = (outer.top..outer.bottom)
        .filter(|y| ink.row_ink(*y, outer.left, outer.right) > noise)
        .collect();
    let (top, bottom) = (*rows.first()?, *rows.last()? + 1);
    let columns: Vec<usize> = (outer.left..outer.right)
        .filter(|x| ink.column_ink(*x, top, bottom) > noise)
        .collect();
    let (left, right) = (*columns.first()?, *columns.last()? + 1);
    Some(PixelBox {
        left,
        top,
        right,
        bottom,
    })
}

/// One text line inside a block, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Line {
    pub(crate) top: usize,
    pub(crate) bottom: usize,
    pub(crate) left: usize,
    pub(crate) right: usize,
    /// Rows of the line's main band - the x-height of mixed case, the cap
    /// height of capitals - where most of its ink is.
    pub(crate) core: usize,
    /// Mean length of a horizontal run of ink: the stroke thickness.
    stroke: f64,
}

/// The text lines of a block: runs of rows with ink. A run much shorter than
/// the block's lines - accents over capitals, a háček, dots - belongs to the
/// line below it (or above, at the end).
pub(crate) fn lines_of(ink: &Ink, area: PixelBox) -> Vec<Line> {
    let noise = 1 + area.width() / 400;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for y in area.top..=area.bottom {
        let inked = y < area.bottom && ink.row_ink(y, area.left, area.right) > noise;
        match (inked, start) {
            (true, None) => start = Some(y),
            (false, Some(from)) => {
                runs.push((from, y));
                start = None;
            }
            _ => {}
        }
    }
    let tallest = runs
        .iter()
        .map(|(top, bottom)| bottom - top)
        .max()
        .unwrap_or(0);
    let mut heights: Vec<usize> = runs
        .iter()
        .map(|(top, bottom)| bottom - top)
        .filter(|height| height * 2 >= tallest)
        .collect();
    heights.sort_unstable();
    let typical = heights.get(heights.len() / 2).copied().unwrap_or(0);
    let mut joined: Vec<(usize, usize)> = Vec::new();
    let mut pending: Option<usize> = None;
    for (top, bottom) in runs {
        if (bottom - top) * 2 < typical {
            pending.get_or_insert(top);
            continue;
        }
        joined.push((pending.take().map_or(top, |accent| accent.min(top)), bottom));
    }
    if let (Some(_), Some(last)) = (pending, joined.last_mut()) {
        // Marks under the last line (a cedilla, an underline) stay with it.
        last.1 = area.bottom.max(last.1);
    }
    joined
        .into_iter()
        .map(|(top, bottom)| measure_line(ink, area, top, bottom))
        .collect()
}

fn measure_line(ink: &Ink, area: PixelBox, top: usize, bottom: usize) -> Line {
    let (mut left, mut right) = (area.right, area.left);
    let (mut pixels, mut runs) = (0usize, 0usize);
    let mut row_counts = Vec::with_capacity(bottom.saturating_sub(top));
    for y in top..bottom {
        let mut previous = false;
        let mut count = 0usize;
        for x in area.left..area.right {
            let dark = ink.is_dark(x, y);
            if dark {
                left = left.min(x);
                right = right.max(x + 1);
                count += 1;
                if !previous {
                    runs += 1;
                }
            }
            previous = dark;
        }
        pixels += count;
        row_counts.push(count);
    }
    let busiest = row_counts.iter().copied().max().unwrap_or(0);
    let core = row_counts
        .iter()
        .filter(|count| **count * 2 >= busiest && **count > 0)
        .count();
    Line {
        top,
        bottom,
        left: left.min(right),
        right,
        core,
        stroke: if runs == 0 {
            0.0
        } else {
            pixels as f64 / runs as f64
        },
    }
}

/// What the ink says about a text block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Measured {
    pub(crate) area: PixelBox,
    pub(crate) lines: Vec<Line>,
}

impl Measured {
    /// Type size in points: line pitch over 1.2; for a single line its main
    /// band, which is the cap height (0.7 em) of capitals and the x-height
    /// (0.5 em) of mixed case.
    pub(crate) fn size_pt(&self, scale: f64, capitals: bool) -> Option<f64> {
        let pitches: Vec<f64> = self
            .lines
            .windows(2)
            .filter_map(|pair| match pair {
                [first, second] => Some((second.top as f64 - first.top as f64) / scale),
                _ => None,
            })
            .collect();
        let size = if pitches.is_empty() {
            let line = self.lines.first()?;
            line.core as f64 / scale / if capitals { 0.7 } else { 0.5 }
        } else {
            median(pitches) / 1.2
        };
        (size.is_finite() && size > 0.0).then(|| ((size * 2.0).round() / 2.0).clamp(6.0, 72.0))
    }

    /// The block's stroke thickness relative to its lines' main band.
    fn stroke_ratio(&self) -> Option<f64> {
        let ratios: Vec<f64> = self
            .lines
            .iter()
            .filter(|line| line.core > 0 && line.stroke > 0.0)
            .map(|line| line.stroke / line.core as f64)
            .collect();
        (!ratios.is_empty()).then(|| median(ratios))
    }

    /// The alignment the line edges show, within `column` (the page's text
    /// column, pixels); `fallback` for a single line that tells nothing.
    pub(crate) fn alignment(&self, column: (usize, usize), fallback: Option<Align>) -> Align {
        let width = self.area.width().max(1) as f64;
        let tolerance = width * 0.02 + 3.0;
        let spread = |values: Vec<f64>| {
            let max = values.iter().copied().fold(f64::MIN, f64::max);
            let min = values.iter().copied().fold(f64::MAX, f64::min);
            max - min
        };
        if self.lines.len() >= 2 {
            let lefts: Vec<f64> = self.lines.iter().map(|line| line.left as f64).collect();
            // The last line of a justified paragraph is short.
            let body = self.lines.get(..self.lines.len() - 1).unwrap_or_default();
            let rights: Vec<f64> = body.iter().map(|line| line.right as f64).collect();
            let all_rights: Vec<f64> = self.lines.iter().map(|line| line.right as f64).collect();
            let centres: Vec<f64> = self
                .lines
                .iter()
                .map(|line| (line.left + line.right) as f64 / 2.0)
                .collect();
            let (left_even, right_even) = (
                spread(lefts.clone()) <= tolerance,
                spread(rights) <= tolerance,
            );
            // An indented first line still makes a flush-left paragraph.
            let rest_left = self.lines.len() >= 3
                && spread(lefts.get(1..).unwrap_or_default().to_vec()) <= tolerance
                && lefts.first() > lefts.get(1);
            return if (left_even || rest_left) && right_even && self.lines.len() >= 3 {
                Align::Justify
            } else if left_even || rest_left {
                Align::Left
            } else if spread(all_rights) <= tolerance {
                Align::Right
            } else if spread(centres) <= tolerance {
                Align::Center
            } else {
                fallback.unwrap_or(Align::Left)
            };
        }
        // One line: where it sits in the page's text column.
        let (column_left, column_right) = column;
        let column_width = column_right.saturating_sub(column_left).max(1) as f64;
        let tolerance = (column_width * 0.02).max(6.0);
        let left_gap = self.area.left as f64 - column_left as f64;
        let right_gap = column_right as f64 - self.area.right as f64;
        if left_gap <= tolerance && right_gap <= tolerance {
            fallback
                .filter(|align| *align != Align::Center)
                .unwrap_or(Align::Left)
        } else if (left_gap - right_gap).abs() <= tolerance * 2.0 {
            Align::Center
        } else if right_gap <= tolerance {
            Align::Right
        } else {
            Align::Left
        }
    }

    /// How far the first line starts right of the others, in pixels.
    fn first_line_indent(&self) -> usize {
        match self.lines.as_slice() {
            [first, second, ..] => first.left.saturating_sub(second.left),
            _ => 0,
        }
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// Where the page sits: the region's offset on the page and the raster scale.
pub(crate) struct Frame {
    /// Pixels per point.
    pub(crate) scale: f64,
    /// 1-based page number, for the report.
    pub(crate) page: usize,
}

/// The page rebuilt from the model's layout, or `None` when the model gave
/// none (the caller then asks for the plain text).
pub(crate) fn rebuild(
    recovery: &dyn TextRecovery,
    pixels: &Pixels,
    frame: &Frame,
    options: &PdfOptions,
    report: &mut ConversionReport,
) -> Option<Vec<RecoveredItem>> {
    let png = pixels.to_png().ok()?;
    let layout = match recovery.recover_layout(&strict_ooxml_ocr::Image::png(png)) {
        Ok(Some(layout)) => layout,
        Ok(None) => return None,
        Err(error) => {
            report.record(
                "text.recovery.layout",
                Severity::Unsupported,
                format!(
                    "page {}: no layout ({error}); the page's text is read as plain lines",
                    frame.page
                ),
            );
            return None;
        }
    };
    let ink = Ink::of(pixels);
    let source = format!("{} {}", layout.model, layout.version);
    if ink.unpaper_ratio() >= PICTURE_PAGE_RATIO {
        let description = layout
            .blocks
            .iter()
            .map(|block| plain(&block.text))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        report.record(
            "text.recovery.layout",
            Severity::Recovered,
            format!(
                "page {}: kept whole as a picture ({:.0}% of it is not paper); the text model {source} \
                 read on it is the picture's description",
                frame.page,
                ink.unpaper_ratio() * 100.0
            ),
        );
        let whole = PixelBox {
            left: 0,
            top: 0,
            right: ink.width,
            bottom: ink.height,
        };
        return Some(vec![RecoveredItem::Picture(picture(
            pixels,
            whole,
            frame.scale,
            (!description.is_empty()).then_some(description),
        ))]);
    }
    Some(build_items(
        &layout.blocks,
        &ink,
        pixels,
        frame,
        options,
        report,
        &source,
    ))
}

/// The items of a page from its blocks.
fn build_items(
    blocks: &[LayoutBlock],
    ink: &Ink,
    pixels: &Pixels,
    frame: &Frame,
    options: &PdfOptions,
    report: &mut ConversionReport,
    source: &str,
) -> Vec<RecoveredItem> {
    let mut placed: Vec<(&LayoutBlock, Measured)> = Vec::new();
    let mut dropped = 0usize;
    let mut page_numbers = 0usize;
    for block in blocks {
        if block.kind == BlockKind::PageNumber {
            page_numbers += 1;
            continue;
        }
        match snap(ink, block.bbox) {
            Some(area) => {
                let lines = lines_of(ink, area);
                placed.push((block, Measured { area, lines }));
            }
            None => dropped += 1,
        }
    }
    let text_blocks = || {
        placed
            .iter()
            .filter(|(block, _)| block.kind != BlockKind::Figure)
    };
    let column = (
        text_blocks()
            .map(|(_, measured)| measured.area.left)
            .min()
            .unwrap_or(0),
        text_blocks()
            .map(|(_, measured)| measured.area.right)
            .max()
            .unwrap_or(ink.width),
    );
    let body_ratios: Vec<f64> = text_blocks()
        .filter(|(block, _)| matches!(block.kind, BlockKind::Paragraph | BlockKind::ListItem))
        .filter_map(|(_, measured)| measured.stroke_ratio())
        .collect();
    let body_ratio = (!body_ratios.is_empty()).then(|| median(body_ratios));

    // Running text on a page is one size; a single line measured a point or
    // two off it is the measure's noise (a line of capitals, a bold label),
    // not a different size. Headings keep theirs.
    let measured_sizes: Vec<Option<f64>> = placed
        .iter()
        .map(|(block, measured)| measured.size_pt(frame.scale, capitals(&block.text)))
        .collect();
    let body_size = {
        let running = |multi: bool| -> Vec<f64> {
            placed
                .iter()
                .zip(&measured_sizes)
                .filter(|((block, measured), _)| {
                    matches!(block.kind, BlockKind::Paragraph | BlockKind::ListItem)
                        && (measured.lines.len() >= 2) == multi
                })
                .filter_map(|(_, size)| *size)
                .collect()
        };
        let multi = running(true);
        let sizes = if multi.is_empty() {
            running(false)
        } else {
            multi
        };
        (!sizes.is_empty()).then(|| median(sizes))
    };
    let size_of = |index: usize, kind: BlockKind| -> Option<f64> {
        let size = measured_sizes.get(index).copied().flatten()?;
        let running = !matches!(kind, BlockKind::Title | BlockKind::Heading);
        match body_size {
            Some(body) if running && size > body * 0.75 && size < body * 1.33 => Some(body),
            _ => Some(size),
        }
    };

    // Solid areas of colour are pictures whatever the model called the blocks
    // inside them: a sticker read as five headings is still a sticker.
    let regions = ink.filled_regions();
    let region_of = |block: &LayoutBlock| {
        let x = f64::midpoint(block.bbox[0], block.bbox[2]) * ink.width as f64;
        let y = f64::midpoint(block.bbox[1], block.bbox[3]) * ink.height as f64;
        regions.iter().position(|region| {
            (region.left as f64..region.right as f64).contains(&x)
                && (region.top as f64..region.bottom as f64).contains(&y)
        })
    };
    let mut region_text: Vec<Vec<String>> = vec![Vec::new(); regions.len()];
    for (block, _) in &placed {
        if let Some(text) = region_of(block).and_then(|index| region_text.get_mut(index)) {
            let words = plain(&block.text);
            if !words.is_empty() {
                text.push(words);
            }
        }
    }
    let mut emitted = vec![false; regions.len()];

    let mut items = Vec::new();
    let mut restored = 0usize;
    let mut previous_bottom: Option<usize> = None;
    let (mut headings, mut figures, mut bold_blocks) = (0usize, 0usize, 0usize);
    let mut emit = |index: usize, items: &mut Vec<RecoveredItem>, figures: &mut usize| {
        let (Some(region), Some(done)) = (regions.get(index), emitted.get_mut(index)) else {
            return None;
        };
        if *done {
            return None;
        }
        *done = true;
        *figures += 1;
        let description = region_text
            .get(index)
            .map(|text| text.join("\n"))
            .unwrap_or_default();
        items.push(RecoveredItem::Picture(picture(
            pixels,
            *region,
            frame.scale,
            (!description.is_empty()).then_some(description),
        )));
        Some(region.bottom)
    };
    for (position, (block, measured)) in placed.iter().enumerate() {
        // Pictures no block fell into, above this block.
        for (index, region) in regions.iter().enumerate() {
            let unclaimed = region_text.get(index).is_none_or(Vec::is_empty);
            if unclaimed && region.top < measured.area.top {
                if let Some(bottom) = emit(index, &mut items, &mut figures) {
                    previous_bottom = Some(bottom);
                }
            }
        }
        if let Some(index) = region_of(block) {
            if let Some(bottom) = emit(index, &mut items, &mut figures) {
                previous_bottom = Some(bottom);
            }
            continue;
        }
        if block.kind == BlockKind::Figure {
            figures += 1;
            let description = plain(&block.text);
            items.push(RecoveredItem::Picture(picture(
                pixels,
                measured.area,
                frame.scale,
                (!description.is_empty()).then_some(description),
            )));
            previous_bottom = Some(measured.area.bottom);
            continue;
        }
        let text = match options.recovery_lexicon.as_deref() {
            Some(lexicon) => {
                let (text, count) = lexicon.restore_text(&block.text);
                restored += count;
                text
            }
            None => block.text.clone(),
        };
        let heading = match block.kind {
            BlockKind::Title => Some(block.level.unwrap_or(1)),
            BlockKind::Heading => Some(block.level.unwrap_or(2)),
            _ => None,
        };
        headings += usize::from(heading.is_some());
        let bold = heading.is_some()
            && matches!(
                (measured.stroke_ratio(), body_ratio),
                (Some(ratio), Some(body)) if ratio > body * BOLD_RATIO
            );
        bold_blocks += usize::from(bold);
        let style = Style {
            align: measured.alignment(column, block.align),
            size: size_of(position, block.kind),
            bold,
            heading,
            before: previous_bottom
                .map(|bottom| measured.area.top.saturating_sub(bottom) as f64 / frame.scale),
            indent: if matches!(
                measured.alignment(column, block.align),
                Align::Left | Align::Justify
            ) {
                measured.area.left.saturating_sub(column.0) as f64 / frame.scale
            } else {
                0.0
            },
            first_line: measured.first_line_indent() as f64 / frame.scale,
            language: options.recovery_language.clone(),
        };
        for paragraph_text in paragraph_texts(block.kind, &text) {
            items.push(RecoveredItem::Paragraph(Box::new(paragraph(
                &paragraph_text,
                &style,
            ))));
        }
        previous_bottom = Some(measured.area.bottom);
    }
    for index in 0..regions.len() {
        let _ = emit(index, &mut items, &mut figures);
    }
    report.record(
        "text.recovered",
        Severity::Recovered,
        format!(
            "page {}: {} block(s) read by model {source} from its layout ({headings} heading(s), \
             {figures} figure(s), {bold_blocks} bold); boxes checked against the page's ink{}{}",
            frame.page,
            placed.len(),
            if dropped > 0 {
                format!(", {dropped} without ink dropped")
            } else {
                String::new()
            },
            if page_numbers > 0 {
                format!(", {page_numbers} page number(s) left out")
            } else {
                String::new()
            }
        ),
    );
    if let Some(lexicon) = options.recovery_lexicon.as_deref() {
        super::policy::record_restored(report, frame.page, restored, Some(lexicon));
    }
    items
}

/// Whether a text is set in capitals: two letters or more, none lower case.
fn capitals(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|ch| ch.is_alphabetic()).collect();
    letters.len() >= 2 && !letters.iter().any(|ch| ch.is_lowercase())
}

/// How a block's paragraphs are set.
struct Style {
    align: Align,
    size: Option<f64>,
    bold: bool,
    heading: Option<u8>,
    /// Space above, in points (the gap to the block before).
    before: Option<f64>,
    /// Left indent from the text column, in points.
    indent: f64,
    /// First-line indent, in points.
    first_line: f64,
    language: Option<String>,
}

/// The paragraphs of a block's text. Running text is one paragraph (a model
/// may break it where the page did: the lines are joined, and a word split
/// with a hyphen at a line end is joined again); anything else keeps its lines.
fn paragraph_texts(kind: BlockKind, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !matches!(kind, BlockKind::Paragraph | BlockKind::Caption) {
        return lines
            .into_iter()
            .map(|line| list_marker(kind, line))
            .collect();
    }
    let mut joined = String::new();
    for line in lines {
        if joined.ends_with('-') && line.chars().next().is_some_and(char::is_lowercase) {
            joined.pop();
        } else if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(line);
    }
    if joined.is_empty() {
        Vec::new()
    } else {
        vec![joined]
    }
}

/// A list item's own marker becomes a bullet.
fn list_marker(kind: BlockKind, line: &str) -> String {
    if kind == BlockKind::ListItem {
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            return format!("\u{2022} {rest}");
        }
    }
    line.trim_start_matches('#').trim_start().to_owned()
}

/// Markdown emphasis as runs: `(text, bold, italic)`.
fn spans(text: &str) -> Vec<(String, bool, bool)> {
    let mut out: Vec<(String, bool, bool)> = Vec::new();
    let (mut bold, mut italic) = (false, false);
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '*' {
            let double = chars.peek() == Some(&'*');
            if double {
                chars.next();
            }
            if !current.is_empty() {
                out.push((std::mem::take(&mut current), bold, italic));
            }
            if double {
                bold = !bold;
            } else {
                italic = !italic;
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        out.push((current, bold, italic));
    }
    out
}

/// Markdown reduced to its words, for a picture's description.
fn plain(text: &str) -> String {
    spans(text)
        .into_iter()
        .map(|(text, _, _)| text)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn paragraph(text: &str, style: &Style) -> Paragraph {
    let twips = |points: f64| Twips(crate::to_twips(points));
    let language = style
        .language
        .as_deref()
        .map(|language| language_of(language, text));
    let runs = spans(text)
        .into_iter()
        .map(|(text, bold, italic)| {
            Inline::Run(Run {
                props: RunProperties {
                    bold: if bold || style.bold {
                        TriState::On
                    } else {
                        TriState::Absent
                    },
                    italic: if italic {
                        TriState::On
                    } else {
                        TriState::Absent
                    },
                    size: style
                        .size
                        .map(|points| HalfPoints((points * 2.0).round() as i32)),
                    language: language.as_ref().map(|tag| Language {
                        val: Some(Arc::from(tag.as_str())),
                        ..Language::default()
                    }),
                    ..RunProperties::default()
                },
                content: vec![RunContent::Text(TextNode {
                    space: if text.starts_with(' ') || text.ends_with(' ') {
                        Space::Preserve
                    } else {
                        Space::default()
                    },
                    text,
                })],
                revision: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            })
        })
        .collect();
    Paragraph {
        props: ParagraphProperties {
            style: Some(StyleId::new(RECOVERED_STYLE_ID)),
            alignment: Some(match style.align {
                Align::Left => Justification::Start,
                Align::Center => Justification::Center,
                Align::Right => Justification::End,
                Align::Justify => Justification::Both,
            }),
            outline_level: style.heading.map(|level| level.saturating_sub(1)),
            keep_next: if style.heading.is_some() {
                TriState::On
            } else {
                TriState::Absent
            },
            spacing: Some(Spacing {
                before: style.before.map(|points| twips(points.clamp(0.0, 72.0))),
                after: Some(Twips(0)),
                ..Spacing::default()
            }),
            indentation: (style.indent > 3.0 || style.first_line > 3.0).then(|| Indentation {
                start: (style.indent > 3.0).then(|| twips(style.indent)),
                first_line: (style.first_line > 3.0).then(|| twips(style.first_line)),
                ..Indentation::default()
            }),
            ..ParagraphProperties::default()
        },
        inlines: runs,
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }
}

/// A part of the page as a picture, `scale` pixels to the point.
fn picture(
    pixels: &Pixels,
    area: PixelBox,
    scale: f64,
    description: Option<String>,
) -> RecoveredPicture {
    let stride = pixels.size.width as usize * 4;
    let mut samples = Vec::with_capacity(area.width() * area.height() * 3);
    for y in area.top..area.bottom {
        let row = y * stride;
        let start = row + area.left * 4;
        let end = row + area.right * 4;
        for pixel in pixels
            .rgba
            .get(start..end)
            .unwrap_or_default()
            .chunks_exact(4)
        {
            samples.extend(pixel.iter().take(3));
        }
    }
    RecoveredPicture {
        image: Encoded::Raw {
            width: u32::try_from(area.width()).unwrap_or(u32::MAX),
            height: u32::try_from(area.height()).unwrap_or(u32::MAX),
            samples: Arc::new(samples),
            components: 3,
            alpha: None,
        },
        width: area.width() as f64 / scale,
        height: area.height() as f64 / scale,
        description,
    }
}

#[cfg(test)]
mod tests {
    use super::{lines_of, paragraph_texts, snap, spans, Ink, Measured, PixelBox};
    use strict_ooxml_ocr::{Align, BlockKind};
    use strict_ooxml_pdf::raster::{Pixels, Size};

    /// A white page with black bars where `bars` say (left, top, right, bottom).
    fn page(width: u32, height: u32, bars: &[(u32, u32, u32, u32)]) -> Pixels {
        let mut rgba = vec![255u8; (width * height * 4) as usize];
        for &(left, top, right, bottom) in bars {
            for y in top..bottom {
                for x in left..right {
                    let at = ((y * width + x) * 4) as usize;
                    rgba[at..at + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
        Pixels {
            size: Size { width, height },
            rgba,
        }
    }

    #[test]
    fn a_box_snaps_to_its_ink_and_an_empty_one_is_dropped() {
        let pixels = page(1000, 1000, &[(200, 300, 600, 320)]);
        let ink = Ink::of(&pixels);
        // The model's box is a little off; the ink decides.
        let area = snap(&ink, [0.19, 0.295, 0.62, 0.33]).expect("ink");
        assert_eq!(
            (area.left, area.top, area.right, area.bottom),
            (200, 300, 600, 320)
        );
        assert!(snap(&ink, [0.7, 0.7, 0.9, 0.9]).is_none());
    }

    #[test]
    fn lines_give_size_and_alignment() {
        // Three justified lines and a short last one, 24 px apart at scale 2:
        // a 12 pt pitch, 10 pt type.
        let bars = [
            (100, 100, 900, 116),
            (100, 124, 900, 140),
            (100, 148, 900, 164),
            (100, 172, 500, 188),
        ];
        let pixels = page(1000, 400, &bars);
        let ink = Ink::of(&pixels);
        let area = PixelBox {
            left: 100,
            top: 100,
            right: 900,
            bottom: 188,
        };
        let measured = Measured {
            area,
            lines: lines_of(&ink, area),
        };
        assert_eq!(measured.lines.len(), 4);
        assert_eq!(measured.size_pt(2.0, false), Some(10.0));
        assert_eq!(measured.alignment((100, 900), None), Align::Justify);
        // Centred lines of different widths.
        let pixels = page(1000, 400, &[(300, 100, 700, 116), (400, 124, 600, 140)]);
        let ink = Ink::of(&pixels);
        let area = PixelBox {
            left: 300,
            top: 100,
            right: 700,
            bottom: 140,
        };
        let measured = Measured {
            area,
            lines: lines_of(&ink, area),
        };
        assert_eq!(measured.alignment((100, 900), None), Align::Center);
        // One line: its gaps to the text column.
        for (left, right, expected) in [
            (150, 850, Align::Center),
            (100, 600, Align::Left),
            (400, 900, Align::Right),
        ] {
            let pixels = page(1000, 400, &[(left, 100, right, 116)]);
            let ink = Ink::of(&pixels);
            let area = PixelBox {
                left: left as usize,
                top: 100,
                right: right as usize,
                bottom: 116,
            };
            let measured = Measured {
                area,
                lines: lines_of(&ink, area),
            };
            assert_eq!(
                measured.alignment((100, 900), None),
                expected,
                "{left}-{right}"
            );
        }
    }

    #[test]
    fn markdown_becomes_runs_and_lines_become_paragraphs() {
        assert_eq!(
            spans("dvanaest **lekcija**, *vežbanke*"),
            vec![
                ("dvanaest ".to_owned(), false, false),
                ("lekcija".to_owned(), true, false),
                (", ".to_owned(), false, false),
                ("vežbanke".to_owned(), false, true),
            ]
        );
        assert_eq!(
            paragraph_texts(BlockKind::Paragraph, "Ovo je pre-\nrađeno i\ndopunjeno"),
            vec!["Ovo je prerađeno i dopunjeno".to_owned()]
        );
        assert_eq!(
            paragraph_texts(BlockKind::Title, "# Учимо српски\n1"),
            vec!["Учимо српски".to_owned(), "1".to_owned()]
        );
        assert_eq!(
            paragraph_texts(BlockKind::ListItem, "- prvo"),
            vec!["\u{2022} prvo".to_owned()]
        );
    }

    /// A white page with bars of one colour.
    fn coloured(width: u32, height: u32, colour: [u8; 3], bars: &[(u32, u32, u32, u32)]) -> Pixels {
        let mut pixels = page(width, height, &[]);
        for &(left, top, right, bottom) in bars {
            for y in top..bottom {
                for x in left..right {
                    let at = ((y * width + x) * 4) as usize;
                    pixels.rgba[at..at + 3].copy_from_slice(&colour);
                }
            }
        }
        pixels
    }

    #[test]
    fn light_coloured_type_is_ink() {
        // The light-blue heading of a scanned book: brighter than any grey
        // threshold, plainly type.
        let pixels = coloured(1000, 1000, [140, 195, 240], &[(200, 100, 500, 130)]);
        let area = snap(&Ink::of(&pixels), [0.2, 0.1, 0.5, 0.13]).expect("ink");
        assert_eq!((area.left, area.top), (200, 100));
    }

    #[test]
    fn a_solid_area_is_a_picture_and_a_scan_edge_is_not() {
        let pixels = coloured(
            800,
            800,
            [120, 60, 170],
            &[(100, 400, 360, 760), (780, 0, 800, 800)],
        );
        let regions = Ink::of(&pixels).filled_regions();
        assert_eq!(regions.len(), 1, "{regions:?}");
        let region = regions[0];
        assert!(region.left <= 112 && region.right >= 352, "{region:?}");
        assert!(region.top <= 400 && region.bottom >= 752, "{region:?}");
    }

    #[test]
    fn a_coloured_page_is_mostly_not_paper() {
        let mut pixels = page(100, 100, &[]);
        for pixel in pixels.rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[240, 230, 40, 255]);
        }
        assert!(Ink::of(&pixels).unpaper_ratio() > 0.9);
        assert!(Ink::of(&page(100, 100, &[(10, 10, 20, 20)])).unpaper_ratio() < 0.05);
    }
}
