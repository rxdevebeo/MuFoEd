//! Table styles in the render cascade: the base `w:tblPr`/`w:tcPr`/`w:pPr`/
//! `w:rPr` of a `w:tblStyle`, and its `w:tblStylePr` conditional formatting
//! (ISO/IEC 29500-1 §17.7.6, `ST_TblStyleOverrideType`).
//!
//! Which conditions reach a cell is a bit mask ([`cell_conditions`]): the cell's
//! position, `w:tblLook`, and `w:tblStyleRowBandSize`/`w:tblStyleColBandSize`.
//! Bits are applied in [`KINDS`] order, later wins, which is Word's precedence:
//! whole table, vertical bands, horizontal bands, first/last column, first/last
//! row, corner cells. A `basedOn` chain contributes too, furthest ancestor
//! first, so a child's condition overrides its parent's of the same kind.
//!
//! The mask is a `u16` so the recursive table layout carries no more than that:
//! a cell's resolved properties are built in `#[inline(never)]` helpers here.
//!
//! Cell paragraphs see the table style through [`enter_cell`]: a scoped entry
//! on a per-thread stack, read by `style::compute_paragraph`, which applies the
//! style after `w:docDefaults` and before the paragraph style (§17.7.2).

use std::cell::RefCell;

use strict_ooxml_wml::model::props::CellProperties;
use strict_ooxml_wml::model::styles::TableStyleCondition;
use strict_ooxml_wml::model::values::{Border, Borders, CellMargins, TableLook};
use strict_ooxml_wml::model::{Document, Style, StyleId, Table};

/// `ST_TblStyleOverrideType` values in application order (later wins).
///
/// Bit `i` of a condition mask selects `KINDS[i]`.
pub(crate) static KINDS: [&str; 13] = [
    "wholeTable",
    "band1Vert",
    "band2Vert",
    "band1Horz",
    "band2Horz",
    "firstCol",
    "lastCol",
    "firstRow",
    "lastRow",
    "nwCell",
    "neCell",
    "swCell",
    "seCell",
];

const WHOLE_TABLE: u16 = 1;
const BAND1_VERT: u16 = 1 << 1;
const BAND2_VERT: u16 = 1 << 2;
const BAND1_HORZ: u16 = 1 << 3;
const BAND2_HORZ: u16 = 1 << 4;
const FIRST_COL: u16 = 1 << 5;
const LAST_COL: u16 = 1 << 6;
const FIRST_ROW: u16 = 1 << 7;
const LAST_ROW: u16 = 1 << 8;
const NW_CELL: u16 = 1 << 9;
const NE_CELL: u16 = 1 << 10;
const SW_CELL: u16 = 1 << 11;
const SE_CELL: u16 = 1 << 12;

/// The table style a cell's paragraphs are laid out under.
struct CellStyle {
    style: StyleId,
    conditions: u16,
}

thread_local! {
    /// Innermost cell last. `None` is a cell of a table without a style, which
    /// must shadow the style of the table around it.
    static CELL_STYLES: RefCell<Vec<Option<CellStyle>>> = const { RefCell::new(Vec::new()) };
}

/// Leaves the cell entered by [`enter_cell`] when dropped.
#[must_use = "the cell style is popped when the guard is dropped"]
pub(crate) struct CellStyleGuard;

impl Drop for CellStyleGuard {
    fn drop(&mut self) {
        CELL_STYLES.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Makes `table`'s style, narrowed to `conditions`, the style of the cell
/// paragraphs laid out until the guard drops.
#[inline(never)]
pub(crate) fn enter_cell(table: &Table, conditions: u16) -> CellStyleGuard {
    let entry = table
        .props
        .style
        .clone()
        .map(|style| CellStyle { style, conditions });
    CELL_STYLES.with(|stack| stack.borrow_mut().push(entry));
    CellStyleGuard
}

/// The table style and condition mask of the innermost cell being laid out.
pub(crate) fn current_cell() -> Option<(StyleId, u16)> {
    CELL_STYLES.with(|stack| {
        stack
            .borrow()
            .last()
            .and_then(Option::as_ref)
            .map(|entry| (entry.style.clone(), entry.conditions))
    })
}

/// `style` and its `basedOn` ancestors, furthest ancestor first.
pub(crate) fn chain<'a>(
    document: &'a Document,
    style: &'a Style,
) -> impl DoubleEndedIterator<Item = &'a Style> + 'a {
    style
        .based_on_chain
        .iter()
        .rev()
        .filter_map(move |id| document.styles.get(id))
        .chain(std::iter::once(style))
}

/// The conditions `mask` selects, in application order: by [`KINDS`] order,
/// then furthest ancestor first within one kind.
pub(crate) fn conditions<'a>(
    document: &'a Document,
    style: &'a Style,
    mask: u16,
) -> impl Iterator<Item = &'a TableStyleCondition> + 'a {
    KINDS
        .iter()
        .enumerate()
        .filter(move |(bit, _)| mask & (1u16 << *bit) != 0)
        .flat_map(move |(_, kind)| {
            chain(document, style).filter_map(move |layer| {
                layer
                    .conditions
                    .iter()
                    .rev()
                    .find(|condition| &*condition.kind == *kind)
            })
        })
}

/// The table's style, when it names one that exists.
fn table_style<'a>(document: &'a Document, table: &Table) -> Option<&'a Style> {
    table
        .props
        .style
        .as_ref()
        .and_then(|id| document.styles.get(id))
}

/// Rows or columns per band: direct, then the style chain, then `1`.
fn band_size(
    table: &Table,
    styles: &[&Style],
    pick: fn(&strict_ooxml_wml::model::props::TableProperties) -> Option<i32>,
) -> usize {
    pick(&table.props)
        .or_else(|| styles.iter().rev().find_map(|style| pick(&style.table)))
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size > 0)
        .unwrap_or(1)
}

/// The conditions that apply to the cell at grid `column` (spanning `span` of
/// `columns`) in row `row`, as a mask over [`KINDS`]. `0` without a table style.
///
/// `w:tblLook` comes from the table, else the style chain, else all off. Row
/// banding skips a first/last row that has its own formatting; column banding
/// skips a first/last column likewise.
#[inline(never)]
pub(crate) fn cell_conditions(
    document: &Document,
    table: &Table,
    row: usize,
    column: usize,
    span: usize,
    columns: usize,
) -> u16 {
    let Some(style) = table_style(document, table) else {
        return 0;
    };
    let styles: Vec<&Style> = chain(document, style).collect();
    let look: TableLook = table
        .props
        .look
        .or_else(|| styles.iter().rev().find_map(|style| style.table.look))
        .unwrap_or_default();
    let row_band = band_size(table, &styles, |props| props.style_row_band_size);
    let col_band = band_size(table, &styles, |props| props.style_col_band_size);
    let first_row = look.first_row && row == 0;
    let last_row = look.last_row && row + 1 == table.rows.len();
    let first_col = look.first_column && column == 0;
    let last_col = look.last_column && column.saturating_add(span) >= columns;

    let mut mask = WHOLE_TABLE;
    if !look.no_v_band && !first_col && !last_col {
        let index = column.saturating_sub(usize::from(look.first_column));
        mask |= if (index / col_band).is_multiple_of(2) {
            BAND1_VERT
        } else {
            BAND2_VERT
        };
    }
    if !look.no_h_band && !first_row && !last_row {
        let index = row.saturating_sub(usize::from(look.first_row));
        mask |= if (index / row_band).is_multiple_of(2) {
            BAND1_HORZ
        } else {
            BAND2_HORZ
        };
    }
    for (on, bit) in [
        (first_col, FIRST_COL),
        (last_col, LAST_COL),
        (first_row, FIRST_ROW),
        (last_row, LAST_ROW),
        (first_row && first_col, NW_CELL),
        (first_row && last_col, NE_CELL),
        (last_row && first_col, SW_CELL),
        (last_row && last_col, SE_CELL),
    ] {
        if on {
            mask |= bit;
        }
    }
    mask
}

/// Overlays the set fields of `layer` onto `target`.
fn merge_borders(target: &mut Borders, layer: &Borders) {
    for (slot, edge) in [
        (&mut target.top, &layer.top),
        (&mut target.bottom, &layer.bottom),
        (&mut target.start, &layer.start),
        (&mut target.end, &layer.end),
        (&mut target.inside_horizontal, &layer.inside_horizontal),
        (&mut target.inside_vertical, &layer.inside_vertical),
    ] {
        if edge.is_some() {
            slot.clone_from(edge);
        }
    }
}

/// Overlays the set sides of `layer` onto `target`.
fn merge_margins(target: &mut CellMargins, layer: &CellMargins) {
    for (slot, side) in [
        (&mut target.top, layer.top),
        (&mut target.start, layer.start),
        (&mut target.bottom, layer.bottom),
        (&mut target.end, layer.end),
    ] {
        if side.is_some() {
            *slot = side;
        }
    }
}

/// Overlays the style-carried cell properties of `layer` onto `target`.
fn merge_cell(target: &mut CellProperties, layer: &CellProperties) {
    if layer.shading.is_some() {
        target.shading.clone_from(&layer.shading);
    }
    merge_borders(&mut target.borders, &layer.borders);
    merge_margins(&mut target.margins, &layer.margins);
    if layer.vertical_align.is_some() {
        target.vertical_align = layer.vertical_align;
    }
}

/// The cell properties the table style gives a cell under `mask`: base `tcPr`
/// down the chain, then each condition's `tcPr` in order.
fn style_cell_layer(document: &Document, style: &Style, mask: u16) -> CellProperties {
    let mut layer = CellProperties::default();
    for ancestor in chain(document, style) {
        merge_cell(&mut layer, &ancestor.cell);
    }
    for condition in conditions(document, style, mask) {
        merge_cell(&mut layer, &condition.cell);
    }
    layer
}

/// A cell's effective properties: the style layer under `mask`, then the cell's
/// own `tcPr`, which wins wherever it says something.
#[inline(never)]
pub(crate) fn resolve_cell_properties(
    document: &Document,
    table: &Table,
    mask: u16,
    direct: &CellProperties,
) -> CellProperties {
    let mut resolved = direct.clone();
    let Some(style) = table_style(document, table) else {
        return resolved;
    };
    let layer = style_cell_layer(document, style, mask);
    if resolved.shading.is_none() {
        resolved.shading = layer.shading;
    }
    let mut borders = layer.borders;
    merge_borders(&mut borders, &direct.borders);
    resolved.borders = borders;
    if resolved.vertical_align.is_none() {
        resolved.vertical_align = layer.vertical_align;
    }
    resolved
}

/// Cell margins the table style supplies for a cell under `mask`: `tcMar` of the
/// style layer over the chain's `tblCellMar`. Default when there is no style.
#[inline(never)]
pub(crate) fn style_cell_margins(document: &Document, table: &Table, mask: u16) -> CellMargins {
    let Some(style) = table_style(document, table) else {
        return CellMargins::default();
    };
    let mut margins = CellMargins::default();
    for ancestor in chain(document, style) {
        merge_margins(&mut margins, &ancestor.table.cell_margins);
    }
    merge_margins(
        &mut margins,
        &style_cell_layer(document, style, mask).margins,
    );
    margins
}

/// One edge of the table's `w:tblBorders` (`0` top, `1` bottom, `2` start,
/// `3` end): direct, else the nearest style in the chain that sets it.
pub(crate) fn table_border<'a>(
    document: &'a Document,
    table: &'a Table,
    edge: u8,
) -> Option<&'a Border> {
    // `0` top, `1` bottom, `2` start, `3` end; `4` insideH and `5` insideV
    // for an edge between two cells.
    let pick = |borders: &'a Borders| match edge {
        0 => borders.top.as_ref(),
        1 => borders.bottom.as_ref(),
        2 => borders.start.as_ref(),
        4 => borders.inside_horizontal.as_ref(),
        5 => borders.inside_vertical.as_ref(),
        _ => borders.end.as_ref(),
    };
    pick(&table.props.borders).or_else(|| {
        let style = table_style(document, table)?;
        chain(document, style)
            .rev()
            .find_map(|layer| pick(&layer.table.borders))
    })
}

#[cfg(test)]
mod tests {
    use super::{FIRST_ROW, KINDS, SE_CELL, WHOLE_TABLE};

    #[test]
    fn mask_bits_name_their_kinds() {
        let bit = |kind: &str| {
            let index = KINDS
                .iter()
                .position(|known| *known == kind)
                .expect("known kind");
            1u16 << index
        };
        assert_eq!(bit("wholeTable"), WHOLE_TABLE);
        assert_eq!(bit("firstRow"), FIRST_ROW);
        assert_eq!(bit("seCell"), SE_CELL);
    }
}
