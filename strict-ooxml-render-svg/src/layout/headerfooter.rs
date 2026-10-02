//! Header/footer layout and per-page decoration (STAGE-5 S5.3).
//!
//! Headers and footers are laid out once per referenced part into a region
//! whose origin is the content left edge at `y = 0`. Each page then receives
//! the region selected by `titlePg`/`evenAndOddHeaders`, positioned by the
//! `w:pgMar/@w:header` and `@w:footer` distances.
//!
//! Per the Stage-5 default decision (STAGE-5 §9, question 4), the header/footer
//! do not reduce the body's available height in this first increment: they are
//! painted inside the top/bottom margins, and the body keeps its own margins.

use std::collections::HashMap;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::props::SectionProperties;
use strict_ooxml_wml::model::values::Twips;
use strict_ooxml_wml::model::{Block, HeaderFooterKind};

use crate::layout::table::{layout_blocks_inline, offset_item};
use crate::layout::{Geometry, Item, LayoutContext, PlacedPage};
use crate::units::twips_to_px;

/// Default distance from a page edge to its header/footer, in twips (0.5 inch).
const DEFAULT_HEADER_FOOTER_TWIPS: i32 = 720;

/// A laid-out header/footer region.
struct Region {
    /// Paint items relative to the content left edge at `y = 0`.
    items: Vec<Item>,
    /// Region height in px.
    height: f64,
}

/// Decorates already-paginated `pages` with the selected headers and footers.
pub(crate) fn decorate_pages(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    geometry: Geometry,
    section: Option<&SectionProperties>,
) {
    if pages.is_empty() {
        return;
    }
    let scale = ctx.options.scale;
    let content_width = geometry.content_width();
    let left = geometry.left;

    let header_offset = twips_to_px(
        section_margin(section, true).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
        scale,
    );
    let footer_offset = twips_to_px(
        section_margin(section, false).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
        scale,
    );
    let even_and_odd = ctx.document.settings.even_and_odd_headers;
    let title_page = section.is_some_and(|section| section.title_page);
    let has_headers = section.is_some_and(|s| s.headers.iter().any(|r| r.part.is_some()));
    let has_footers = section.is_some_and(|s| s.footers.iter().any(|r| r.part.is_some()));
    if !has_headers && !has_footers {
        return;
    }

    let mut cache: HashMap<PartId, Region> = HashMap::new();
    for (index, page) in pages.iter_mut().enumerate() {
        let page_number = index + 1;
        let mut decorated: Vec<Item> = Vec::with_capacity(page.items.len() + 16);

        if has_headers {
            let header = select_reference(section, true, page_number, title_page, even_and_odd);
            if let Some(region) =
                header.and_then(|part| region_for(ctx, part, left, content_width, &mut cache))
            {
                for item in &region.items {
                    decorated.push(offset_item(item, 0.0, header_offset));
                }
            }
        }

        decorated.append(&mut page.items);

        if has_footers {
            let footer = select_reference(section, false, page_number, title_page, even_and_odd);
            if let Some(region) =
                footer.and_then(|part| region_for(ctx, part, left, content_width, &mut cache))
            {
                let y = (page.height_px - footer_offset - region.height).max(0.0);
                for item in &region.items {
                    decorated.push(offset_item(item, 0.0, y));
                }
            }
        }

        page.items = decorated;
    }
}

/// Returns the `w:header`/`w:footer` margin in twips, if declared.
fn section_margin(section: Option<&SectionProperties>, is_header: bool) -> Option<i32> {
    let margins = section?.page_margins?;
    let margin = if is_header {
        margins.header
    } else {
        margins.footer
    };
    margin.map(Twips::value)
}

/// Selects the referenced part for a page, honouring `titlePg`/`evenAndOddHeaders`.
fn select_reference(
    section: Option<&SectionProperties>,
    is_header: bool,
    page_number: usize,
    title_page: bool,
    even_and_odd: bool,
) -> Option<&PartId> {
    let section = section?;
    let references = if is_header {
        &section.headers
    } else {
        &section.footers
    };
    let find = |kind: HeaderFooterKind| {
        references
            .iter()
            .find(|reference| reference.kind == kind)
            .and_then(|reference| reference.part.as_ref())
    };
    if page_number == 1 && title_page {
        // With `w:titlePg` the first page uses only the `first` reference; when
        // it is absent the first page has no header/footer (no Default/Even
        // fallback), as Word/WPS do.
        return find(HeaderFooterKind::First);
    }
    if even_and_odd && page_number.is_multiple_of(2) {
        return find(HeaderFooterKind::Even);
    }
    find(HeaderFooterKind::Default)
}

/// Returns the cached region for `part`, laying it out on first use.
fn region_for<'a>(
    ctx: &LayoutContext<'_>,
    part: &PartId,
    left: f64,
    width: f64,
    cache: &'a mut HashMap<PartId, Region>,
) -> Option<&'a Region> {
    if !cache.contains_key(part) {
        let header_footer = ctx.document.header_footer(part)?;
        let (items, height) = layout_region(ctx, &header_footer.blocks, left, width);
        cache.insert(part.clone(), Region { items, height });
    }
    cache.get(part)
}

/// Lays out a header/footer region, returning its items and height.
fn layout_region(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
) -> (Vec<Item>, f64) {
    let mut items = Vec::new();
    let mut y = 0.0;
    // A header is a block container of its own, so its content starts at 1 -
    // the same budget the reader counted it against, and not a fresh zero.
    layout_blocks_inline(ctx, blocks, left, width, &mut y, &mut items, 1, None);
    (items, y)
}
