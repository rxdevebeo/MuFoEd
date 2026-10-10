//! Map native DjVu zones; do not invent PDF glyphs or run PDF structure inference.
use std::collections::{BTreeMap, BTreeSet};

use crate::{to_twips, ConversionReport, ConvertError, Converted, Mode, Severity};
use strict_ooxml_djvu::{
    AnnotationTarget, DjvuDocument, DjvuPage, NavItem, Rect, TextLayer, TextZone, ZoneKind,
};
use strict_ooxml_wml::model::{
    block::{Block, Paragraph},
    inline::{Bookmark, BookmarkId, Hyperlink, Inline, Run, RunContent, TextNode},
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
    // SC-D9/L3: the `NAVM` outline becomes flat bookmarks on its target pages.
    let mut outline = Vec::new();
    let mut outline_external = 0usize;
    let mut outline_unresolved = 0usize;
    collect_outline(
        &reader.outline,
        all,
        &mut outline,
        &mut outline_external,
        &mut outline_unresolved,
    );
    if !reader.outline.is_empty() {
        report.record(
            "djvu.outline",
            Severity::Inferred,
            "NAVM outline levels are flattened: Word bookmarks are flat and the tree's depth is not kept",
        );
    }
    if outline_external > 0 {
        report.record(
            "djvu.outline.external",
            Severity::Lost,
            format!(
                "{outline_external} outline entries point outside the document and are not bookmarked"
            ),
        );
    }
    if outline_unresolved > 0 {
        report.record(
            "djvu.outline.unresolved",
            Severity::Lost,
            format!("{outline_unresolved} outline entries target no page"),
        );
    }
    let mut outline_by_page: BTreeMap<usize, Vec<&OutlineBookmark>> = BTreeMap::new();
    for entry in &outline {
        outline_by_page.entry(entry.page).or_default().push(entry);
    }
    // `maparea` page bookmarks take `w:id` 1..=page count; outline bookmarks continue.
    let mut next_bookmark_id = all.len() + 1;
    let mut used_names = BTreeSet::new();
    // SC-D9/L2: a page that some `maparea` points at gets a bookmark to link to.
    let bookmarked = bookmarked_pages(all);
    for (index, page) in all
        .get(start - 1..end)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
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
                // SC-D9: a `maparea` becomes a hyperlink on the text it covers.
                let mut links: Vec<(Rect, Link)> = Vec::new();
                for annotation in &page.annotations {
                    match &annotation.target {
                        AnnotationTarget::Url(url) => {
                            links.push((annotation.rect, Link::External(url.clone())));
                        }
                        AnnotationTarget::Page(reference) => {
                            if let Some(target) = resolve_page(reference, page.number, all) {
                                links.push((
                                    annotation.rect,
                                    Link::Anchor(format!("djvu-page-{target}")),
                                ));
                            } else {
                                report.record(
                                    "djvu.annotation.unresolved",
                                    Severity::Lost,
                                    format!(
                                        "page {}: maparea page link {reference:?} does not resolve",
                                        page.number
                                    ),
                                );
                            }
                        }
                    }
                }
                let mut mapped = vec![false; links.len()];
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
                    if let Some(index) = links.iter().enumerate().position(|(index, (area, _))| {
                        !mapped.get(index).copied().unwrap_or(true) && covers(*area, z.rect)
                    }) {
                        if let Some((_, link)) = links.get(index) {
                            wrap_hyperlink(&mut p, link);
                            if let Some(slot) = mapped.get_mut(index) {
                                *slot = true;
                            }
                        }
                    }
                    document.body.blocks.push(Block::Paragraph(p));
                }
                for (index, (_, link)) in links.iter().enumerate() {
                    if mapped.get(index).copied().unwrap_or(false) {
                        report.record(
                            "djvu.annotation.link",
                            Severity::Inferred,
                            format!("page {}: maparea became a hyperlink", page.number),
                        );
                    } else {
                        let _ = link;
                        report.record(
                            "djvu.annotation.unmatched",
                            Severity::Lost,
                            format!(
                                "page {}: maparea covers no text zone and is not a hyperlink",
                                page.number
                            ),
                        );
                    }
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
        // A `maparea` on a page with no native text cannot cover a word; report
        // it rather than dropping it silently (SC-D10).
        if !matches!(page.text, TextLayer::Readable(_)) {
            for _ in &page.annotations {
                report.record(
                    "djvu.annotation.no-text",
                    Severity::Lost,
                    format!("page {}: maparea has no native text to cover", page.number),
                );
            }
        }
        // A page an internal `maparea` points at carries a bookmark to link to.
        if bookmarked.contains(&page.number) {
            if let Some(blocks) = document.body.blocks.get_mut(first..) {
                if add_page_bookmark(
                    blocks,
                    &page.number.to_string(),
                    &format!("djvu-page-{}", page.number),
                ) {
                    report.record(
                        "djvu.annotation.bookmark",
                        Severity::Inferred,
                        format!("page {}: bookmark for internal maparea links", page.number),
                    );
                }
            }
        }
        // SC-D9/L3: outline entries targeting this page become bookmarks named
        // after their heading. Nesting is recorded, not represented.
        if let Some(entries) = outline_by_page.get(&page.number) {
            if let Some(blocks) = document.body.blocks.get_mut(first..) {
                for entry in entries {
                    let name = outline_name(&entry.title, &mut used_names);
                    if add_page_bookmark(blocks, &next_bookmark_id.to_string(), &name) {
                        next_bookmark_id += 1;
                        report.record(
                            "djvu.outline.bookmark",
                            Severity::Inferred,
                            format!("page {}: outline bookmark {name:?}", page.number),
                        );
                    }
                }
            }
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
            code: None,
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
                title: None,
            }),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            graphic_uri: None,
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some(Arc::from("DjVu page")),
                descr: None,
                nv_id: None,
                bw_mode: None,
                blip: Some(BlipRef {
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
            })),
            location: location.clone(),
        }),
        location,
    })
}

/// A resolved `maparea` link.
enum Link {
    /// An external URL (`r:id` via the writer).
    External(String),
    /// An in-document bookmark name (`w:anchor`).
    Anchor(String),
}

/// Whether a zone's centre falls inside a `maparea` rectangle.
///
/// A maparea is an image-map hotspot; the text it means is the text under it, so
/// the zone centre is the test. Both rectangles are in the same point system.
fn covers(area: Rect, zone: Rect) -> bool {
    let cx = zone.x + zone.width / 2.0;
    let cy = zone.y + zone.height / 2.0;
    cx >= area.x && cx <= area.x + area.width && cy >= area.y && cy <= area.y + area.height
}

/// The one-based page a `maparea` reference points at, if it resolves.
///
/// Order follows `STAGE-9-TASK.md` §7.3 L2: `page=N`, a `±N` offset, a numeric
/// index, then a page id or title.
fn resolve_page(reference: &str, current: usize, pages: &[DjvuPage]) -> Option<usize> {
    let count = pages.len();
    if let Some(number) = reference.strip_prefix("page=") {
        return number
            .parse::<usize>()
            .ok()
            .filter(|number| (1..=count).contains(number));
    }
    let count = i64::try_from(count).ok()?;
    if reference.starts_with('+') || reference.starts_with('-') {
        let delta = reference.parse::<i64>().ok()?;
        let target = i64::try_from(current).ok()? + delta;
        if (1..=count).contains(&target) {
            return usize::try_from(target).ok();
        }
        return None;
    }
    if let Ok(number) = reference.parse::<usize>() {
        return (1..=pages.len()).contains(&number).then_some(number);
    }
    pages
        .iter()
        .find(|page| page.id == reference || page.title == reference)
        .map(|page| page.number)
}

/// Every page an internal `maparea` in `pages` points at.
fn bookmarked_pages(pages: &[DjvuPage]) -> BTreeSet<usize> {
    let mut bookmarked = BTreeSet::new();
    for page in pages {
        for annotation in &page.annotations {
            if let AnnotationTarget::Page(reference) = &annotation.target {
                if let Some(target) = resolve_page(reference, page.number, pages) {
                    bookmarked.insert(target);
                }
            }
        }
    }
    bookmarked
}

/// A resolved outline entry: the page it targets and its heading.
struct OutlineBookmark {
    page: usize,
    title: String,
}

/// Where an outline entry's URL points.
enum OutlineTarget {
    /// A one-based page in this document.
    Internal(usize),
    /// Outside the document.
    External,
    /// Internal, but not resolvable to a page.
    Unresolved,
}

/// Flattens the outline forest into one bookmark per resolvable entry.
fn collect_outline(
    items: &[NavItem],
    pages: &[DjvuPage],
    out: &mut Vec<OutlineBookmark>,
    external: &mut usize,
    unresolved: &mut usize,
) {
    for item in items {
        match outline_target(&item.url, pages) {
            OutlineTarget::Internal(page) => out.push(OutlineBookmark {
                page,
                title: item.title.clone(),
            }),
            OutlineTarget::External => *external += 1,
            OutlineTarget::Unresolved => *unresolved += 1,
        }
        collect_outline(&item.children, pages, out, external, unresolved);
    }
}

/// An outline URL is internal when it is a `#` fragment; DjVuLibre writes
/// `#page=N` or `#name`. A `±N` offset names no page of its own and is left
/// unresolved.
fn outline_target(url: &str, pages: &[DjvuPage]) -> OutlineTarget {
    if let Some(reference) = url.strip_prefix('#') {
        if reference.starts_with('+') || reference.starts_with('-') {
            return OutlineTarget::Unresolved;
        }
        return match resolve_page(reference, 1, pages) {
            Some(page) => OutlineTarget::Internal(page),
            None => OutlineTarget::Unresolved,
        };
    }
    if url.is_empty() {
        OutlineTarget::Unresolved
    } else {
        OutlineTarget::External
    }
}

/// A unique, Word-legal bookmark name carrying the outline heading.
///
/// `w:name` must begin with a letter, so the heading is reduced to
/// letters/digits/underscores behind a `djvu-outline-` stem; a numeric suffix
/// makes it unique. The stem is capped well under Word's 40-character limit.
fn outline_name(title: &str, used: &mut BTreeSet<String>) -> String {
    let mut stem = String::from("djvu-outline-");
    for ch in title.chars() {
        if stem.len() >= 32 {
            break;
        }
        stem.push(if ch.is_alphanumeric() || ch == '_' {
            ch
        } else {
            '_'
        });
    }
    let mut candidate = stem.clone();
    let mut suffix = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}-{suffix}");
        suffix += 1;
    }
    candidate
}

/// Wraps a page's first-to-last paragraph in a bookmark. Returns whether one was
/// written (a page with no paragraph has nowhere to attach it). `id` is the
/// numeric `w:id` OOXML requires; `name` is the `w:anchor` target.
fn add_page_bookmark(blocks: &mut [Block], id: &str, name: &str) -> bool {
    let id = BookmarkId::new(id);
    let Some(first) = blocks.iter_mut().find_map(paragraph_mut) else {
        return false;
    };
    first.inlines.insert(
        0,
        Inline::BookmarkStart(Bookmark {
            id: id.clone(),
            name: std::sync::Arc::from(name),
            col_first: None,
            col_last: None,
        }),
    );
    let Some(last) = blocks.iter_mut().rev().find_map(paragraph_mut) else {
        return false;
    };
    last.inlines.push(Inline::BookmarkEnd(id));
    true
}

fn paragraph_mut(block: &mut Block) -> Option<&mut Paragraph> {
    match block {
        Block::Paragraph(paragraph) => Some(paragraph),
        _ => None,
    }
}

/// Wraps a paragraph's inlines in a hyperlink, external or internal.
fn wrap_hyperlink(paragraph: &mut Paragraph, link: &Link) {
    let (target, anchor) = match link {
        Link::External(url) => (Some(std::sync::Arc::from(url.as_str())), None),
        Link::Anchor(name) => (None, Some(std::sync::Arc::from(name.as_str()))),
    };
    let inlines = std::mem::take(&mut paragraph.inlines);
    paragraph.inlines = vec![Inline::Hyperlink(Hyperlink {
        rel_id: None,
        target,
        anchor,
        tooltip: None,
        history: false,
        tgt_frame: None,
        inlines,
        location: strict_ooxml_core::error::SourceLocation::unknown(),
    })];
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
