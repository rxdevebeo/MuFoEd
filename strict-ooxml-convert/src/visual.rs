//! The visual mode: reconstruct where everything *was*.
//!
//! The semantic mode gives an editable document whose pagination is a
//! reconstruction. This mode does the opposite: it gives a document that
//! *paginates* like the original at the cost of a convention that only a machine
//! reads well. One paragraph per line, at the line's indent, with the leading
//! the PDF had.
//!
//! # What is reproduced, and how exactly
//!
//! | On the page | In the document |
//! |---|---|
//! | page box | `w:pgSz`, in twips — exact, the PDF states it |
//! | baseline of a line | the paragraph's position, via the leading and the indent |
//! | left edge of a line | `w:ind/@w:start` — exact to a twip |
//! | font, size, colour | `w:rPr` — exact |
//! | the gap to the next line | `w:spacing/@w:line`, `w:lineRule="exact"` |
//!
//! The leading is `exact` because an automatic leading is the layout engine's
//! choice, and the whole point of this mode is that the engine's choice is not
//! the producer's. A line that is 13 pt tall with 11 pt of type keeps that,
//! rather than being re-spaced to whatever the engine thinks looks better.
//!
//! What this mode does *not* do: it does not reproduce columns, absolute
//! positioning, or vertical alignment. A page whose text is placed at an x that
//! the indent chain cannot express is recorded, not fudged.

use strict_ooxml_pdf::PdfPage;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::props::ParagraphProperties;
use strict_ooxml_wml::model::values::Spacing;
use strict_ooxml_wml::model::values::{Indentation, LineSpacingRule, Rsids, Twips};
use strict_ooxml_wml::model::Document;

use crate::geometry::{section_for, DEFAULT_MARGIN_PT};
use crate::media::MediaCollector;
use crate::recover::Recovered;
use crate::report::ConversionReport;
use crate::sections::SectionAccumulator;
use crate::semantic::{body_pitch_pub, body_size_pub, images_pub, push_runs_public};

use crate::{to_twips, PdfOptions, TWIPS_PER_POINT};

/// Fraction of an exact line box that the renderer uses as the baseline.
///
/// `strict-ooxml-render-svg` places an exact line's baseline at 80% of the
/// line box. The first paragraph's `w:spacing/@w:before` makes up the rest of
/// the distance from the top margin to the PDF baseline.
const EXACT_LINE_ASCENT: f64 = 0.8;

/// Builds the visual document.
pub(crate) fn build(
    pages: &[PdfPage],
    recovered: &Recovered,
    options: &PdfOptions,
    report: &mut ConversionReport,
) -> (Document, Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>) {
    let mut media = MediaCollector::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut sections = SectionAccumulator::new();

    for (index, page) in pages.iter().enumerate() {
        // Visual keeps a break on every page, including neighbours of the same size.
        sections.begin_page(&mut blocks, section_for(page, true), true);
        let lines = strict_ooxml_pdf::text::lines(page.items());
        report.lines += lines.len();
        let body = body_size_pub(&lines);
        let pitch = body_pitch_pub(&lines);

        let mut previous_baseline: Option<f64> = None;
        for line in &lines {
            // The leading of this line is the distance from the previous
            // baseline, which is the number the PDF actually has.
            let leading = previous_baseline.map_or(line.size, |previous| line.baseline - previous);
            // The section already insets the page by the visual margin. The
            // indent and the first baseline are measured from that edge, so a
            // 72 pt margin is not added again on top of a 72 pt coordinate.
            let indent = line.x - DEFAULT_MARGIN_PT;
            let before = if previous_baseline.is_none() {
                let ascent = leading * EXACT_LINE_ASCENT;
                line.baseline - DEFAULT_MARGIN_PT - ascent
            } else {
                0.0
            };
            let mut paragraph = Paragraph {
                props: ParagraphProperties {
                    indentation: Some(Indentation {
                        start: Some(Twips(to_twips(indent))),
                        ..Indentation::default()
                    }),
                    spacing: Some(Spacing {
                        before: if before.abs() > 0.01 {
                            Some(Twips(to_twips(before)))
                        } else {
                            None
                        },
                        line: Some(Twips(to_twips(leading))),
                        line_rule: Some(LineSpacingRule::Exact),
                        ..Spacing::default()
                    }),
                    ..ParagraphProperties::default()
                },
                inlines: Vec::new(),
                rsids: Rsids::default(),
                para_id: None,
                text_id: None,
                revision: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            };
            push_runs_public(&mut paragraph, line);
            blocks.push(Block::Paragraph(paragraph));
            previous_baseline = Some(line.baseline);
        }
        let _ = (index, body, pitch, TWIPS_PER_POINT);

        if options.embed_images {
            blocks.extend(images_pub(page, &mut media, report, options));
        }
        blocks.extend(crate::vectors::blocks_for(page, report));
        // A recovered block has the indent of the region it came from and
        // nothing else: a model returns words, not baselines, so a fabricated
        // leading would be a lie about where the lines sat.
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
    report.paragraphs = document
        .body
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::Paragraph(_)))
        .count();
    report.images = media.len();
    (document, media.into_parts())
}
