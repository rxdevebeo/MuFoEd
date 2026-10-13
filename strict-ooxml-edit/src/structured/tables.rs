//! Merging and splitting cells on a table's logical grid (K09).
//!
//! A cell occupies a region of the grid: the grid columns its `w:gridSpan`
//! covers, from the column after the cells and `w:gridBefore` in front of it,
//! and the rows its `w:vMerge` restart is continued over. Both commands work
//! on regions, so a merged cell is addressed by any grid position inside it.
use crate::{EditError, Invariant};
use std::collections::HashMap;
use std::ops::Range;
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::{
    Block, CellProperties, GridCol, Paragraph, ParagraphProperties, Rsids, Table, TableCell,
    TableRow, Twips, VerticalMerge, Width, WidthKind,
};

/// The most columns or rows one split makes, as in Word.
const MAX_PARTS: usize = 63;

/// The grid region of one cell (with the cells its vertical merge continues).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    row: usize,
    column: usize,
    span: usize,
    height: usize,
}
impl Region {
    fn columns(&self) -> Range<usize> {
        self.column..self.column + self.span
    }
    fn rows(&self) -> Range<usize> {
        self.row..self.row + self.height
    }
}

/// One cell of one row: its index in the row, first grid column, span and the
/// region it belongs to.
#[derive(Clone, Copy, Debug)]
struct Slot {
    index: usize,
    column: usize,
    span: usize,
    region: usize,
}

/// Every region of a table, and where each row's cells sit.
struct Layout {
    regions: Vec<Region>,
    rows: Vec<Vec<Slot>>,
}
impl Layout {
    fn of(table: &Table) -> Result<Self, EditError> {
        let topology = || EditError::InvalidModel(Invariant::TableTopology);
        let mut regions: Vec<Region> = Vec::new();
        let mut rows = Vec::with_capacity(table.rows.len());
        let mut open: HashMap<(usize, usize), usize> = HashMap::new();
        for (r, row) in table.rows.iter().enumerate() {
            let mut column =
                usize::try_from(row.props.grid_before.unwrap_or(0)).map_err(|_| topology())?;
            let mut next = HashMap::new();
            let mut slots = Vec::with_capacity(row.cells.len());
            for (index, cell) in row.cells.iter().enumerate() {
                let span = usize::from(cell.props.grid_span.unwrap_or(1)).max(1);
                let region = if cell.props.vertical_merge == Some(VerticalMerge::Continue) {
                    let id = *open.get(&(column, span)).ok_or_else(topology)?;
                    if let Some(region) = regions.get_mut(id) {
                        region.height += 1;
                    }
                    next.insert((column, span), id);
                    id
                } else {
                    regions.push(Region {
                        row: r,
                        column,
                        span,
                        height: 1,
                    });
                    let id = regions.len() - 1;
                    if cell.props.vertical_merge == Some(VerticalMerge::Restart) {
                        next.insert((column, span), id);
                    }
                    id
                };
                slots.push(Slot {
                    index,
                    column,
                    span,
                    region,
                });
                column += span;
            }
            rows.push(slots);
            open = next;
        }
        Ok(Self { regions, rows })
    }

    fn region(&self, id: usize) -> Result<Region, EditError> {
        self.regions
            .get(id)
            .copied()
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))
    }

    /// The region covering grid position (`row`, `column`).
    fn at(&self, row: usize, column: usize) -> Result<usize, EditError> {
        self.rows
            .get(row)
            .and_then(|slots| {
                slots
                    .iter()
                    .find(|slot| (slot.column..slot.column + slot.span).contains(&column))
            })
            .map(|slot| slot.region)
            .ok_or(EditError::InvalidRange)
    }

    /// The cell of `region` in `row`.
    fn slot(&self, row: usize, region: usize) -> Result<Slot, EditError> {
        self.rows
            .get(row)
            .and_then(|slots| slots.iter().find(|slot| slot.region == region))
            .copied()
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))
    }
}

fn empty_paragraph() -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![],
        rsids: Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::unknown(),
    }
}

/// A cell holding only an empty paragraph adds nothing to a merge.
fn is_empty(cell: &TableCell) -> bool {
    matches!(cell.blocks.as_slice(), [Block::Paragraph(p)] if p.inlines.is_empty())
}

/// A cell that continues `props`' vertical merge.
fn continuation(props: &CellProperties) -> TableCell {
    let mut props = props.clone();
    props.vertical_merge = Some(VerticalMerge::Continue);
    TableCell {
        props,
        blocks: vec![Block::Paragraph(empty_paragraph())],
        sdt: None,
        location: SourceLocation::unknown(),
    }
}

fn span_of(columns: usize) -> Result<Option<u16>, EditError> {
    if columns <= 1 {
        return Ok(None);
    }
    u16::try_from(columns)
        .map(Some)
        .map_err(|_| EditError::LimitExceeded)
}

/// The width of grid columns `columns` in twips, when the grid gives each one.
fn grid_width(table: &Table, columns: &Range<usize>) -> Option<Width> {
    let mut total: i32 = 0;
    for column in columns.clone() {
        let width = table.grid.get(column)?.width?;
        total = total.checked_add(width.value())?;
    }
    Some(Width {
        kind: WidthKind::Dxa,
        value: Some(total),
    })
}

fn cell_mut(table: &mut Table, row: usize, index: usize) -> Result<&mut TableCell, EditError> {
    table
        .rows
        .get_mut(row)
        .and_then(|r| r.cells.get_mut(index))
        .ok_or(EditError::InvalidModel(Invariant::TableTopology))
}

/// Merges the cells covering `rows` × `columns` of the grid into one.
///
/// The top-left cell keeps its properties and receives the content of every
/// cell in reading order (cells holding only an empty paragraph add none);
/// the rows below hold cells that continue its vertical merge. A cell that
/// sticks out of the rectangle, or a gap in it, refuses the merge; a
/// rectangle that already is one cell is left as it is.
pub(super) fn merge_cells(
    table: &mut Table,
    rows: &Range<usize>,
    columns: &Range<usize>,
) -> Result<(), EditError> {
    if rows.is_empty() || columns.is_empty() || rows.end > table.rows.len() {
        return Err(EditError::InvalidRange);
    }
    let layout = Layout::of(table)?;
    let overlaps = |region: &Region| {
        region.row < rows.end
            && rows.start < region.row + region.height
            && region.column < columns.end
            && columns.start < region.column + region.span
    };
    let mut area = 0;
    let mut count = 0;
    for region in layout.regions.iter().filter(|region| overlaps(region)) {
        let inside = rows.start <= region.row
            && region.row + region.height <= rows.end
            && columns.start <= region.column
            && region.column + region.span <= columns.end;
        if !inside {
            return Err(EditError::InvalidRange);
        }
        area += region.span * region.height;
        count += 1;
    }
    if area != rows.len() * columns.len() {
        return Err(EditError::InvalidRange);
    }
    if count == 1 {
        return Ok(());
    }
    let top_left = layout.slot(rows.start, layout.at(rows.start, columns.start)?)?;
    let mut blocks = Vec::new();
    for r in rows.clone() {
        for slot in layout.rows.get(r).into_iter().flatten() {
            if !columns.contains(&slot.column) {
                continue;
            }
            let cell = table
                .rows
                .get(r)
                .and_then(|row| row.cells.get(slot.index))
                .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
            if !is_empty(cell) {
                blocks.extend(cell.blocks.iter().cloned());
            }
        }
    }
    let first = cell_mut(table, rows.start, top_left.index)?.clone();
    if blocks.is_empty() {
        blocks.clone_from(&first.blocks);
    }
    if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
        blocks.push(Block::Paragraph(empty_paragraph()));
    }
    let mut props = first.props.clone();
    props.grid_span = span_of(columns.len())?;
    props.vertical_merge = (rows.len() > 1).then_some(VerticalMerge::Restart);
    if let Some(width) = grid_width(table, columns) {
        props.width = Some(width);
    }
    for (k, r) in rows.clone().enumerate() {
        let inside: Vec<usize> = layout
            .rows
            .get(r)
            .into_iter()
            .flatten()
            .filter(|slot| columns.contains(&slot.column))
            .map(|slot| slot.index)
            .collect();
        let (Some(&from), Some(&to)) = (inside.first(), inside.last()) else {
            return Err(EditError::InvalidModel(Invariant::TableTopology));
        };
        let cell = if k == 0 {
            TableCell {
                props: props.clone(),
                blocks: std::mem::take(&mut blocks),
                sdt: first.sdt.clone(),
                location: first.location.clone(),
            }
        } else {
            continuation(&props)
        };
        let row = table
            .rows
            .get_mut(r)
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
        if to >= row.cells.len() || from > to {
            return Err(EditError::InvalidModel(Invariant::TableTopology));
        }
        row.cells.splice(from..=to, std::iter::once(cell));
    }
    Ok(())
}

/// Splits `count` into `parts` sizes as equal as can be, the larger first.
fn shares(count: usize, parts: usize) -> Vec<usize> {
    let base = count / parts;
    let extra = count % parts;
    (0..parts).map(|i| base + usize::from(i < extra)).collect()
}

/// Splits the cell covering grid position (`row`, `column`) into `columns` ×
/// `rows` cells.
///
/// The parts share the cell's grid columns and rows as evenly as they divide;
/// when there are more parts than grid columns, the cell's columns are cut
/// into equal ones (the last takes the rest, so the total width stays) and
/// the cells above and below that cover them widen; when there are more parts
/// than rows, rows are added below the cell and the other cells of its last
/// row continue down through them. The top-left part keeps the content, the
/// others hold an empty paragraph.
pub(super) fn split_cell(
    table: &mut Table,
    row: usize,
    column: usize,
    rows: usize,
    columns: usize,
) -> Result<(), EditError> {
    if rows == 0 || columns == 0 {
        return Err(EditError::InvalidRange);
    }
    if rows > MAX_PARTS || columns > MAX_PARTS {
        return Err(EditError::LimitExceeded);
    }
    let layout = Layout::of(table)?;
    let target = layout.region(layout.at(row, column)?)?;
    if rows == 1 && columns == 1 {
        return Ok(());
    }
    if columns > target.span {
        widen_grid(table, &layout, target, columns)?;
    }
    if rows > target.height {
        add_rows(table, target, rows - target.height)?;
    }
    // Both steps keep the cell's region where it starts; find it again.
    let layout = Layout::of(table)?;
    let id = layout.at(target.row, target.column)?;
    let region = layout.region(id)?;
    let spans = shares(region.span, columns);
    let heights = shares(region.height, rows);
    let top = layout.slot(region.row, id)?;
    let original = table
        .rows
        .get(region.row)
        .and_then(|r| r.cells.get(top.index))
        .cloned()
        .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
    let mut content = split_content(table, &layout, region, id, &original)?;
    let mut group_starts = Vec::with_capacity(heights.len());
    let mut start = region.row;
    for height in &heights {
        group_starts.push((start, *height));
        start += height;
    }
    for r in region.rows() {
        let slot = layout.slot(r, id)?;
        let (group_start, group_height) = group_starts
            .iter()
            .copied()
            .rfind(|(start, _)| *start <= r)
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
        let mut cells = Vec::with_capacity(spans.len());
        let mut part_column = region.column;
        for (p, span) in spans.iter().enumerate() {
            let mut props = original.props.clone();
            props.grid_span = span_of(*span)?;
            props.vertical_merge = if group_height <= 1 {
                None
            } else if r == group_start {
                Some(VerticalMerge::Restart)
            } else {
                Some(VerticalMerge::Continue)
            };
            if let Some(width) = grid_width(table, &(part_column..part_column + span)) {
                props.width = Some(width);
            }
            let first = r == region.row && p == 0;
            cells.push(TableCell {
                props,
                blocks: if first {
                    std::mem::take(&mut content)
                } else {
                    vec![Block::Paragraph(empty_paragraph())]
                },
                sdt: if first { original.sdt.clone() } else { None },
                location: if first {
                    original.location.clone()
                } else {
                    SourceLocation::unknown()
                },
            });
            part_column += span;
        }
        let row = table
            .rows
            .get_mut(r)
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
        if slot.index >= row.cells.len() {
            return Err(EditError::InvalidModel(Invariant::TableTopology));
        }
        row.cells.splice(slot.index..=slot.index, cells);
    }
    Ok(())
}

/// The content a split leaves in the top-left part: the cell's own, then
/// that of the cells its merge continued (usually empty), ending with a
/// paragraph as a cell must.
fn split_content(
    table: &Table,
    layout: &Layout,
    region: Region,
    id: usize,
    original: &TableCell,
) -> Result<Vec<Block>, EditError> {
    let mut content = original.blocks.clone();
    for r in region.rows().skip(1) {
        let slot = layout.slot(r, id)?;
        if let Some(cell) = table.rows.get(r).and_then(|row| row.cells.get(slot.index)) {
            if !is_empty(cell) {
                content.extend(cell.blocks.iter().cloned());
            }
        }
    }
    if !matches!(content.last(), Some(Block::Paragraph(_))) {
        content.push(Block::Paragraph(empty_paragraph()));
    }
    Ok(content)
}

/// Cuts `target`'s grid columns into `columns` equal ones; every other cell
/// and every `gridBefore`/`gridAfter` over them must cover all of them, and
/// widens by the columns added.
fn widen_grid(
    table: &mut Table,
    layout: &Layout,
    target: Region,
    columns: usize,
) -> Result<(), EditError> {
    let range = target.columns();
    let added = columns - target.span;
    let covers = |start: usize, end: usize| start <= range.start && range.end <= end;
    let apart = |start: usize, end: usize| end <= range.start || range.end <= start;
    // Every cell over the columns grows; one that covers only some of them
    // cannot be given a share of the new ones.
    let mut grow = Vec::new();
    for (r, slots) in layout.rows.iter().enumerate() {
        for slot in slots {
            let (start, end) = (slot.column, slot.column + slot.span);
            if layout.regions.get(slot.region) == Some(&target) {
                continue;
            }
            if covers(start, end) {
                grow.push((r, slot.index));
            } else if !apart(start, end) {
                return Err(EditError::InvalidRange);
            }
        }
    }
    let total_columns = table.grid.len().max(
        layout
            .rows
            .iter()
            .filter_map(|slots| slots.last().map(|slot| slot.column + slot.span))
            .max()
            .unwrap_or(0),
    );
    for (r, row) in table.rows.iter_mut().enumerate() {
        let before = usize::try_from(row.props.grid_before.unwrap_or(0)).unwrap_or(0);
        if before > 0 {
            if covers(0, before) {
                row.props.grid_before = Some(widen(before, added)?);
            } else if !apart(0, before) {
                return Err(EditError::InvalidRange);
            }
        }
        let after = usize::try_from(row.props.grid_after.unwrap_or(0)).unwrap_or(0);
        let end = layout
            .rows
            .get(r)
            .and_then(|slots| slots.last().map(|slot| slot.column + slot.span))
            .unwrap_or(before);
        if after > 0 {
            if covers(end, end + after) {
                row.props.grid_after = Some(widen(after, added)?);
            } else if !apart(end, end + after) {
                return Err(EditError::InvalidRange);
            }
        }
    }
    for (r, index) in grow {
        let cell = cell_mut(table, r, index)?;
        let span = usize::from(cell.props.grid_span.unwrap_or(1)) + added;
        cell.props.grid_span = span_of(span)?;
    }
    if range.end <= table.grid.len() {
        let total = grid_width(table, &range).and_then(|width| width.value);
        let widths: Vec<GridCol> = match total {
            Some(total) => {
                let count = i32::try_from(columns).map_err(|_| EditError::LimitExceeded)?;
                let each = total / count;
                (0..columns)
                    .map(|i| GridCol {
                        width: Some(Twips(if i + 1 == columns {
                            total - each * (count - 1)
                        } else {
                            each
                        })),
                    })
                    .collect()
            }
            None => vec![GridCol { width: None }; columns],
        };
        table.grid.splice(range, widths);
    } else if !table.grid.is_empty() && range.end > total_columns {
        return Err(EditError::InvalidModel(Invariant::Grid));
    }
    Ok(())
}

fn widen(count: usize, added: usize) -> Result<i32, EditError> {
    i32::try_from(count + added).map_err(|_| EditError::LimitExceeded)
}

/// Adds `count` rows below `target`'s last row. Every other cell of that row
/// continues down through them, so the target alone gains rows.
fn add_rows(table: &mut Table, target: Region, count: usize) -> Result<(), EditError> {
    let last = target.row + target.height - 1;
    let layout = Layout::of(table)?;
    let template = table
        .rows
        .get(last)
        .cloned()
        .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
    let slots = layout
        .rows
        .get(last)
        .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
    let mut cells = Vec::with_capacity(template.cells.len());
    for slot in slots {
        let cell = template
            .cells
            .get(slot.index)
            .ok_or(EditError::InvalidModel(Invariant::TableTopology))?;
        cells.push(continuation(&cell.props));
        // A cell that merged no rows starts a merge it now continues.
        if cell.props.vertical_merge.is_none() {
            cell_mut(table, last, slot.index)?.props.vertical_merge = Some(VerticalMerge::Restart);
        }
    }
    let mut props = template.props.clone();
    props.header = strict_ooxml_wml::model::TriState::Absent;
    let new_row = TableRow {
        props,
        cells,
        ..template
    };
    for k in 0..count {
        table.rows.insert(last + 1 + k, new_row.clone());
    }
    Ok(())
}
