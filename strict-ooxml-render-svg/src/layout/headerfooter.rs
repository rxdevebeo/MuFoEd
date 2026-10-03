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
//!
//! AUD-70: a part that contains PAGE/NUMPAGES/SECTIONPAGES/SECTION is laid out
//! again on every page with a [`FieldEnv`](crate::fields::FieldEnv); parts
//! without those fields stay cached.

use std::collections::HashMap;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::props::SectionProperties;
use strict_ooxml_wml::model::values::Twips;
use strict_ooxml_wml::model::{Block, HeaderFooterKind};

use strict_ooxml_core::error::Result;

use crate::fields::{blocks_have_dynamic_fields, FieldEnv};
use crate::layout::table::{layout_blocks_inline, offset_item};
use crate::layout::{Geometry, Item, LayoutContext, PlacedPage};
use crate::notes::NumberFormat;
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

/// Cached layout of a header/footer part without dynamic fields.
struct CachedRegion {
    region: Region,
}

/// Decorates already-paginated `pages` with the selected headers and footers.
pub(crate) fn decorate_pages(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    geometry: Geometry,
    section: Option<&SectionProperties>,
    total_pages: usize,
) -> Result<()> {
    if pages.is_empty() {
        return Ok(());
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
        return Ok(());
    }

    let page_start = section
        .and_then(|properties| properties.page_number.as_ref())
        .and_then(|page_number| page_number.start)
        .unwrap_or(1)
        .max(1);
    let page_format = NumberFormat::from_strict(
        section
            .and_then(|properties| properties.page_number.as_ref())
            .and_then(|page_number| page_number.format.as_deref()),
        NumberFormat::Decimal,
    );
    // Single-section documents: section index 1, section page count = total.
    // Multi-section geometry is AUD-74.
    let section_index = 1usize;
    let section_pages = total_pages.max(1);

    let mut cache: HashMap<PartId, CachedRegion> = HashMap::new();
    let mut dynamic: HashMap<PartId, bool> = HashMap::new();
    for (index, page) in pages.iter_mut().enumerate() {
        let page_ordinal = index + 1;
        let display_page = usize::try_from(page_start.saturating_sub(1))
            .unwrap_or(0)
            .saturating_add(page_ordinal);
        let env = FieldEnv {
            page_number: display_page,
            page_count: total_pages.max(1),
            section_index,
            section_pages,
            page_format,
        };
        let mut decorated: Vec<Item> = Vec::with_capacity(page.items.len() + 16);

        if has_headers {
            let header = select_reference(section, true, page_ordinal, title_page, even_and_odd);
            if let Some(region) = header.and_then(|part| {
                region_for(
                    ctx,
                    part,
                    left,
                    content_width,
                    env,
                    &mut cache,
                    &mut dynamic,
                )
            }) {
                ctx.charge_items(region.items.len())?;
                for item in &region.items {
                    decorated.push(offset_item(item, 0.0, header_offset));
                }
            }
        }

        decorated.append(&mut page.items);

        if has_footers {
            let footer = select_reference(section, false, page_ordinal, title_page, even_and_odd);
            if let Some(region) = footer.and_then(|part| {
                region_for(
                    ctx,
                    part,
                    left,
                    content_width,
                    env,
                    &mut cache,
                    &mut dynamic,
                )
            }) {
                ctx.charge_items(region.items.len())?;
                let y = (page.height_px - footer_offset - region.height).max(0.0);
                for item in &region.items {
                    decorated.push(offset_item(item, 0.0, y));
                }
            }
        }

        page.items = decorated;
    }
    ctx.field_env.set(None);
    Ok(())
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

/// Returns the region for `part`, caching only when it has no dynamic fields.
fn region_for(
    ctx: &LayoutContext<'_>,
    part: &PartId,
    left: f64,
    width: f64,
    env: FieldEnv,
    cache: &mut HashMap<PartId, CachedRegion>,
    dynamic: &mut HashMap<PartId, bool>,
) -> Option<Region> {
    let header_footer = ctx.document.header_footer(part)?;
    let is_dynamic = *dynamic
        .entry(part.clone())
        .or_insert_with(|| blocks_have_dynamic_fields(&header_footer.blocks));
    if is_dynamic {
        Some(layout_region(
            ctx,
            &header_footer.blocks,
            left,
            width,
            Some(env),
        ))
    } else if let Some(cached) = cache.get(part) {
        Some(Region {
            items: cached.region.items.clone(),
            height: cached.region.height,
        })
    } else {
        let region = layout_region(ctx, &header_footer.blocks, left, width, None);
        cache.insert(
            part.clone(),
            CachedRegion {
                region: Region {
                    items: region.items.clone(),
                    height: region.height,
                },
            },
        );
        Some(region)
    }
}

/// Lays out a header/footer region, returning its items and height.
fn layout_region(
    ctx: &LayoutContext<'_>,
    blocks: &[Block],
    left: f64,
    width: f64,
    env: Option<FieldEnv>,
) -> Region {
    ctx.field_env.set(env);
    let mut items = Vec::new();
    let mut y = 0.0;
    // A header is a block container of its own, so its content starts at 1 -
    // the same budget the reader counted it against, and not a fresh zero.
    layout_blocks_inline(ctx, blocks, left, width, &mut y, &mut items, 1, None);
    ctx.field_env.set(None);
    Region { items, height: y }
}
