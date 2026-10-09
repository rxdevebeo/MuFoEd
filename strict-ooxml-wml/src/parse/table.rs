//! Parsing of tables: `w:tbl`, `w:tr`, `w:tc`, `w:tblGrid`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::block::{GridCol, SdtProperties, Table, TableCell, TableGridChange, TableRow};
use crate::model::props::{CellProperties, RowProperties, TableProperties};
use crate::model::support::SupportStatus;
use crate::model::values::Twips;

use super::{is_wml, parse_u32, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a table (`w:tbl`); its start element has been consumed.
    pub(crate) fn parse_table(&mut self) -> Result<Table> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = TableProperties::default();
            let mut grid = Vec::new();
            let mut grid_change = None;
            let mut rows = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "tblPr" => parser.read_table_properties(&mut props)?,
                            "tblGrid" => {
                                let parsed = parser.parse_table_grid()?;
                                grid = parsed.0;
                                grid_change = parsed.1.or(grid_change);
                            }
                            "tr" => rows.push(parser.parse_table_row(&attrs)?),
                            "sdt" => parser.push_sdt_rows(&mut rows)?,
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of table")),
                }
            }
            Ok(Table {
                props,
                grid,
                grid_change,
                rows,
                location,
            })
        })
    }

    /// Parses `w:tblGrid`, including a following `w:tblGridChange`.
    fn parse_table_grid(&mut self) -> Result<(Vec<GridCol>, Option<TableGridChange>)> {
        self.nested(|parser| {
            let mut grid = Vec::new();
            let mut change = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == "gridCol" {
                            grid.push(GridCol {
                                width: parser
                                    .measure_or_percent(&attrs, "w", "w:gridCol")
                                    .map(Twips),
                            });
                            parser.skip_element()?;
                        } else if is_wml(&name) && name.local() == "tblGridChange" {
                            change = Some(parser.parse_grid_change(&attrs)?);
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of table grid")),
                }
            }
            Ok((grid, change))
        })
    }

    /// Parses `w:tblGridChange`: the previous `w:tblGrid` of a tracked change.
    fn parse_grid_change(&mut self, attrs: &[Attr]) -> Result<TableGridChange> {
        let id = wml_attr(attrs, "id").and_then(parse_u32).unwrap_or(0);
        self.nested(|parser| {
            let mut grid = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_wml(&name) && name.local() == "tblGrid" {
                            grid = parser.parse_table_grid()?.0;
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of table grid change"))
                    }
                }
            }
            Ok(TableGridChange { id, grid })
        })
    }

    /// Parses a table row (`w:tr`).
    pub(crate) fn parse_table_row(&mut self, attrs: &[Attr]) -> Result<TableRow> {
        for attr in attrs {
            let local = attr.name.local();
            if local.starts_with("rsid") {
                self.record(
                    &format!("w:tr@{local}"),
                    crate::model::support::SupportStatus::Partial,
                    Some("row revision id is not written back".to_owned()),
                    Some(self.location()),
                );
            }
        }
        let location = self.location();
        self.nested(|parser| {
            let mut props = RowProperties::default();
            let mut cells = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "trPr" => parser.read_row_properties(&mut props)?,
                            "tblPrEx" => parser.read_row_exception(&mut props)?,
                            "tc" => cells.push(parser.parse_table_cell(&attrs)?),
                            "sdt" => parser.push_sdt_cells(&mut cells)?,
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of table row")),
                }
            }
            Ok(TableRow {
                props,
                cells,
                sdt: None,
                location,
            })
        })
    }

    /// Parses a table cell (`w:tc`).
    pub(crate) fn parse_table_cell(&mut self, _attrs: &[Attr]) -> Result<TableCell> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = CellProperties::default();
            let mut blocks = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "tcPr" => parser.read_cell_properties(&mut props)?,
                            "p" | "tbl" | "sdt" | "altChunk" | "ins" | "del" | "customXml"
                            | "smartTag" => {
                                parser.parse_block_element_into(&name, &attrs, &mut blocks)?;
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of table cell")),
                }
            }
            Ok(TableCell {
                props,
                blocks,
                sdt: None,
                location,
            })
        })
    }

    // The helpers below run work that does not recurse - property bags, and the
    // row/cell-level content controls whose `SdtProperties` is large - in a frame
    // of their own. `parse_table` -> `parse_table_row` -> `parse_table_cell` ->
    // `parse_block_element_into` is entered once per nesting level, and a debug
    // build reserves every local of a function in its frame whether or not the
    // arm runs: twelve nested tables needed 1031 KiB to parse, over the 1 MiB the
    // hostile suite gives a thread (measured with a stack probe, 2026-10-08).

    #[inline(never)]
    fn read_table_properties(&mut self, props: &mut TableProperties) -> Result<()> {
        *props = self.parse_table_properties()?;
        Ok(())
    }

    #[inline(never)]
    fn read_cell_properties(&mut self, props: &mut CellProperties) -> Result<()> {
        *props = self.parse_cell_properties()?;
        Ok(())
    }

    /// `w:trPr`. `w:tblPrEx` is the preceding sibling, so replacing the row
    /// properties must keep the exception.
    #[inline(never)]
    fn read_row_properties(&mut self, props: &mut RowProperties) -> Result<()> {
        let exception_borders = std::mem::take(&mut props.exception_borders);
        let exception = props.exception.take();
        *props = self.parse_row_properties()?;
        props.exception_borders = exception_borders;
        props.exception = exception;
        Ok(())
    }

    /// `w:tblPrEx` (AUD-46): exception properties may carry cell spacing, and
    /// borders on the exception are the row's own edges.
    #[inline(never)]
    fn read_row_exception(&mut self, props: &mut RowProperties) -> Result<()> {
        let ex = self.parse_table_properties()?;
        props.exception_borders = ex.borders.clone();
        props.exception = Some(Box::new(ex));
        self.record(
            "w:tblPrEx",
            SupportStatus::Partial,
            Some("row exception properties partially modelled".to_owned()),
            Some(self.location()),
        );
        Ok(())
    }

    /// AUD-41: a row-level content control - unwrap its `w:tr` children and keep
    /// `sdtPr` on each row for the writer.
    #[inline(never)]
    fn push_sdt_rows(&mut self, rows: &mut Vec<TableRow>) -> Result<()> {
        let (sdt, mut found) = self.parse_sdt_table_rows()?;
        self.record(
            "w:sdt",
            SupportStatus::Partial,
            Some("row/cell-level content control unwrapped; properties kept".to_owned()),
            Some(sdt.location.clone()),
        );
        for row in &mut found {
            if row.sdt.is_none() {
                row.sdt = Some(sdt.clone());
            }
        }
        rows.append(&mut found);
        Ok(())
    }

    /// AUD-41: a cell-level content control (see [`Self::push_sdt_rows`]).
    #[inline(never)]
    fn push_sdt_cells(&mut self, cells: &mut Vec<TableCell>) -> Result<()> {
        let (sdt, mut found) = self.parse_sdt_table_cells()?;
        self.record(
            "w:sdt",
            SupportStatus::Partial,
            Some("row/cell-level content control unwrapped; properties kept".to_owned()),
            Some(sdt.location.clone()),
        );
        for cell in &mut found {
            if cell.sdt.is_none() {
                cell.sdt = Some(sdt.clone());
            }
        }
        cells.append(&mut found);
        Ok(())
    }

    /// Parses a row-level `w:sdt`, returning its properties and the rows inside
    /// `sdtContent`.
    fn parse_sdt_table_rows(&mut self) -> Result<(SdtProperties, Vec<TableRow>)> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = SdtProperties {
                location: location.clone(),
                ..SdtProperties::default()
            };
            let mut rows = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "sdtPr" => {
                                parser.parse_sdt_properties()?.merge_into(&mut props);
                            }
                            "sdtEndPr" => {
                                props.has_end_pr = true;
                                props.end_run_props =
                                    parser.parse_sdt_end_properties()?.or(props.end_run_props);
                            }
                            "sdtContent" => {
                                rows.append(&mut parser.parse_table_row_children()?);
                            }
                            _ => {
                                let _ = attrs;
                                parser.skip_element()?;
                            }
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(
                            parser.invalid("unexpected end of row-level structured document tag")
                        )
                    }
                }
            }
            Ok((props, rows))
        })
    }

    /// Parses a cell-level `w:sdt`, returning its properties and the cells inside
    /// `sdtContent`.
    fn parse_sdt_table_cells(&mut self) -> Result<(SdtProperties, Vec<TableCell>)> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = SdtProperties {
                location: location.clone(),
                ..SdtProperties::default()
            };
            let mut cells = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "sdtPr" => {
                                parser.parse_sdt_properties()?.merge_into(&mut props);
                            }
                            "sdtEndPr" => {
                                props.has_end_pr = true;
                                props.end_run_props =
                                    parser.parse_sdt_end_properties()?.or(props.end_run_props);
                            }
                            "sdtContent" => {
                                cells.append(&mut parser.parse_table_cell_children()?);
                            }
                            _ => {
                                let _ = attrs;
                                parser.skip_element()?;
                            }
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(
                            parser.invalid("unexpected end of cell-level structured document tag")
                        )
                    }
                }
            }
            Ok((props, cells))
        })
    }

    /// Parses `w:tr` (and nested row-level `w:sdt`) children until the current
    /// element's end — used for `sdtContent` inside a table.
    fn parse_table_row_children(&mut self) -> Result<Vec<TableRow>> {
        self.nested(|parser| {
            let mut rows = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "tr" => rows.push(parser.parse_table_row(&attrs)?),
                            "sdt" => {
                                let (sdt, mut found) = parser.parse_sdt_table_rows()?;
                                for row in &mut found {
                                    if row.sdt.is_none() {
                                        row.sdt = Some(sdt.clone());
                                    }
                                }
                                rows.append(&mut found);
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of table row content"))
                    }
                }
            }
            Ok(rows)
        })
    }

    /// Parses `w:tc` (and nested cell-level `w:sdt`) children until the current
    /// element's end — used for `sdtContent` inside a row.
    fn parse_table_cell_children(&mut self) -> Result<Vec<TableCell>> {
        self.nested(|parser| {
            let mut cells = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        match name.local() {
                            "tc" => cells.push(parser.parse_table_cell(&attrs)?),
                            "sdt" => {
                                let (sdt, mut found) = parser.parse_sdt_table_cells()?;
                                for cell in &mut found {
                                    if cell.sdt.is_none() {
                                        cell.sdt = Some(sdt.clone());
                                    }
                                }
                                cells.append(&mut found);
                            }
                            _ => parser.skip_element()?,
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of table cell content"))
                    }
                }
            }
            Ok(cells)
        })
    }
}
