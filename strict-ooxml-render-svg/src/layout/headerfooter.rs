//! Header/footer layout and per-page decoration (STAGE-5 S5.3).
//!
//! Headers and footers are laid out once per referenced part into a region
//! whose origin is the content left edge at `y = 0`. Each page then receives
//! the region selected by `titlePg`/`evenAndOddHeaders`, positioned by the
//! `w:pgMar/@w:header` and `@w:footer` distances.
//!
//! Per the A15 decision, a header or footer that extends past the page margin
//! pushes the body start down or the body end up. A region that fits inside
//! the margin leaves the body margins unchanged. Negative margins and an
//! intentional overlap stay a diagnostic, not an invented extra gap.
//!
//! AUD-70: a part that contains PAGE/NUMPAGES/SECTIONPAGES/SECTION is laid out
//! again on every page with a [`FieldEnv`](crate::fields::FieldEnv); parts
//! without those fields stay cached.
//!
//! AUD-74: each page uses the section of its first body content, with header/
//! footer reference inheritance from prior sections when the current one has
//! none.

use std::collections::HashMap;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::props::{HeaderFooterRef, Section, SectionProperties};
use strict_ooxml_wml::model::values::Twips;
use strict_ooxml_wml::model::{Block, HeaderFooterKind};

use crate::fields::{blocks_have_dynamic_fields, FieldEnv};
use crate::layout::table::{layout_blocks_inline, offset_item};
use crate::layout::{geometry_for, Geometry, Item, LayoutContext, PlacedPage};
use crate::notes::NumberFormat;
use crate::units::twips_to_px;

/// Default distance from a page edge to its header/footer, in twips (0.5 inch).
const DEFAULT_HEADER_FOOTER_TWIPS: i32 = 720;

/// A laid-out header/footer region.
struct Region {
    /// Paint items relative to the content left edge at `y = 0`.
    items: Vec<Item>,
    /// Flow height in px - what the body clears and the cache stores.
    height: f64,
    /// Lowest ink the region draws, floats included. A footer is placed by
    /// this, so a footer made of floating objects lands on the page.
    ink_bottom: f64,
}

/// Cached layout of a header/footer part without dynamic fields.
struct CachedRegion {
    region: Region,
}

/// Decorates already-paginated `pages` with the selected headers and footers.
#[allow(clippy::too_many_lines)]
pub(crate) fn decorate_pages(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    sections: &[Section],
    total_pages: usize,
    section_pages_hint: &[usize],
) -> Result<()> {
    if pages.is_empty() {
        return Ok(());
    }
    let even_and_odd = ctx.document.settings.even_and_odd_headers;
    let mut cache: HashMap<(PartId, u64), CachedRegion> = HashMap::new();
    let mut dynamic: HashMap<PartId, bool> = HashMap::new();
    let mut section_page_counts = vec![0usize; sections.len().max(1)];
    for page in pages.iter() {
        let index = page.section_index.min(section_page_counts.len() - 1);
        if let Some(count) = section_page_counts.get_mut(index) {
            *count += 1;
        }
    }
    let mut section_page_seen = vec![0usize; sections.len().max(1)];

    for (index, page) in pages.iter_mut().enumerate() {
        let section_index = page.section_index.min(sections.len().saturating_sub(1));
        let seen_slot = page.section_index.min(section_page_seen.len() - 1);
        let mut section_page_ordinal = 0;
        if let Some(seen) = section_page_seen.get_mut(seen_slot) {
            *seen += 1;
            section_page_ordinal = *seen;
        }
        let Some(section) = inherited_section(sections, section_index) else {
            continue;
        };
        let margins = geometry_for(
            Some(&section),
            ctx.options.scale,
            None,
            ctx.document.settings.gutter_at_top,
            ctx.options.default_paper,
        );
        let geometry = Geometry {
            width: page.width_px,
            height: page.height_px,
            left: margins.left,
            top: margins.top,
            right: margins.right,
            bottom: margins.bottom,
            grid_line_pitch: None,
            // Headers and footers are never set in columns.
            columns: crate::layout::ColumnLayout::single(
                page.width_px - margins.left - margins.right,
            ),
            active_column: 0,
        };
        let content_width = geometry.content_width();
        let left = geometry.left;
        let header_offset = twips_to_px(
            section_margin(Some(&section), true).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
            ctx.options.scale,
        );
        let footer_offset = twips_to_px(
            section_margin(Some(&section), false).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
            ctx.options.scale,
        );
        let title_page = section.title_page;
        let has_headers = section.headers.iter().any(|r| r.part.is_some());
        let has_footers = section.footers.iter().any(|r| r.part.is_some());
        if !has_headers && !has_footers {
            continue;
        }

        let page_start = section
            .page_number
            .as_ref()
            .and_then(|page_number| page_number.start)
            .unwrap_or(1)
            .max(1);
        let page_format = NumberFormat::from_strict(
            section
                .page_number
                .as_ref()
                .and_then(|page_number| page_number.format.as_deref()),
            NumberFormat::Decimal,
        );
        let page_ordinal = index + 1;
        let display_page = usize::try_from(page_start.saturating_sub(1))
            .unwrap_or(0)
            .saturating_add(page_ordinal);
        let env = FieldEnv {
            page_number: display_page,
            page_count: total_pages.max(1),
            section_index: section_index + 1,
            section_pages: section_pages_hint
                .get(section_index)
                .copied()
                .or_else(|| section_page_counts.get(section_index).copied())
                .unwrap_or(1)
                .max(1),
            page_format,
            chapter: crate::fields::chapter_on_page(ctx, index, section_index),
        };
        let mut decorated: Vec<Item> = Vec::with_capacity(page.items.len() + 16);

        if has_headers {
            let header = select_reference(
                Some(&section),
                true,
                page_ordinal,
                section_page_ordinal,
                title_page,
                even_and_odd,
            );
            // Page- and margin-relative pictures are resolved while this is
            // set, then stored minus the shift `offset_item` adds below.
            ctx.region_shift.set(Some(header_offset));
            let region = header.and_then(|part| {
                region_for(
                    ctx,
                    part,
                    left,
                    content_width,
                    env,
                    &mut cache,
                    &mut dynamic,
                )
            });
            ctx.region_shift.set(None);
            if let Some(region) = region {
                ctx.charge_items(region.items.iter().map(|item| item.leaves().count()).sum())?;
                for item in &region.items {
                    decorated.push(offset_item(item, 0.0, header_offset));
                }
            }
        }

        decorated.append(&mut page.items);

        if has_footers {
            let footer = select_reference(
                Some(&section),
                false,
                page_ordinal,
                section_page_ordinal,
                title_page,
                even_and_odd,
            );
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
                ctx.charge_items(region.items.iter().map(|item| item.leaves().count()).sum())?;
                let y = (page.height_px - footer_offset - region.ink_bottom).max(0.0);
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

/// Section properties with header/footer refs inherited from prior sections.
fn inherited_section(sections: &[Section], index: usize) -> Option<SectionProperties> {
    let base = sections.get(index)?.properties.clone();
    let mut props = base;
    if !props
        .headers
        .iter()
        .any(|reference| reference.part.is_some())
    {
        if let Some(headers) = prior_refs(sections, index, true) {
            props.headers = headers;
        }
    }
    if !props
        .footers
        .iter()
        .any(|reference| reference.part.is_some())
    {
        if let Some(footers) = prior_refs(sections, index, false) {
            props.footers = footers;
        }
    }
    Some(props)
}

fn prior_refs(sections: &[Section], index: usize, headers: bool) -> Option<Vec<HeaderFooterRef>> {
    for prior in sections.iter().take(index).rev() {
        let refs = if headers {
            &prior.properties.headers
        } else {
            &prior.properties.footers
        };
        if refs.iter().any(|reference| reference.part.is_some()) {
            return Some(refs.clone());
        }
    }
    None
}

/// Inputs for measuring the *active* header/footer of one page.
pub(crate) struct PageRegionQuery<'a> {
    /// Layout context (fonts, document parts, warnings).
    pub ctx: &'a LayoutContext<'a>,
    /// Document sections (for first/even/default inheritance).
    pub sections: &'a [Section],
    /// Zero-based section index of the page.
    pub section_index: usize,
    /// 1-based document page ordinal (odd/even).
    pub page_ordinal: usize,
    /// 1-based page ordinal within the section (`titlePg`).
    pub section_page_ordinal: usize,
    /// Assumed `NUMPAGES` for this pass.
    pub total_pages: usize,
    /// Assumed `SECTIONPAGES` for this pass.
    pub section_pages: usize,
    /// `w:pgNumType/@w:start`.
    pub page_start: u32,
    /// `w:pgNumType/@w:fmt`.
    pub page_format: NumberFormat,
    /// Section page margins *before* header/footer extras.
    pub geometry: Geometry,
}

/// Extra body inset from the active header/footer of one page.
pub(crate) struct PageRegionReserve {
    /// Added to the top margin so the body clears the header.
    pub extra_top: f64,
    /// Added to the bottom margin so the body clears the footer.
    pub extra_bottom: f64,
    /// Measured active header height in px.
    pub header_height: f64,
    /// Measured active footer height in px.
    pub footer_height: f64,
}

/// How far the body must move so it clears the *active* header and footer.
///
/// First/even/default (and inherited refs) are selected before measuring.
/// PAGE/NUMPAGES/SECTIONPAGES use `query`'s FieldEnv. Negative source margins
/// are left overlapping with a diagnostic instead of inventing a gap.
pub(crate) fn page_body_reserve(query: &PageRegionQuery<'_>) -> PageRegionReserve {
    let empty = PageRegionReserve {
        extra_top: 0.0,
        extra_bottom: 0.0,
        header_height: 0.0,
        footer_height: 0.0,
    };
    let Some(section) = inherited_section(query.sections, query.section_index) else {
        return empty;
    };
    let scale = query.ctx.options.scale;
    let header_offset = twips_to_px(
        section_margin(Some(&section), true).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
        scale,
    );
    let footer_offset = twips_to_px(
        section_margin(Some(&section), false).unwrap_or(DEFAULT_HEADER_FOOTER_TWIPS),
        scale,
    );
    let negative_margins = query.geometry.top < 0.0
        || query.geometry.bottom < 0.0
        || header_offset < 0.0
        || footer_offset < 0.0;
    if negative_margins {
        query.ctx.warn(
            "render.negative-page-margin: source overlap left in place; body not auto-shifted"
                .to_owned(),
        );
        return empty;
    }
    let even_and_odd = query.ctx.document.settings.even_and_odd_headers;
    let display_page = usize::try_from(query.page_start.saturating_sub(1))
        .unwrap_or(0)
        .saturating_add(query.page_ordinal);
    let env = FieldEnv {
        page_number: display_page,
        page_count: query.total_pages.max(1),
        section_index: query.section_index + 1,
        section_pages: query.section_pages.max(1),
        page_format: query.page_format,
        chapter: crate::fields::chapter_on_page(
            query.ctx,
            query.page_ordinal.saturating_sub(1),
            query.section_index,
        ),
    };
    let header = select_reference(
        Some(&section),
        true,
        query.page_ordinal,
        query.section_page_ordinal,
        section.title_page,
        even_and_odd,
    );
    let footer = select_reference(
        Some(&section),
        false,
        query.page_ordinal,
        query.section_page_ordinal,
        section.title_page,
        even_and_odd,
    );
    let header_height = measure_part(query.ctx, header, &query.geometry, env);
    let footer_height = measure_part(query.ctx, footer, &query.geometry, env);
    let extra_top = if header_height > 0.0 {
        (header_offset + header_height - query.geometry.top).max(0.0)
    } else {
        0.0
    };
    let extra_bottom = if footer_height > 0.0 {
        (footer_offset + footer_height - query.geometry.bottom).max(0.0)
    } else {
        0.0
    };
    PageRegionReserve {
        extra_top,
        extra_bottom,
        header_height,
        footer_height,
    }
}

fn measure_part(
    ctx: &LayoutContext<'_>,
    part: Option<&PartId>,
    geometry: &Geometry,
    env: FieldEnv,
) -> f64 {
    let Some(part) = part else {
        return 0.0;
    };
    let Some(header_footer) = ctx.document.header_footer(part) else {
        return 0.0;
    };
    let width = geometry.content_width();
    let dynamic = blocks_have_dynamic_fields(&header_footer.blocks);
    let key = (part.clone(), geometry.left.to_bits(), width.to_bits());
    if !dynamic {
        if let Some(height) = ctx.region_heights.borrow().get(&key).copied() {
            return height;
        }
    }
    let charged = ctx.render_items.get();
    let height = layout_region(ctx, &header_footer.blocks, geometry.left, width, Some(env)).height;
    ctx.render_items.set(charged);
    if !dynamic {
        ctx.region_heights.borrow_mut().insert(key, height);
    }
    height
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
    page_ordinal: usize,
    section_page_ordinal: usize,
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
    if title_page && section_page_ordinal == 1 {
        return find(HeaderFooterKind::First);
    }
    if even_and_odd && page_ordinal.is_multiple_of(2) {
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
    cache: &mut HashMap<(PartId, u64), CachedRegion>,
    dynamic: &mut HashMap<PartId, bool>,
) -> Option<Region> {
    let header_footer = ctx.document.header_footer(part)?;
    let is_dynamic = *dynamic
        .entry(part.clone())
        .or_insert_with(|| blocks_have_dynamic_fields(&header_footer.blocks));
    let key = (
        part.clone(),
        ctx.region_shift.get().unwrap_or(0.0).to_bits(),
    );
    if is_dynamic {
        Some(layout_region(
            ctx,
            &header_footer.blocks,
            left,
            width,
            Some(env),
        ))
    } else if let Some(cached) = cache.get(&key) {
        Some(Region {
            items: cached.region.items.clone(),
            height: cached.region.height,
            ink_bottom: cached.region.ink_bottom,
        })
    } else {
        let region = layout_region(ctx, &header_footer.blocks, left, width, None);
        cache.insert(
            key,
            CachedRegion {
                region: Region {
                    items: region.items.clone(),
                    height: region.height,
                    ink_bottom: region.ink_bottom,
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
    let mut page_frames = Vec::new();
    layout_blocks_inline(
        ctx,
        blocks,
        left,
        width,
        &mut y,
        &mut items,
        1,
        None,
        false,
        &mut page_frames,
    );
    ctx.field_env.set(None);
    // A header/footer of only floating objects has no flow height (`y` stays
    // 0). Its `ink_bottom` is where those objects actually reach, so a
    // bottom-anchored footer still lands with its content on the page
    // (CC0/019: a footer text box drew 15 pt under the page; CC0/020: 740 pt).
    // `height` stays the flow height: the body clears the text, not a stray
    // box that reaches far down, and using the ink here exploded the page count.
    //
    // Only ink that is *drawn* counts. An auto-fit VML text box keeps its
    // declared extent in its invisible rectangle (`fill="none"`, `stroked="f"`);
    // counting that rectangle lifted a footer whose text was fine right up into
    // the body (CC0_DOCX_MATH/095: a 144 pt page-number box over a 60 pt text
    // line, the flow text then overlapping the last body lines).
    let ink_bottom = items
        .iter()
        .filter(|item| paints_ink(item))
        .map(|item| crate::layout::table::item_vertical_bounds(ctx, item).1)
        .fold(y, f64::max);
    Region {
        items,
        height: y,
        ink_bottom,
    }
}

/// Whether an item puts any ink on the page: text, a picture or a line always
/// does; a shape only with a fill, gradient or stroke; a group as its children.
fn paints_ink(item: &Item) -> bool {
    match item {
        Item::Text(_) | Item::Image(_) | Item::Line(_) => true,
        Item::Rect(rect) => rect.fill.is_some() || rect.stroke.is_some(),
        Item::Path(path) => path.fill.is_some() || path.paint.is_some() || path.stroke.is_some(),
        Item::Group(group) => group.items.iter().any(paints_ink),
    }
}
