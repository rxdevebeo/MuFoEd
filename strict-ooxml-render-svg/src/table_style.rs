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
    static CELL_STYLES: RefCell<Vec<(Option<CellStyle>, bool)>> = const { RefCell::new(Vec::new()) };
    static WRAP_OVERRIDES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

/// Restores a drawing container's wrapping policy when it leaves scope.
pub(crate) struct WrapGuard;
impl Drop for WrapGuard {
    fn drop(&mut self) {
        WRAP_OVERRIDES.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}
thread_local! {
    static TEXT_FLOWS: RefCell<Vec<(strict_ooxml_wml::model::values::TextDirection, Option<f64>)>> = const { RefCell::new(Vec::new()) };
}
pub(crate) struct TextFlowGuard;
impl Drop for TextFlowGuard {
    fn drop(&mut self) {
        TEXT_FLOWS.with(|flows| {
            flows.borrow_mut().pop();
        });
    }
}
pub(crate) fn enter_text_flow(
    direction: strict_ooxml_wml::model::values::TextDirection,
    height: Option<f64>,
) -> TextFlowGuard {
    TEXT_FLOWS.with(|flows| flows.borrow_mut().push((direction, height)));
    TextFlowGuard
}
pub(crate) fn current_text_flow(
) -> Option<(strict_ooxml_wml::model::values::TextDirection, Option<f64>)> {
    TEXT_FLOWS.with(|flows| flows.borrow().last().copied())
}

/// Sets wrapping for a drawing's text container independently of its host cell.
pub(crate) fn wrap_override(no_wrap: bool) -> WrapGuard {
    WRAP_OVERRIDES.with(|stack| stack.borrow_mut().push(no_wrap));
    WrapGuard
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
pub(crate) fn enter_cell(table: &Table, conditions: u16, no_wrap: bool) -> CellStyleGuard {
    let entry = table
        .props
        .style
        .clone()
        .map(|style| CellStyle { style, conditions });
    CELL_STYLES.with(|stack| stack.borrow_mut().push((entry, no_wrap)));
    CellStyleGuard
}

/// The table style and condition mask of the innermost cell being laid out.
pub(crate) fn current_cell() -> Option<(StyleId, u16)> {
    CELL_STYLES.with(|stack| {
        stack
            .borrow()
            .last()
            .and_then(|(entry, _)| entry.as_ref())
            .map(|entry| (entry.style.clone(), entry.conditions))
    })
}

/// Whether the innermost cell suppresses soft line wrapping.
pub(crate) fn current_no_wrap() -> bool {
    WRAP_OVERRIDES
        .with(|stack| stack.borrow().last().copied())
        .unwrap_or_else(|| {
            CELL_STYLES.with(|stack| stack.borrow().last().is_some_and(|(_, no_wrap)| *no_wrap))
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

fn merge_table(
    target: &mut strict_ooxml_wml::model::props::TableProperties,
    layer: &strict_ooxml_wml::model::props::TableProperties,
) {
    macro_rules! overlay { ($($field:ident),*) => { $(if layer.$field.is_some() { target.$field.clone_from(&layer.$field); })* }; }
    overlay!(
        style,
        width,
        alignment,
        layout,
        shading,
        look,
        indent,
        indent_kind,
        positioning,
        allow_overlap,
        cell_spacing,
        style_row_band_size,
        style_col_band_size
    );
    merge_margins(&mut target.cell_margins, &layer.cell_margins);
    merge_borders(&mut target.borders, &layer.borders);
    if layer.bidi_visual_state != strict_ooxml_wml::model::values::TriState::Absent {
        target.bidi_visual_state = layer.bidi_visual_state;
        target.bidi_visual = layer.bidi_visual_state.is_on();
    } else if layer.bidi_visual {
        target.bidi_visual = true;
    }
}

fn merge_row(
    target: &mut strict_ooxml_wml::model::props::RowProperties,
    layer: &strict_ooxml_wml::model::props::RowProperties,
) {
    use strict_ooxml_wml::model::values::TriState;
    macro_rules! overlay { ($($field:ident),*) => { $(if layer.$field.is_some() { target.$field.clone_from(&layer.$field); })* }; }
    overlay!(
        height,
        grid_before,
        grid_after,
        width_before,
        width_after,
        alignment,
        cell_spacing,
        exception
    );
    if layer.header != TriState::Absent {
        target.header = layer.header;
    }
    if layer.cant_split_state != TriState::Absent {
        target.cant_split_state = layer.cant_split_state;
        target.cant_split = layer.cant_split_state.is_on();
    } else if layer.cant_split {
        target.cant_split = true;
    }
    merge_margins(&mut target.cell_margins, &layer.cell_margins);
    merge_borders(&mut target.exception_borders, &layer.exception_borders);
}

/// Resolves table and row properties before width and pagination decisions.
pub(crate) fn resolve_table(document: &Document, table: &Table) -> Table {
    let Some(style) = table_style(document, table) else {
        return table.clone();
    };
    let mut resolved = table.clone();
    let mut properties = strict_ooxml_wml::model::props::TableProperties::default();
    for ancestor in chain(document, style) {
        merge_table(&mut properties, &ancestor.table);
    }
    for condition in conditions(document, style, WHOLE_TABLE) {
        merge_table(&mut properties, &condition.table);
    }
    merge_table(&mut properties, &table.props);
    resolved.props = properties;
    let columns = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| usize::from(cell.props.grid_span.unwrap_or(1)).max(1))
                .sum::<usize>()
                .saturating_add(row.props.grid_before.unwrap_or(0) as usize)
                .saturating_add(row.props.grid_after.unwrap_or(0) as usize)
        })
        .max()
        .unwrap_or(1);
    for (index, row) in resolved.rows.iter_mut().enumerate() {
        let mask = cell_conditions(document, table, index, 0, columns, columns)
            & (WHOLE_TABLE | BAND1_HORZ | BAND2_HORZ | FIRST_ROW | LAST_ROW);
        let mut properties = strict_ooxml_wml::model::props::RowProperties::default();
        let mut exception = strict_ooxml_wml::model::props::TableProperties::default();
        for ancestor in chain(document, style) {
            merge_row(&mut properties, &ancestor.row);
        }
        for condition in conditions(document, style, mask) {
            merge_row(&mut properties, &condition.row);
            merge_table(&mut exception, &condition.table);
        }
        if let Some(direct) = row.props.exception.as_deref() {
            merge_table(&mut exception, direct);
        }
        merge_borders(&mut properties.exception_borders, &exception.borders);
        merge_margins(&mut properties.cell_margins, &exception.cell_margins);
        if exception.cell_spacing.is_some() {
            properties.cell_spacing = exception.cell_spacing;
        }
        if exception.alignment.is_some() {
            properties.alignment = exception.alignment;
        }
        merge_row(&mut properties, &row.props);
        properties.exception = Some(Box::new(exception));
        row.props = properties;
        let mut column = usize::try_from(row.props.grid_before.unwrap_or(0)).unwrap_or(0);
        for cell in &mut row.cells {
            let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
            let mask = cell_conditions(document, table, index, column, span, columns);
            let mut defaults = strict_ooxml_wml::model::props::TableProperties::default();
            for ancestor in chain(document, style) {
                merge_table(&mut defaults, &ancestor.table);
            }
            for condition in conditions(document, style, mask) {
                merge_table(&mut defaults, &condition.table);
            }
            merge_table(&mut defaults, &table.props);
            if let Some(exception) = table.rows[index].props.exception.as_deref() {
                merge_table(&mut defaults, exception);
            }
            let mut effective = CellProperties::default();
            effective.margins = defaults.cell_margins;
            effective.shading = defaults.shading;
            effective.borders.top = if index == 0 {
                defaults.borders.top.clone()
            } else {
                defaults.borders.inside_horizontal.clone()
            };
            effective.borders.bottom = if index + 1 == table.rows.len() {
                defaults.borders.bottom.clone()
            } else {
                defaults.borders.inside_horizontal.clone()
            };
            effective.borders.start = if column == 0 {
                defaults.borders.start.clone()
            } else {
                defaults.borders.inside_vertical.clone()
            };
            effective.borders.end = if column + span >= columns {
                defaults.borders.end.clone()
            } else {
                defaults.borders.inside_vertical.clone()
            };
            merge_cell(&mut effective, &style_cell_layer(document, style, mask));
            merge_margins(
                &mut effective.margins,
                &table.rows[index].props.cell_margins,
            );
            merge_cell(&mut effective, &cell.props);
            effective.location = cell.props.location.clone();
            effective.grid_span = cell.props.grid_span;
            effective.vertical_merge = cell.props.vertical_merge;
            cell.props = effective;
            column += span;
        }
    }
    resolved
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
    if layer.width.is_some() {
        target.width = layer.width;
    }
    if layer.hide_mark_state != strict_ooxml_wml::model::values::TriState::Absent {
        target.hide_mark_state = layer.hide_mark_state;
        target.hide_mark = layer.hide_mark_state.is_on();
    } else if layer.hide_mark {
        target.hide_mark = true;
    }
    if layer.fit_text_state != strict_ooxml_wml::model::values::TriState::Absent {
        target.fit_text_state = layer.fit_text_state;
        target.fit_text = layer.fit_text_state.is_on();
    } else if layer.fit_text {
        target.fit_text = true;
    }
    if layer.no_wrap_state != strict_ooxml_wml::model::values::TriState::Absent {
        target.no_wrap_state = layer.no_wrap_state;
        target.no_wrap = layer.no_wrap_state.is_on();
    } else if layer.no_wrap {
        target.no_wrap = true;
    }
    if layer.diagonal_down.is_some() {
        target.diagonal_down = layer.diagonal_down.clone();
    }
    if layer.diagonal_up.is_some() {
        target.diagonal_up = layer.diagonal_up.clone();
    }
    if layer.shading.is_some() {
        target.shading.clone_from(&layer.shading);
    }
    merge_borders(&mut target.borders, &layer.borders);
    merge_margins(&mut target.margins, &layer.margins);
    if layer.vertical_align.is_some() {
        target.vertical_align = layer.vertical_align;
    }
    if layer.text_direction.is_some() {
        target.text_direction = layer.text_direction;
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
    let mut resolved = table_style(document, table).map_or_else(CellProperties::default, |style| {
        style_cell_layer(document, style, mask)
    });
    // Structural properties (merge/span/width) remain those of the cell.
    if direct.width.is_some() {
        resolved.width = direct.width;
    }
    resolved.grid_span = direct.grid_span;
    resolved.location = direct.location.clone();
    resolved.vertical_merge = direct.vertical_merge;
    merge_cell(&mut resolved, direct);
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
    for condition in conditions(document, style, mask) {
        merge_margins(&mut margins, &condition.table.cell_margins);
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
