//! Per-page section features applied after pagination: vertical alignment of
//! the page (`w:sectPr/w:vAlign`), the line between columns (`w:cols/@w:sep`),
//! line numbers (`w:lnNumType`) and mirrored margins (`w:mirrorMargins`).
//!
//! Each needs the finished page: the extent of its content, or which body lines
//! landed on it.

use strict_ooxml_core::error::Result;
use strict_ooxml_wml::model::props::{LineNumbering, Section};
use strict_ooxml_wml::model::values::{LineNumberRestart, VerticalJc};

use crate::layout::{
    geometry_for, ColumnRegion, Item, LayoutContext, LineItem, LineMark, PlacedPage, TextItem,
};
use crate::style::ComputedRun;
use crate::units::twips_to_px;

/// Default distance between a line number and its text (`auto`): 0.25 inch.
const LINE_NUMBER_DISTANCE_TWIPS: i32 = 360;

/// Applies vertical alignment, column separators and line numbers to the body
/// of each page. Runs before headers and footers are added, so "the content"
/// is the body.
pub(crate) fn apply(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    marks: &mut [LineMark],
    regions: &mut [ColumnRegion],
    sections: &[Section],
) -> Result<()> {
    let scale = ctx.options.scale;
    for (index, page) in pages.iter_mut().enumerate() {
        let Some(section) = sections.get(page.section_index).map(|s| &s.properties) else {
            continue;
        };
        let geometry = crate::layout::body_geometry_for(
            Some(section),
            scale,
            None,
            ctx.document.settings.gutter_at_top,
        );
        let body_top = geometry.top;
        let body_bottom = page.height_px - geometry.bottom;
        let shift = match section.vertical_align {
            Some(VerticalJc::Center | VerticalJc::Bottom) => {
                let content_bottom = content_bottom(page, body_top, body_bottom);
                let free = (body_bottom - content_bottom).max(0.0);
                if matches!(section.vertical_align, Some(VerticalJc::Center)) {
                    free / 2.0
                } else {
                    free
                }
            }
            _ => 0.0,
        };
        if shift > 0.0 {
            for item in &mut page.items {
                *item = crate::layout::table::offset_item(item, 0.0, shift);
            }
            for mark in marks.iter_mut().filter(|mark| mark.page == index) {
                mark.baseline += shift;
            }
            for region in regions.iter_mut().filter(|region| region.page == index) {
                region.top += shift;
                region.bottom += shift;
            }
        }
        let separators = column_separators(regions, index);
        ctx.charge_items(separators.len())?;
        page.items.extend(separators);
    }
    line_numbers(ctx, pages, marks, sections)
}

/// Restore physical page coordinates before adding horizontal headers/footers.
pub(crate) fn orient_body(ctx: &LayoutContext<'_>, pages: &mut [PlacedPage], sections: &[Section]) {
    use strict_ooxml_wml::model::values::TextDirection;
    for page in pages {
        let properties = sections.get(page.section_index).map(|s| &s.properties);
        let direction = properties.and_then(|s| s.text_direction);
        let physical = geometry_for(
            properties,
            ctx.options.scale,
            None,
            ctx.document.settings.gutter_at_top,
        );
        let matrix = match direction {
            Some(TextDirection::TbRl | TextDirection::TbRlV) => {
                Some([0.0, 1.0, -1.0, 0.0, physical.width, 0.0])
            }
            Some(TextDirection::BtLr) => Some([0.0, -1.0, 1.0, 0.0, 0.0, physical.height]),
            Some(TextDirection::TbLrV) => None,
            _ => None,
        };
        let mut items = std::mem::take(&mut page.items);
        if matches!(
            direction,
            Some(TextDirection::TbRlV | TextDirection::TbLrV | TextDirection::LrTbV)
        ) {
            items = crate::layout::table::rotate_east_asian_items(ctx, items);
        }
        page.items = if direction == Some(TextDirection::TbLrV) {
            items
                .into_iter()
                .flat_map(|item| {
                    let center = match &item {
                        Item::Text(text) => text.baseline,
                        _ => {
                            let (top, bottom) =
                                crate::layout::table::item_vertical_bounds(ctx, &item);
                            (top + bottom) / 2.0
                        }
                    };
                    crate::paint::graphics::transform_items(
                        vec![item],
                        [0.0, 1.0, -1.0, 0.0, 2.0 * center, 0.0],
                    )
                })
                .collect()
        } else if let Some(matrix) = matrix {
            crate::paint::graphics::transform_items(items, matrix)
        } else {
            items
        };
        page.width_px = physical.width;
        page.height_px = physical.height;
    }
}

/// The lowest painted point of the body, px from the page top.
fn content_bottom(page: &PlacedPage, top: f64, bottom: f64) -> f64 {
    page.items
        .iter()
        .map(|item| match item {
            Item::Group(group) => group.bounds[1] + group.bounds[3],
            Item::Text(text) => text.baseline + text.size_px * 0.25,
            Item::Rect(rect) => rect.y + rect.h,
            Item::Line(line) => line.y1.max(line.y2),
            Item::Path(path) => path.y + path.h,
            Item::Image(image) => image.y + image.h,
        })
        .filter(|y| *y > top && *y <= bottom + 1e-6)
        .fold(top, f64::max)
}

/// Lines between the columns of a `w:cols/@w:sep` section (ISO/IEC 29500-1
/// §17.6.4): in the middle of each gap, from the top of the columns to the
/// bottom of the longest one, for every multi-column area of page `page`.
fn column_separators(regions: &[ColumnRegion], page: usize) -> Vec<Item> {
    regions
        .iter()
        .filter(|region| region.page == page && region.columns.separator)
        .flat_map(|region| {
            region.columns.gap_centres().into_iter().map(|offset| {
                let x = region.left + offset;
                Item::Line(LineItem {
                    x1: x,
                    y1: region.top,
                    x2: x,
                    y2: region.bottom,
                    color: "#000000".to_owned(),
                    width: 0.75,
                    dashed: false,
                })
            })
        })
        .collect()
}

/// Paints `w:lnNumType` numbers in the margin beside the body lines.
fn line_numbers(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    marks: &[LineMark],
    sections: &[Section],
) -> Result<()> {
    if !sections
        .iter()
        .any(|section| section.properties.line_numbering.is_some())
    {
        return Ok(());
    }
    let run = line_number_run(ctx);
    let size_px = ctx.size_px(run.size_pt);
    let mut count: u32 = 0;
    let mut last: Option<(usize, usize)> = None;
    for mark in marks {
        let Some(numbering) = sections
            .get(mark.section)
            .and_then(|section| section.properties.line_numbering)
        else {
            last = None;
            continue;
        };
        let restart = match (last, numbering.restart) {
            (None, _) => true,
            (Some((_, section)), _) if section != mark.section => {
                !matches!(numbering.restart, Some(LineNumberRestart::Continuous))
            }
            (Some((page, _)), None | Some(LineNumberRestart::NewPage)) => page != mark.page,
            _ => false,
        };
        if restart {
            count = u32::from(numbering.start.unwrap_or(0));
        }
        count = count.saturating_add(1);
        last = Some((mark.page, mark.section));
        if !count.is_multiple_of(u32::from(numbering.count_by.unwrap_or(1).max(1))) {
            continue;
        }
        let Some(page) = pages.get_mut(mark.page) else {
            continue;
        };
        let text = count.to_string();
        let width = ctx.measure(&text, &run);
        let distance = distance_px(&numbering, ctx.options.scale);
        ctx.charge_items(1)?;
        page.items.push(Item::Text(TextItem {
            x: mark.column_left - distance - width,
            baseline: mark.baseline,
            width,
            text: text.clone(),
            run: run.clone(),
            size_px,
            advance: ctx.advance_kind_for(&text, &run),
            field: None,
            compress_punctuation: false,
            following_non_space: None,
            plain_space_factor: 1.0,
        }));
    }
    Ok(())
}

fn distance_px(numbering: &LineNumbering, scale: f64) -> f64 {
    let twips = numbering
        .distance
        .map(strict_ooxml_wml::model::values::Twips::value)
        .filter(|value| *value > 0)
        .unwrap_or(LINE_NUMBER_DISTANCE_TWIPS);
    twips_to_px(twips, scale)
}

/// The format of the numbers: the document defaults, then the `LineNumber`
/// character style when the document has one.
fn line_number_run(ctx: &LayoutContext<'_>) -> ComputedRun {
    let theme = ctx.document.theme.as_ref();
    let mut run = ComputedRun::default();
    if let Some(defaults) = ctx.document.styles.defaults() {
        crate::style::apply_run_props(&mut run, &defaults.run, theme);
    }
    let id = strict_ooxml_wml::model::StyleId::new("LineNumber");
    if let Some(style) = ctx.document.styles.get(&id) {
        crate::style::apply_run_props(&mut run, &style.run, theme);
    }
    run
}

/// Moves even pages of `w:mirrorMargins` documents so their inside margin is
/// on the right: the content box takes the odd page's right margin on its
/// left. Runs after headers and footers, which follow the margins too.
pub(crate) fn mirror_margins(
    ctx: &LayoutContext<'_>,
    pages: &mut [PlacedPage],
    sections: &[Section],
) {
    if !ctx.document.settings.mirror_margins {
        return;
    }
    let scale = ctx.options.scale;
    for (index, page) in pages.iter_mut().enumerate() {
        // Index 0 is page 1, a right-hand (odd) page.
        if index % 2 == 0 {
            continue;
        }
        let section = sections.get(page.section_index).map(|s| &s.properties);
        let geometry = geometry_for(section, scale, None, ctx.document.settings.gutter_at_top);
        let dx = geometry.right - geometry.left;
        if dx.abs() < 1e-9 {
            continue;
        }
        for item in &mut page.items {
            *item = crate::layout::table::offset_item(item, dx, 0.0);
        }
    }
}
