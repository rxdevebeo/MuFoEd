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
//! # What is not reconstructed
//!
//! Tables. A grid read from ruling lines is a reconstruction of something the
//! PDF never states, and doing it badly produces a document that *looks* like a
//! table and is not one. This increment detects a grid and says so
//! ([`Severity::Unsupported`]) instead of guessing; the cells stay in the
//! document as paragraphs, which is honest and reversible.

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
use crate::report::{ConversionReport, Severity};
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

/// Builds the semantic document.
pub(crate) fn build(
    pages: &[PdfPage],
    options: &PdfOptions,
    report: &mut ConversionReport,
) -> (Document, Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>) {
    let mut media = MediaCollector::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut sections: Vec<crate::Section> = Vec::new();

    for (index, page) in pages.iter().enumerate() {
        let lines = strict_ooxml_pdf::text::lines(page.items());
        report.lines += lines.len();
        let body = body_size(&lines);
        let pitch = body_pitch(&lines);
        report_grid(page, index, report);

        let mut paragraph: Option<Paragraph> = None;
        let mut previous: Option<&GlyphLine> = None;
        for line in &lines {
            let starts_new = previous.map_or(true, |previous| {
                let gap = line.baseline - previous.baseline;
                let moved = (line.x - previous.x).abs();
                gap > pitch * options.paragraphs.paragraph_gap_ratio
                    || moved > body * options.paragraphs.indent_ratio
                    || !same_format(previous, line)
            });
            if starts_new || paragraph.is_none() {
                if let Some(finished) = paragraph.take() {
                    blocks.push(Block::Paragraph(finished));
                }
                let heading = heading_level(line, body, &options.paragraphs);
                if let Some(level) = heading {
                    report.record(
                        "heading.inferred",
                        Severity::Inferred,
                        format!(
                            "page {}: {:.1} pt text on a {:.1} pt body became heading {}",
                            index + 1,
                            line.size,
                            body,
                            level + 1
                        ),
                    );
                }
                paragraph = Some(new_paragraph(line, heading));
            } else if let Some(paragraph) = paragraph.as_mut() {
                set_line_gap(paragraph, line, body);
            }
            if let Some(paragraph) = paragraph.as_mut() {
                push_runs(paragraph, line);
            }
            previous = Some(line);
        }
        if let Some(finished) = paragraph.take() {
            blocks.push(Block::Paragraph(finished));
        }

        if options.embed_images {
            blocks.extend(images_of(page, &mut media, report, options));
        }
        sections.push(crate::Section {
            number: index + 1,
            properties: section_for(page, false),
        });
    }

    let mut document = crate::empty_document();
    document.body = strict_ooxml_wml::model::document::Body { blocks };
    document.sections = sections
        .into_iter()
        .map(|section| strict_ooxml_wml::model::props::Section {
            properties: section.properties,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        })
        .collect();
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

/// Records that a page carries a ruling grid, which this increment does not
/// reconstruct.
///
/// Reporting it is worth more than guessing: a table guessed from its lines
/// looks like a table in the output and is not one, and the caller cannot tell
/// the difference without reading this report.
fn report_grid(page: &PdfPage, index: usize, report: &mut ConversionReport) {
    let rules = page
        .items()
        .iter()
        .filter(|item| {
            let strict_ooxml_pdf::Item::Vector(vector) = item else {
                return false;
            };
            // A ruling line is a filled box that is long and thin.
            let bounds = strict_ooxml_pdf::content::Matrix::IDENTITY;
            let mut width = 0.0f64;
            let mut height = 0.0f64;
            for subpath in &vector.subpaths {
                for (x, y) in &subpath.points {
                    let (x, y) = bounds.apply(*x, *y);
                    width = width.max(x);
                    height = height.max(y);
                }
            }
            let thin = width.min(height) < 2.0;
            thin && width.max(height) > 20.0
        })
        .count();
    if rules >= 4 {
        report.record(
            "table.detected",
            Severity::Unsupported,
            format!(
                "page {} carries {rules} ruling lines that look like a table grid; the \\
                 cells were left as paragraphs",
                index + 1
            ),
        );
    }
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
    sizes.sort_by(|left, right| right.1.cmp(&left.1));
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
fn new_paragraph(line: &GlyphLine, heading: Option<u8>) -> Paragraph {
    Paragraph {
        props: ParagraphProperties {
            // An indent is the only paragraph-level geometry a PDF states, and
            // keeping it is what stops a converted page from re-flowing.
            indentation: Some(Indentation {
                start: Some(Twips(to_twips(line.x))),
                ..Indentation::default()
            }),
            outline_level: heading,
            keep_next: heading.is_some(),
            ..ParagraphProperties::default()
        },
        inlines: Vec::new(),
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
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
    let mut current: Option<(RunProperties, String)> = None;
    for glyph in &line.glyphs {
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
                doc_pr: Some(strict_ooxml_wml::model::drawing::DocPr {
                    id: None,
                    name: Some(Arc::from("Picture")),
                    // The field a screen reader reads, and the only place a
                    // model's words enter the document.
                    descr: description.clone().map(Arc::from),
                }),
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
