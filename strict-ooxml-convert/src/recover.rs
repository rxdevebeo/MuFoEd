//! Reading a page that has no text, by showing it to a model
//! (`STAGE-8-TASK.md` §6, O1 and O11).
//!
//! # What this is for
//!
//! A scanned page has no glyphs. The text of such a page is a picture of text,
//! and the only way to get words out of it is something that can read pictures —
//! a vision model. Every step of the pipeline is a decision, and they are:
//!
//! 1. **Which pages.** A page whose text layer is [`TextLayer::Readable`] is not
//!    touched: the glyphs are the truth and a model's reading of them would be
//!    strictly worse — this is the ordinary case for a technical manual, whose
//!    pages are text *and* diagrams, and those diagrams are a figure's business
//!    (`FigureClassifier`), not its text's. A page with no text and **no ink at
//!    all** is blank, and asking a model about a blank page spends a call to be
//!    told nothing is there. So the trigger is: no readable text, and ink.
//! 2. **Which part of the page** — and this is the part that is not obvious. There
//!    are two strategies, and the page picks:
//!
//!    - **The regions.** A page whose ink is *pictures* has its pictures offered
//!      to the model one at a time, largest first. A scan is one big picture and
//!      costs one call; a magazine page with three photographs costs three, and
//!      the ones that answer «no text» are told apart in the report from the ones
//!      that answered. The region is rasterized, not the page, so a 400 × 500 pt
//!      scan costs 800 × 1000 pixels instead of a full A4 sheet.
//!    - **The page.** A page with no readable text and **no picture** — its ink is
//!      vector paths, which is what a document rasterized to outlines looks like —
//!      has no region to name, so the page itself is the region.
//!
//!    Both are recorded, with the region named, so a caller can tell a recovered
//!    paragraph from a recovered *picture* without parsing prose.
//! 3. **What is too small to be text.** A 12 pt icon is not a line of type, and
//!    asking a model about every logo on a page is how a page of text turns into
//!    thirty calls. Regions smaller than
//!    [`PdfOptions::recovery_min_region_pt`] are not sent, and a page left with no
//!    region at all falls back to the page.
//! 4. **What comes back.** A model's answer is text with no coordinates, so it
//!    becomes plain paragraphs, split on the line breaks the model gave. The
//!    paragraph boundaries are therefore **the model's**, which is a fact about the
//!    document and not about the page.
//! 5. **How it is marked.** Twice. In the document: every recovered paragraph
//!    carries the `Recovered` style, so a person opening the file can see which
//!    text is not from the PDF. In the report: one `Severity::Recovered` entry per
//!    region, naming the model and the region, so a caller can filter on it.
//!
//! # What it does not do
//!
//! It does not reconstruct layout. A model's answer carries no baseline, no
//! leading and no size; the only geometry a recovered paragraph gets is the
//! **left edge of the region it came from**, which is a real number from the PDF
//! and the reason the semantic mode can indent it where the picture was. The
//! visual mode gets a block with that indent and nothing else, which is stated
//! rather than faked: a fabricated leading would be a lie about where the lines
//! sat.

use std::collections::BTreeMap;

#[cfg(feature = "raster")]
use strict_ooxml_pdf::PdfDocument;
use strict_ooxml_pdf::{PdfPage, TextLayer};
use strict_ooxml_wml::model::block::Paragraph;

use crate::report::{ConversionReport, Severity};
#[cfg(feature = "raster")]
use crate::PdfOptions;

#[cfg(feature = "raster")]
pub(crate) mod layout;
/// The style id of a recovered paragraph.
///
/// The *name* ([`crate::RECOVERY_STYLE_NAME`]) is what a reader of the file
/// sees; the id is what the paragraph refers to, and it has no spaces because a
/// style id is a reference and not a label.
#[cfg(any(feature = "raster", feature = "djvu-image"))]
pub(crate) mod policy;
#[cfg(feature = "raster")]
pub(crate) use policy::declare_style;

/// How a region was chosen, which is a fact about the document a caller may want
/// to filter on.
#[cfg(feature = "raster")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// One picture of the page, and only that.
    Picture,
    /// The whole page, because the page's ink is not a picture.
    Page,
}

#[cfg(feature = "raster")]
impl Origin {
    /// The word a report line uses.
    fn name(self) -> &'static str {
        match self {
            Self::Picture => "a picture of the page",
            Self::Page => "the whole page (its ink is not a picture)",
        }
    }
}

/// One model answer: what a model read out of one region.
///
/// The region itself is not kept here — its left edge is already the recovered
/// paragraphs' indent, and the report names the region, its size and its position,
/// which is where a caller reads them from.
///
/// **Not** behind `feature = "raster"`, unlike everything else in this file:
/// [`Recovered`] carries these in every build, so a build without the feature
/// would not compile — and the gates build `--all-features`, so nothing would have
/// said so. The type costs nothing when it is always empty.
#[derive(Clone, Debug)]
pub(crate) struct RecoveredBlock {
    /// What the model said - one paragraph per line it gave, or, from a
    /// page's layout, its paragraphs and pictures in reading order.
    pub items: Vec<RecoveredItem>,
}

/// One thing a recovered block places.
#[derive(Clone, Debug)]
pub(crate) enum RecoveredItem {
    /// A paragraph of text a model read.
    Paragraph(Box<Paragraph>),
    /// A part of the page cut out as a picture.
    Picture(RecoveredPicture),
    /// A table a model read.
    Table(Box<strict_ooxml_wml::model::block::Table>),
}

/// A picture cut out of a scanned page.
#[derive(Clone, Debug)]
pub(crate) struct RecoveredPicture {
    /// The pixels.
    pub image: strict_ooxml_pdf::Encoded,
    /// Its size on the page, in points.
    pub width: f64,
    /// Its height on the page, in points.
    pub height: f64,
    /// What a model read inside it, for `wp:docPr/@descr`.
    pub description: Option<String>,
}

/// The paragraph list of every page a model read, by 0-based page index.
///
/// Exists in every build, because a document built without the `raster` feature
/// must place the pages it *does* have the same way a document built with it
/// does — the only difference is whether the map is full.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recovered {
    /// `page index -> blocks`, in the order the regions were sent.
    blocks: BTreeMap<usize, Vec<RecoveredBlock>>,
}

impl Recovered {
    /// The blocks recovered from one page, if any.
    pub(crate) fn page(&self, index: usize) -> &[RecoveredBlock] {
        self.blocks.get(&index).map_or(&[], Vec::as_slice)
    }

    /// Whether anything was recovered at all.
    #[cfg(feature = "raster")]
    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Asks a model about the pages that have no text, and records what happened.
///
/// A failure is never a failure of the conversion: the page keeps whatever the
/// PDF gave (nothing), the reason goes into the report, and the rest of the
/// document is built as usual. That is the order's `O5`, and it is also the only
/// behaviour that makes a model optional in a library.
#[cfg(feature = "raster")]
pub(crate) fn recover_pages(
    pdf: &mut PdfDocument,
    pages: &[PdfPage],
    options: &PdfOptions,
    report: &mut ConversionReport,
) -> Recovered {
    let mut out = Recovered::default();
    let Some(recovery) = options.text_recovery.clone() else {
        record_absent(pages, report);
        return out;
    };
    let rasterizer = match pdf.rasterizer() {
        Ok(rasterizer) => rasterizer,
        Err(error) => {
            report.record(
                "text.recovery",
                Severity::Lost,
                format!("no page could be read by a model: {error}"),
            );
            return out;
        }
    };
    for (index, page) in pages.iter().enumerate() {
        let readable = page.text_layer() == TextLayer::Readable;
        if !readable && !page.has_ink() {
            // No text and nothing drawn: a blank page, which has nothing to read —
            // not a loss, and not a call.
            continue;
        }
        // Which regions, and which call, is the difference between a page with no
        // text and a page whose text is complete but hides some inside a picture.
        let (regions, ask) = if readable {
            let regions = mixed_regions_of(&rasterizer, page, options, report, index);
            let ask = if regions.is_empty() {
                Ask::Page
            } else {
                Ask::Region
            };
            (regions, ask)
        } else {
            (regions_of(page, options), Ask::Page)
        };
        for region in regions {
            let raster_result = rasterize(&rasterizer, page, &region, options)
                .and_then(|(pixels, scale)| pixels.to_png().map(|png| (pixels, scale, png)));
            let (pixels, scale, request) = match raster_result {
                Ok(raster) => raster,
                Err(error) => {
                    report.record(
                        "text.recovery",
                        Severity::Lost,
                        format!("page {} could not be rasterized: {error}", page.number),
                    );
                    continue;
                }
            };
            // A scan read block by block keeps its headings, alignment, sizes
            // and pictures; a model that gives no layout gets the plain ask.
            if ask == Ask::Page && options.recovery_layout {
                // The same region at twice the scale, for the weight of single
                // words: at the model's 144 dpi a stroke is 2-3 pixels.
                let detail = rasterizer
                    .region_pixels(
                        page.number,
                        strict_ooxml_pdf::raster::Region {
                            x: region.left,
                            y: region.top,
                            width: region.width,
                            height: region.height,
                        },
                        &strict_ooxml_pdf::raster::RasterOptions {
                            scale: scale * 2.0,
                            max_pixels: options.recovery_max_pixels.saturating_mul(4),
                        },
                    )
                    .ok();
                let frame = layout::Frame {
                    scale,
                    page: page.number,
                    detail,
                };
                if let Some(items) = layout::rebuild(&*recovery, &pixels, &frame, options, report) {
                    report.recovered_pages += 1;
                    report.recovered_paragraphs += items
                        .iter()
                        .filter(|item| matches!(item, RecoveredItem::Paragraph(_)))
                        .count();
                    out.blocks
                        .entry(index)
                        .or_default()
                        .push(RecoveredBlock { items });
                    continue;
                }
            }
            let image = strict_ooxml_ocr::Image::png(request);
            let answer = match ask {
                Ask::Page => recovery.recover_page(&image),
                Ask::Region => recovery.recover_region(&image),
            };
            match answer {
                Ok(Some(answer)) => {
                    let (paragraphs, restored) = policy::recovered_paragraphs(
                        &answer.text,
                        region.left,
                        options.recovery_lexicon.as_deref(),
                        options.recovery_language.as_deref(),
                    );
                    policy::record_restored(
                        report,
                        page.number,
                        restored,
                        options.recovery_lexicon.as_deref(),
                    );
                    report.recovered_pages += 1;
                    report.recovered_paragraphs += paragraphs.len();
                    report.record(
                        "text.recovered",
                        Severity::Recovered,
                        format!(
                            "page {}: {} paragraph(s) read by model {} {} from {}, {:.0}x{:.0} pt at \
                             ({:.0},{:.0}); the paragraph breaks are the model's",
                            page.number,
                            paragraphs.len(),
                            answer.model,
                            answer.version,
                            region.origin.name(),
                            region.width,
                            region.height,
                            region.left,
                            region.top,
                        ),
                    );
                    out.blocks.entry(index).or_default().push(RecoveredBlock {
                        items: paragraphs
                            .into_iter()
                            .map(|paragraph| RecoveredItem::Paragraph(Box::new(paragraph)))
                            .collect(),
                    });
                }
                Ok(None) => report.record(
                    "text.recovery",
                    Severity::Lost,
                    format!(
                        "page {}: the model found no text in {}, which has ink on the page",
                        page.number,
                        region.origin.name()
                    ),
                ),
                Err(error) => report.record(
                    "text.recovery",
                    Severity::Lost,
                    format!("page {}: {error}", page.number),
                ),
            }
        }
    }
    out
}

/// Which of the two methods of [`TextRecovery`] a region is sent to.
///
/// A crop is not a page, and the prompt says so (`REGION_PROMPT`): a model told it
/// is reading a page accounts for the page, and the words it invents for the parts
/// it cannot see go into the document as if the page had said them.
#[cfg(feature = "raster")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Page,
    Region,
}

/// One region to send to a model.
#[cfg(feature = "raster")]
struct Region {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    origin: Origin,
}

/// Where a picture lies on the page as it is displayed - the page the
/// rasterizer draws and the crop is cut from.
///
/// A placed image keeps PDF's frame: unrotated, with `y` its lower edge measured
/// up from the bottom. A scanned book stored as landscape pages turned by
/// `/Rotate 90` was cropped to its unrotated 842x595 box on a 595x842 page, and
/// the bottom third of every page never reached the model.
#[cfg(feature = "raster")]
fn picture_region(page: &PdfPage, image: &strict_ooxml_pdf::PlacedImage) -> Region {
    let (left, top, width, height) = displayed_rect(
        page.geometry,
        image.x,
        page.geometry.height - (image.y + image.height),
        image.width,
        image.height,
    );
    Region {
        left,
        top,
        width,
        height,
        origin: Origin::Picture,
    }
}

/// A rectangle given from the unrotated page's top-left corner, as it lands
/// once `/Rotate` (clockwise) turns the page: `(left, top, width, height)`.
#[cfg(feature = "raster")]
fn displayed_rect(
    geometry: strict_ooxml_pdf::PageGeometry,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64) {
    let (page_width, page_height) = (geometry.width, geometry.height);
    match geometry.rotation {
        90 => (page_height - (top + height), left, height, width),
        180 => (
            page_width - (left + width),
            page_height - (top + height),
            width,
            height,
        ),
        270 => (top, page_width - (left + width), height, width),
        _ => (left, top, width, height),
    }
}

/// The regions of a page that a model should be asked about, in the order they
/// will be sent.
///
/// The page's pictures, **largest first**, and the whole page when it has none.
/// Largest first because a scan is one big picture and a magazine page is several
/// small ones: a caller who has a budget for one call per page wants the one that
/// is most likely to be the text.
#[cfg(feature = "raster")]
fn regions_of(page: &PdfPage, options: &PdfOptions) -> Vec<Region> {
    let (page_width, page_height) = page.geometry.displayed();
    let mut pictures: Vec<Region> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            strict_ooxml_pdf::Item::Image(image) => Some(picture_region(page, image)),
            _ => None,
        })
        .filter(|region| {
            let long_side = region.width.max(region.height);
            // A picture smaller than a few lines of type is a logo or a bullet,
            // and a model asked about thirty of those has told the caller nothing
            // it did not already know from the pictures being there.
            long_side >= options.recovery_min_region_pt
                && region.width.is_finite()
                && region.height.is_finite()
                && region.width > 0.0
                && region.height > 0.0
        })
        .collect();
    // A page is a region, not a choice: the same picture twice is the same call
    // twice, so duplicates are dropped before the budget is spent.
    pictures.sort_by(|left, right| {
        let left_area = left.width * left.height;
        let right_area = right.width * right.height;
        right_area
            .partial_cmp(&left_area)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                left.top
                    .partial_cmp(&right.top)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    pictures.dedup_by(|left, right| {
        (left.left - right.left).abs() < 0.5
            && (left.top - right.top).abs() < 0.5
            && (left.width - right.width).abs() < 0.5
            && (left.height - right.height).abs() < 0.5
    });
    pictures.truncate(options.recovery_max_regions);
    if pictures.is_empty() {
        // No picture to name: the page's ink is vector paths, so the page is the
        // region. A page of nothing at all never gets here.
        pictures.push(Region {
            left: 0.0,
            top: 0.0,
            width: page_width,
            height: page_height,
            origin: Origin::Page,
        });
    }
    pictures
}

/// The pictures of a page whose **text we already have** that might still hide
/// text of their own (`Q-28`).
///
/// The trigger is a second one, and it is deliberately much stricter than the
/// scan rule. A page with no text is *all* candidate; a page whose text is
/// complete is candidate only where a **raster** picture is large enough to hold
/// lines of type, because a picture's text is invisible to a reader however good
/// the reader is. Everything else is left alone:
///
/// - **vector art** is not a candidate at all — a chart drawn with paths has its
///   labels as real glyphs, and the reader has them;
/// - a picture below the area floor is a logo, a bullet, a rule or a diagram.
///
/// The floor is a **share of the page**, not points, because the question is
/// whether the picture is a picture *of something* rather than a mark on the page.
/// Measured over the foreign corpus (`STAGE-8-OPEN.md` `Q-28`): of 106 526 raster
/// pictures on 5 142 readable pages, **105 581 are under a tenth of the page** —
/// they are the STM32 manual's diagrams, tiled small — and only **418** reach a
/// quarter of the page. Those 418 are the population this rule can ever offer, on
/// 46 documents, and a caller who wants fewer should raise the floor or set
/// [`PdfOptions::figure_classifier`], whose verdict about a picture is better
/// evidence than its size.
///
/// When a classifier is configured it gets the **last word**: a picture it names a
/// `diagram`, `table`, `formula` or `logo` is a figure, and this project's answer
/// for a figure is a `descr` (`FigureClassifier`), not paragraphs of text guessed
/// out of its pixels.
#[cfg(feature = "raster")]
fn mixed_regions_of(
    rasterizer: &strict_ooxml_pdf::raster::Rasterizer,
    page: &PdfPage,
    options: &PdfOptions,
    report: &mut ConversionReport,
    index: usize,
) -> Vec<Region> {
    let (page_width, page_height) = page.geometry.displayed();
    let page_area = page_width * page_height;
    if !page_area.is_finite() || page_area <= 0.0 {
        return Vec::new();
    }
    let mut regions: Vec<Region> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            strict_ooxml_pdf::Item::Image(image) if image.image.is_some() => {
                Some(picture_region(page, image))
            }
            _ => None,
        })
        .filter(|region| {
            region.width.is_finite()
                && region.height.is_finite()
                && region.width > 0.0
                && region.height > 0.0
                && region.width * region.height / page_area >= options.recovery_mixed_min_area_ratio
        })
        .collect();
    regions.sort_by(|left, right| {
        let left_area = left.width * left.height;
        let right_area = right.width * right.height;
        right_area
            .partial_cmp(&left_area)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                left.top
                    .partial_cmp(&right.top)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    regions.truncate(options.recovery_max_regions);
    if let Some(classifier) = options.figure_classifier.clone() {
        regions.retain(|region| {
            let png = rasterize(rasterizer, page, region, options)
                .and_then(|(pixels, _)| pixels.to_png());
            let image = match png {
                Ok(png) => strict_ooxml_ocr::Image::png(png),
                Err(error) => {
                    report.record(
                        "text.recovery",
                        Severity::Lost,
                        format!(
                            "page {}: a picture that could not be rasterized ({error}) was not \
                             classified and not asked about",
                            index + 1
                        ),
                    );
                    return false;
                }
            };
            match classifier.describe(&image, "") {
                Ok(answer) => {
                    let kind = strict_ooxml_ocr::FigureKind::parse(&answer.text);
                    let wanted = !matches!(
                        kind,
                        strict_ooxml_ocr::FigureKind::Diagram
                            | strict_ooxml_ocr::FigureKind::Table
                            | strict_ooxml_ocr::FigureKind::Formula
                            | strict_ooxml_ocr::FigureKind::Logo
                    );
                    report.record(
                        "text.recovery.figure",
                        Severity::Inferred,
                        format!(
                            "page {}: a picture covering {:.0}% of the page was classified as {} \
                             and {}",
                            index + 1,
                            100.0 * region.width * region.height / page_area,
                            kind,
                            if wanted {
                                "its text is worth asking a model for"
                            } else {
                                "a figure is described, not transcribed"
                            }
                        ),
                    );
                    wanted
                }
                Err(error) => {
                    report.record(
                        "text.recovery.figure",
                        Severity::Inferred,
                        format!(
                            "page {}: a picture could not be classified ({error}), so the size \
                             floor is the only evidence and it is asked about",
                            index + 1
                        ),
                    );
                    true
                }
            }
        });
    }
    regions
}

/// Records every page whose text this converter cannot produce.
///
/// This is the half of the feature that does **not** need a rasterizer, and it is
/// the half that matters most: a scanned page converted to an empty page is a
/// silent loss today, and no caller should have to read a report to find out
/// that half their document is missing.
pub(crate) fn record_absent(pages: &[PdfPage], report: &mut ConversionReport) {
    for (index, page) in pages.iter().enumerate() {
        if page.text_layer() == TextLayer::Readable {
            continue;
        }
        if !page.has_ink() {
            continue;
        }
        let why = match page.text_layer() {
            TextLayer::Unreadable => "it draws text and none of it could be mapped to characters",
            _ => "it draws no text at all, so it is a picture of text",
        };
        report.record(
            "text.absent",
            Severity::Lost,
            format!(
                "page {} has no text this converter can read, because {why}, and no model was \
                 configured to read a picture of it",
                index + 1
            ),
        );
    }
}

/// Rasterizes one region of one page.
///
/// The scale is a **negotiation between three numbers**, and the order matters:
///
/// - `recovery_min_region_px` says what the model needs to read a line of type,
///   which for a 40 pt column means a scale no whole page could ever afford;
/// - `recovery_max_pixels` says what one call may allocate, and it is spent on the
///   **page**, because the rasterizer draws the page and crops the region out of
///   it — so a page of 200 × 100 pt at the scale a 40 pt region would like is
///   eighteen megapixels, and the honest answer is the largest scale that fits;
/// - `recovery_scale` is the floor, so a page that fits anyway is never rasterized
///   below the resolution a reader can see.
///
/// When even the floor does not fit — a poster, like the 1533 × 11 985 pt page in
/// the corpus — the call is refused by the rasterizer's own budget and the report
/// says so, which is the right outcome: the picture is not worth a gigabyte, and
/// the page is not a scan.
#[cfg(feature = "raster")]
fn rasterize(
    rasterizer: &strict_ooxml_pdf::raster::Rasterizer,
    page: &PdfPage,
    region: &Region,
    options: &PdfOptions,
) -> strict_ooxml_pdf::error::Result<(strict_ooxml_pdf::raster::Pixels, f64)> {
    let (page_width, page_height) = page.geometry.displayed();
    let page_area = (page_width * page_height).max(1.0);
    // `u64 as f64` loses precision above 2^53, and a budget that large is not a
    // budget but a mistake; the cast is deliberate and the clamp below is what
    // makes a mistake harmless.
    #[allow(clippy::cast_precision_loss)]
    let budget = options.recovery_max_pixels as f64;
    let affordable = (budget / page_area).sqrt();
    let long_side = region.width.max(region.height).max(1.0);
    let wanted = options.recovery_min_region_px / long_side;
    let scale = wanted.min(affordable).max(options.recovery_scale);
    rasterizer
        .region_pixels(
            page.number,
            strict_ooxml_pdf::raster::Region {
                x: region.left,
                y: region.top,
                width: region.width,
                height: region.height,
            },
            &strict_ooxml_pdf::raster::RasterOptions {
                scale,
                max_pixels: options.recovery_max_pixels,
            },
        )
        .map(|pixels| (pixels, scale))
}

#[cfg(all(test, feature = "raster"))]
mod tests {
    use super::displayed_rect;
    use super::policy::paragraphs_of;

    #[test]
    fn recovered_words_are_repaired_and_tagged_by_script() {
        use strict_ooxml_wml::model::inline::Inline;
        let lexicon = strict_ooxml_ocr::Lexicon::from_hunspell(
            "sr-Latn",
            "2\nnemački\nčovek\n",
            "SET UTF-8\n",
            None,
        )
        .expect("parses");
        let (paragraphs, restored) = super::policy::recovered_paragraphs(
            "Prevod: nemacki covek\nПревод речника",
            0.0,
            Some(&lexicon),
            Some("sr"),
        );
        assert_eq!(restored, 2);
        let tags: Vec<String> = paragraphs
            .iter()
            .map(|paragraph| match paragraph.inlines.first() {
                Some(Inline::Run(run)) => run
                    .props
                    .language
                    .as_ref()
                    .and_then(|language| language.val.as_deref())
                    .unwrap_or("")
                    .to_owned(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(tags, ["sr-Latn-RS", "sr-Cyrl-RS"]);
        assert_eq!(super::policy::language_of("en-GB", "Hello"), "en-GB");
    }

    #[test]
    fn a_rotated_page_is_cropped_where_it_is_displayed() {
        let geometry = |rotation| strict_ooxml_pdf::PageGeometry {
            width: 842.0,
            height: 595.0,
            rotation,
        };
        // The whole landscape box of a page turned upright covers the upright page.
        assert_eq!(
            displayed_rect(geometry(90), 0.0, 0.0, 842.0, 595.0),
            (0.0, 0.0, 595.0, 842.0)
        );
        // The unrotated top-left corner turns to the top-right.
        assert_eq!(
            displayed_rect(geometry(90), 0.0, 0.0, 100.0, 50.0),
            (545.0, 0.0, 50.0, 100.0)
        );
        assert_eq!(
            displayed_rect(geometry(270), 0.0, 0.0, 100.0, 50.0),
            (0.0, 742.0, 50.0, 100.0)
        );
        assert_eq!(
            displayed_rect(geometry(180), 0.0, 0.0, 100.0, 50.0),
            (742.0, 545.0, 100.0, 50.0)
        );
        assert_eq!(
            displayed_rect(geometry(0), 10.0, 20.0, 100.0, 50.0),
            (10.0, 20.0, 100.0, 50.0)
        );
    }
    use super::policy::RECOVERED_STYLE_ID;

    #[test]
    fn a_models_breaks_become_paragraphs_and_they_are_marked() {
        let paragraphs = paragraphs_of("  first line  \n\n second line\n   \nthird", 36.0);
        assert_eq!(paragraphs.len(), 3, "blank lines are not paragraphs");
        for paragraph in &paragraphs {
            let style = paragraph.props.style.as_ref().expect("marked");
            assert_eq!(style.as_str(), RECOVERED_STYLE_ID);
            // The region's left edge, in twips: 36 pt is 720.
            assert_eq!(
                paragraph
                    .props
                    .indentation
                    .and_then(|indent| indent.start)
                    .map(|start| start.0),
                Some(720),
                "the text is indented to the region it was read from"
            );
        }
    }
}
