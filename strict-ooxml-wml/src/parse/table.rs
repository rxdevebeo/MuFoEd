//! Parsing of tables: `w:tbl`, `w:tr`, `w:tc`, `w:tblGrid`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::block::{GridCol, Table, TableCell, TableRow};
use crate::model::props::{CellProperties, RowProperties, TableProperties};
use crate::model::values::Twips;

use super::{is_wml, parse_i32, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a table (`w:tbl`); its start element has been consumed.
    pub(crate) fn parse_table(&mut self) -> Result<Table> {
        let location = self.location();
        self.enter()?;
        let mut props = TableProperties::default();
        let mut grid = Vec::new();
        let mut rows = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "tblPr" => props = self.parse_table_properties()?,
                        "tblGrid" => grid = self.parse_table_grid()?,
                        "tr" => rows.push(self.parse_table_row(&attrs)?),
                        "sdt" => {
                            let container = self.parse_sdt(true)?;
                            for block in container.blocks {
                                if let crate::model::block::Block::Table(table) = block {
                                    rows.extend(table.rows);
                                }
                            }
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of table")),
            }
        }
        self.leave();
        Ok(Table {
            props,
            grid,
            rows,
            location,
        })
    }

    /// Parses `w:tblGrid`.
    fn parse_table_grid(&mut self) -> Result<Vec<GridCol>> {
        self.enter()?;
        let mut grid = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) && name.local() == "gridCol" {
                        grid.push(GridCol {
                            width: wml_attr(&attrs, "w").and_then(parse_i32).map(Twips),
                        });
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of table grid")),
            }
        }
        self.leave();
        Ok(grid)
    }

    /// Parses a table row (`w:tr`).
    pub(crate) fn parse_table_row(&mut self, _attrs: &[Attr]) -> Result<TableRow> {
        let location = self.location();
        self.enter()?;
        let mut props = RowProperties::default();
        let mut cells = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "trPr" => props = self.parse_row_properties()?,
                        "tc" => cells.push(self.parse_table_cell(&attrs)?),
                        "sdt" => {
                            let container = self.parse_sdt(true)?;
                            for block in container.blocks {
                                if let crate::model::block::Block::Table(table) = block {
                                    for row in table.rows {
                                        cells.extend(row.cells);
                                    }
                                }
                            }
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of table row")),
            }
        }
        self.leave();
        Ok(TableRow {
            props,
            cells,
            location,
        })
    }

    /// Parses a table cell (`w:tc`).
    pub(crate) fn parse_table_cell(&mut self, _attrs: &[Attr]) -> Result<TableCell> {
        let location = self.location();
        self.enter()?;
        let mut props = CellProperties::default();
        let mut blocks = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "tcPr" => props = self.parse_cell_properties()?,
                        "p" | "tbl" | "sdt" | "altChunk" | "ins" | "del" | "customXml" => {
                            let mut sections = Vec::new();
                            self.parse_block_element_into(
                                &name,
                                &attrs,
                                &mut blocks,
                                &mut sections,
                            )?;
                        }
                        _ => self.skip_element()?,
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of table cell")),
            }
        }
        self.leave();
        Ok(TableCell {
            props,
            blocks,
            location,
        })
    }
}
