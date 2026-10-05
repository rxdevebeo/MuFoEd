//! Map native DjVu zones; do not invent PDF glyphs or run PDF structure inference.
use crate::{to_twips, ConversionReport, ConvertError, Converted, Mode, Severity};
use strict_ooxml_djvu::{DjvuDocument, DjvuPage, TextLayer, TextZone, ZoneKind};
use strict_ooxml_wml::model::{
    block::{Block, Paragraph},
    inline::{Inline, Run, RunContent, TextNode},
    props::{
        PageMargins, PageSize, ParagraphProperties, RunProperties, Section, SectionProperties,
    },
    values::{Indentation, LineSpacingRule, Rsids, Space, Spacing, Twips},
};
/// DjVu conversion policy. Recognition is opt-in and never replaces native text.
#[derive(Clone)]
#[must_use]
pub struct DjvuOptions {
    /// Native paragraphs or native lines with estimated visual placement.
    pub mode: Mode,
    /// Inclusive one-based page range; invalid endpoints are errors.
    pub pages: Option<(usize, usize)>,
    /// Preserve scans as page PNGs when structured text is absent.
    pub embed_images: bool,
    /// Provider for pages with no text chunk. Available only with native image decoding.
    #[cfg(feature = "djvu-image")]
    pub text_recovery: Option<std::sync::Arc<dyn strict_ooxml_ocr::TextRecovery>>,
    /// Maximum bytes accepted from a caller-supplied recognition provider.
    pub max_recovered_text_bytes: usize,
}
impl Default for DjvuOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Semantic,
            pages: None,
            embed_images: true,
            #[cfg(feature = "djvu-image")]
            text_recovery: None,
            max_recovered_text_bytes: 1024 * 1024,
        }
    }
}
impl DjvuOptions {
    /// Choose a conversion mode.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
    /// Control whether scan images are embedded.
    pub fn embed_images(mut self, enabled: bool) -> Self {
        self.embed_images = enabled;
        self
    }
    /// Select an inclusive one-based range.
    pub fn pages(mut self, start: usize, end: usize) -> Self {
        self.pages = Some((start, end));
        self
    }
    /// Supply an OCR provider; merely supplying one does not authorize overwriting native text.
    #[cfg(feature = "djvu-image")]
    pub fn text_recovery(
        mut self,
        provider: std::sync::Arc<dyn strict_ooxml_ocr::TextRecovery>,
    ) -> Self {
        self.text_recovery = Some(provider);
        self
    }
}
/// Convert DjVu zones into a document ready for the Strict writer.
/// OCR failure is a report entry; malformed page selection is a conversion error.
pub fn convert_djvu(
    reader: &mut DjvuDocument,
    options: &DjvuOptions,
) -> Result<Converted, ConvertError> {
    let all = reader.pages();
    let (start, end) = options.pages.unwrap_or((1, all.len()));
    if start == 0 || start > end || start > all.len() {
        return Err(ConvertError::NoSuchPage(start));
    }
    if end > all.len() {
        return Err(ConvertError::NoSuchPage(end));
    }
    let mut document = crate::empty_document();
    let mut report = ConversionReport::new(options.mode);
    let mut media = Vec::new();
    for warning in &reader.warnings {
        report.record("djvu.document.unsupported", Severity::Unsupported, warning);
    }
    for (index, page) in all[start - 1..end].iter().enumerate() {
        for warning in &page.warnings {
            report.record(
                "djvu.reader",
                Severity::Unsupported,
                format!("page {}: {warning}", page.number),
            );
        }
        let first = document.body.blocks.len();
        match &page.text {
            TextLayer::Readable(root) => {
                let mut zones = Vec::new();
                collect(root, options.mode, &mut zones);
                report.lines += count_lines(root);
                report.record("djvu.box-font",Severity::Inferred,"font size is estimated from a zone's box height; original font and emphasis are unknown");
                report.record("djvu.graphics",Severity::Inferred,"native text zones are retained; drawings outside the zones are not segmented from the page image");
                let mut previous = None;
                for z in zones {
                    let mut p = paragraph(&normalize(&z.text));
                    let size = crate::to_half_points(font_height(z).clamp(6.0, 72.0));
                    if let Some(Inline::Run(run)) = p.inlines.first_mut() {
                        run.props.size = Some(strict_ooxml_wml::model::values::HalfPoints(size));
                    }
                    if options.mode == Mode::Visual {
                        p.props.indentation = Some(Indentation {
                            start: Some(Twips(to_twips(z.rect.x))),
                            ..Indentation::default()
                        });
                        let baseline = z.rect.y + z.rect.height;
                        let leading = previous
                            .map_or(baseline, |prev: f64| baseline - prev)
                            .max(1.0);
                        p.props.spacing = Some(Spacing {
                            line: Some(Twips(to_twips(leading))),
                            line_rule: Some(LineSpacingRule::Exact),
                            ..Spacing::default()
                        });
                        previous = Some(baseline);
                        report.record("djvu.visual-placement",Severity::Inferred,"line boxes estimate leading; glyph positions and original typography are unavailable");
                    }
                    document.body.blocks.push(Block::Paragraph(p));
                }
            }
            TextLayer::Unusable { text, reason } => {
                report.record("djvu.text.unusable",Severity::Lost,format!("page {}: {reason}; native text is retained without geometry; OCR is not substituted",page.number));
                if !text.is_empty() {
                    document
                        .body
                        .blocks
                        .push(Block::Paragraph(paragraph(&normalize(text))));
                }
                scan(
                    page,
                    reader,
                    options,
                    &mut document,
                    &mut report,
                    &mut media,
                    false,
                );
            }
            TextLayer::Empty => {
                report.record(
                    "djvu.text.empty",
                    Severity::Lost,
                    format!("page {} has no native text layer", page.number),
                );
                scan(
                    page,
                    reader,
                    options,
                    &mut document,
                    &mut report,
                    &mut media,
                    true,
                );
            }
        }
        if first == document.body.blocks.len() {
            document.body.blocks.push(Block::Paragraph(paragraph("")));
        }
        let properties = section(page);
        // Each source page ends its own WML section; the final one is the body sectPr.
        if index + 1 < end - start + 1 {
            if let Some(Block::Paragraph(p)) = document.body.blocks.last_mut() {
                p.props.section = Some(properties);
            }
        } else {
            document.sections.push(Section {
                properties,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            });
        }
    }
    crate::sanitize_xml_text(&mut document, &mut report);
    report.paragraphs = document.body.blocks.len();
    report.images = media.len();
    Ok(Converted {
        document,
        report,
        media,
    })
}
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{b}' | '\u{1d}' | '\u{1f}' | '\n' | '\r' => ' ',
            _ => c,
        })
        .collect::<String>()
        .trim_end()
        .to_owned()
}
fn collect<'a>(z: &'a TextZone, mode: Mode, out: &mut Vec<&'a TextZone>) {
    if z.kind == ZoneKind::Line || (mode == Mode::Semantic && z.kind == ZoneKind::Paragraph) {
        out.push(z);
        return;
    }
    for child in &z.children {
        collect(child, mode, out);
    }
}
fn count_lines(z: &TextZone) -> usize {
    usize::from(z.kind == ZoneKind::Line) + z.children.iter().map(count_lines).sum::<usize>()
}
fn paragraph(text: &str) -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Run(Run {
            props: RunProperties::default(),
            content: vec![RunContent::Text(TextNode {
                text: text.to_owned(),
                space: Space::Preserve,
            })],
            revision: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        })],
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    }
}
fn section(page: &DjvuPage) -> SectionProperties {
    let (mut w, mut h) = page.info.points();
    if matches!(page.info.rotation, 90 | 270) {
        std::mem::swap(&mut w, &mut h);
    }
    SectionProperties {
        page_size: Some(PageSize {
            width: Some(Twips(to_twips(w))),
            height: Some(Twips(to_twips(h))),
            orientation: None,
        }),
        page_margins: Some(PageMargins {
            top: Some(Twips(0)),
            left: Some(Twips(0)),
            right: Some(Twips(0)),
            bottom: Some(Twips(0)),
            ..PageMargins::default()
        }),
        ..SectionProperties::default()
    }
}
#[cfg(not(feature = "djvu-image"))]
fn scan(
    page: &DjvuPage,
    _: &DjvuDocument,
    options: &DjvuOptions,
    _: &mut strict_ooxml_wml::model::Document,
    report: &mut ConversionReport,
    _: &mut Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>,
    _: bool,
) {
    let _ = options;
    report.record(
        "djvu.image.disabled",
        Severity::Lost,
        format!(
            "page {}: rebuild with djvu-image for scans/OCR",
            page.number
        ),
    );
}
#[cfg(feature = "djvu-image")]
#[allow(clippy::too_many_arguments)]
fn scan(
    page: &DjvuPage,
    reader: &DjvuDocument,
    options: &DjvuOptions,
    document: &mut strict_ooxml_wml::model::Document,
    report: &mut ConversionReport,
    media: &mut Vec<(strict_ooxml_core::part::PartId, Vec<u8>)>,
    recover: bool,
) {
    if !(options.embed_images || recover && options.text_recovery.is_some()) {
        return;
    }
    let raster = match page.image(reader.limits) {
        Ok(raster) => raster,
        Err(error) => {
            report.record(
                "djvu.image.failed",
                Severity::Lost,
                format!("page {}: {error}", page.number),
            );
            return;
        }
    };
    let png = raster.bytes;
    if options.embed_images {
        let part = strict_ooxml_core::part::PartId::new(format!(
            "/word/media/djvu-page-{}.png",
            page.number
        ));
        document
            .media
            .insert(strict_ooxml_wml::model::drawing::MediaItem {
                part: part.clone(),
                content_type: None,
                kind: strict_ooxml_wml::model::drawing::MediaKind::Png,
            });
        let mut p = paragraph("");
        let (mut w, mut h) = page.info.points();
        if matches!(page.info.rotation, 90 | 270) {
            std::mem::swap(&mut w, &mut h);
        }
        p.inlines = vec![picture(&part, w, h)];
        document.body.blocks.push(Block::Paragraph(p));
        media.push((part, png.clone()));
    } else {
        report.record(
            "djvu.image.omitted",
            Severity::Lost,
            format!("page {} image embedding disabled", page.number),
        );
    }
    if !recover || !raster.has_ink {
        return;
    }
    let Some(provider) = &options.text_recovery else {
        report.record(
            "djvu.ocr.disabled",
            Severity::Lost,
            format!("page {}: OCR provider not configured", page.number),
        );
        return;
    };
    let image = strict_ooxml_ocr::Image {
        bytes: png,
        encoding: strict_ooxml_ocr::Encoding::Png,
    };
    match provider.recover_page(&image) {
        Ok(Some(answer))
            if !answer.text.trim().is_empty()
                && answer.text.len() <= options.max_recovered_text_bytes =>
        {
            let paragraphs = crate::recover::policy::paragraphs_of(&answer.text, 0.0);
            report.recovered_pages += 1;
            report.recovered_paragraphs += paragraphs.len();
            report.record(
                "djvu.ocr.recovered",
                Severity::Recovered,
                format!(
                    "page {}: {} version {}; no text geometry claimed",
                    page.number, answer.model, answer.version
                ),
            );
            crate::recover::policy::declare_style(document);
            document
                .body
                .blocks
                .extend(paragraphs.into_iter().map(Block::Paragraph));
        }
        Ok(Some(_)) => report.record(
            "djvu.ocr.unusable",
            Severity::Lost,
            format!("page {}: empty or oversized OCR answer", page.number),
        ),
        Ok(None) => report.record(
            "djvu.ocr.no-text",
            Severity::Lost,
            format!(
                "page {}: {} found no text",
                page.number,
                provider.model_name()
            ),
        ),
        Err(error) => report.record(
            "djvu.ocr.failed",
            Severity::Lost,
            format!("page {}: {}: {error}", page.number, provider.model_name()),
        ),
    }
}
#[cfg(feature = "djvu-image")]
fn picture(part: &strict_ooxml_core::part::PartId, w: f64, h: f64) -> Inline {
    use std::sync::Arc;
    use strict_ooxml_wml::model::drawing::{
        BlipRef, DocPr, Drawing, DrawingKind, Extent, Graphic, InlineDrawing, Picture,
    };
    let location = strict_ooxml_core::error::SourceLocation::unknown();
    let extent = Extent {
        cx: strict_ooxml_wml::model::values::Emu((w * 12700.0).round() as i64),
        cy: strict_ooxml_wml::model::values::Emu((h * 12700.0).round() as i64),
    };
    Inline::Drawing(Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            doc_pr: Some(DocPr {
                id: None,
                name: Some(Arc::from("DjVu page")),
                descr: None,
            }),
            graphic_uri: None,
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some(Arc::from("DjVu page")),
                descr: None,
                blip: Some(BlipRef {
                    embed: None,
                    link: None,
                    resolved: Some(part.clone()),
                    location: location.clone(),
                }),
                extent: Some(extent),
                src_rect: None,
                xfrm: None,
            })),
            location: location.clone(),
        }),
        location,
    })
}

// A paragraph's total box height is not its font size. Prefer word boxes,
// then visual lines, so multi-line paragraphs do not become 72-point type.
fn font_height(zone: &TextZone) -> f64 {
    fn collect(zone: &TextZone, kind: ZoneKind, out: &mut Vec<f64>) {
        if zone.kind == kind {
            out.push(zone.rect.height);
            return;
        }
        for child in &zone.children {
            collect(child, kind, out);
        }
    }
    let mut heights = Vec::new();
    collect(zone, ZoneKind::Word, &mut heights);
    if heights.is_empty() {
        collect(zone, ZoneKind::Line, &mut heights);
    }
    heights.sort_by(f64::total_cmp);
    heights
        .get(heights.len() / 2)
        .copied()
        .unwrap_or(zone.rect.height)
}
