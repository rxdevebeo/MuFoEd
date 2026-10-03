//! Parsing of tables: `w:tbl`, `w:tr`, `w:tc`, `w:tblGrid`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::block::{GridCol, Table, TableCell, TableRow};
use crate::model::props::{CellProperties, RowProperties, TableProperties};
use crate::model::values::Twips;

use super::{is_wml, PartParser};

impl PartParser<'_> {
    /// Parses a table (`w:tbl`); its start element has been consumed.
    pub(crate) fn parse_table(&mut self) -> Result<Table> {
        let location = self.location();
        self.nested(|parser| {
            let mut props = TableProperties::default();
            let mut grid = Vec::new();
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
                            "tblPr" => props = parser.parse_table_properties()?,
                            "tblGrid" => grid = parser.parse_table_grid()?,
                            "tr" => rows.push(parser.parse_table_row(&attrs)?),
                            "sdt" => {
                                let container = parser.parse_sdt(true)?;
                                for block in container.blocks {
                                    if let crate::model::block::Block::Table(table) = block {
                                        rows.extend(table.rows);
                                    }
                                }
                            }
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
                rows,
                location,
            })
        })
    }

    /// Parses `w:tblGrid`.
    fn parse_table_grid(&mut self) -> Result<Vec<GridCol>> {
        self.nested(|parser| {
            let mut grid = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == "gridCol" {
                            grid.push(GridCol {
                                width: parser
                                    .measure_or_percent(&attrs, "w", "w:gridCol")
                                    .map(Twips),
                            });
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of table grid")),
                }
            }
            Ok(grid)
        })
    }

    /// Parses a table row (`w:tr`).
    pub(crate) fn parse_table_row(&mut self, _attrs: &[Attr]) -> Result<TableRow> {
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
                            "trPr" => props = parser.parse_row_properties()?,
                            "tc" => cells.push(parser.parse_table_cell(&attrs)?),
                            "sdt" => {
                                let container = parser.parse_sdt(true)?;
                                for block in container.blocks {
                                    if let crate::model::block::Block::Table(table) = block {
                                        for row in table.rows {
                                            cells.extend(row.cells);
                                        }
                                    }
                                }
                            }
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
                            "tcPr" => props = parser.parse_cell_properties()?,
                            "p" | "tbl" | "sdt" | "altChunk" | "ins" | "del" | "customXml" => {
                                let mut sections = Vec::new();
                                parser.parse_block_element_into(
                                    &name,
                                    &attrs,
                                    &mut blocks,
                                    &mut sections,
                                )?;
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
                location,
            })
        })
    }
}
