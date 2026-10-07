//! Tables, end to end: a PDF this workspace wrote, converted back to a package a
//! reader opens.
//!
//! The fixture is a `Document` with a bordered table, rendered to PDF by the
//! project's own renderers and then read by the project's own reader. That is
//! deliberate and it is a limitation: it proves the converter against ink this
//! project laid down, not against ink a foreign producer drew (Q-9, the corpus
//! of third-party PDFs). What it does prove is the whole chain — a table in, a
//! `w:tbl` out, readable by our own reader and by an independent XML parser —
//! and the unit tests in `src/tables.rs` cover the decisions with rules drawn by
//! hand, including the shapes our renderer never produces.
//!
//! Every claim here has a named acceptance criterion behind it: `SC-5` (99.9 %
//! of the characters reach the document) and `SC-1` (two runs, the same bytes).
//!
//! The character counts are bounded by the document, far below 2^53, and the
//! test crate spells out `WordprocessingML` and `O9a` in prose.
#![allow(clippy::cast_precision_loss, clippy::doc_markdown)]

mod common;

use std::path::Path;

use common::text::{pdf_reading_text, score};
use strict_ooxml_convert::{convert, Mode, PdfOptions, TableRules};
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_wml::model::block::{Block, GridCol, Paragraph, Table, TableCell, TableRow};
use strict_ooxml_wml::model::drawing::MediaIndex;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::notes::NoteTable;
use strict_ooxml_wml::model::numbering::NumberingTable;
use strict_ooxml_wml::model::props::{
    CellProperties, ParagraphProperties, RowProperties, RunProperties, TableProperties,
};
use strict_ooxml_wml::model::settings::Settings;
use strict_ooxml_wml::model::styles::StyleTable;
use strict_ooxml_wml::model::support::SupportModel;
use strict_ooxml_wml::model::values::{
    Border, BorderStyle, Borders, Color, EighthsPoint, Rsids, Space, Twips, VerticalMerge,
};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The columns of the fixture table, in twips.
const COLUMNS: [i32; 3] = [2666, 2666, 2668];

/// The WordprocessingML Strict namespace, for the independent oracle.
const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

fn location() -> SourceLocation {
    SourceLocation::unknown()
}

fn paragraph(value: &str) -> Paragraph {
    Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Run(Run {
            props: RunProperties::default(),
            content: vec![RunContent::Text(TextNode {
                text: value.to_owned(),
                space: Space::default(),
            })],
            revision: None,
            location: location(),
        })],
        rsids: Rsids::default(),
        para_id: None,
        text_id: None,
        revision: None,
        location: location(),
    }
}

fn border() -> Border {
    Border {
        style: Some(BorderStyle::Single),
        size: Some(EighthsPoint(4)),
        color: Some(Color::new("000000")),
        space: None,
        shadow: false,
        frame: false,
    }
}

fn cell(value: &str) -> TableCell {
    TableCell {
        props: CellProperties::default(),
        blocks: if value.is_empty() {
            Vec::new()
        } else {
            vec![Block::Paragraph(paragraph(value))]
        },
        sdt: None,
        location: location(),
    }
}

fn merged_across(value: &str, span: u16) -> TableCell {
    TableCell {
        props: CellProperties {
            grid_span: Some(span),
            ..CellProperties::default()
        },
        blocks: vec![Block::Paragraph(paragraph(value))],
        sdt: None,
        location: location(),
    }
}

fn merged_down(state: VerticalMerge, value: &str) -> TableCell {
    TableCell {
        props: CellProperties {
            vertical_merge: Some(state),
            ..CellProperties::default()
        },
        blocks: if value.is_empty() {
            Vec::new()
        } else {
            vec![Block::Paragraph(paragraph(value))]
        },
        sdt: None,
        location: location(),
    }
}

fn row(cells: Vec<TableCell>) -> TableRow {
    TableRow {
        props: RowProperties::default(),
        cells,
        sdt: None,
        location: location(),
    }
}

/// The fixture: a paragraph, a three-column table with a merged header and a
/// vertically merged first column, and a paragraph after it.
fn fixture() -> Document {
    let borders = Borders {
        top: Some(border()),
        bottom: Some(border()),
        start: Some(border()),
        end: Some(border()),
        inside_horizontal: Some(border()),
        inside_vertical: Some(border()),
    };
    let table = Table {
        props: TableProperties {
            width: Some(strict_ooxml_wml::model::values::Width {
                kind: strict_ooxml_wml::model::values::WidthKind::Dxa,
                value: Some(8000),
            }),
            borders,
            ..TableProperties::default()
        },
        grid: COLUMNS
            .iter()
            .map(|width| GridCol {
                width: Some(Twips(*width)),
            })
            .collect(),
        grid_change: None,
        rows: vec![
            row(vec![merged_across("Merged header", 3)]),
            row(vec![
                merged_down(VerticalMerge::Restart, "Side"),
                cell("B1"),
                cell("C1"),
            ]),
            row(vec![
                merged_down(VerticalMerge::Continue, ""),
                cell("B2"),
                cell("C2"),
            ]),
        ],
        location: location(),
    };
    Document {
        body: strict_ooxml_wml::model::document::Body {
            blocks: vec![
                Block::Paragraph(paragraph("Before the table")),
                Block::Table(table),
                Block::Paragraph(paragraph("After the table")),
            ],
        },
        styles: StyleTable::default(),
        numbering: NumberingTable::default(),
        footnotes: NoteTable::default(),
        endnotes: NoteTable::default(),
        settings: Settings::default(),
        font_table: None,
        theme: None,
        sections: Vec::new(),
        headers_footers: Vec::new(),
        media: MediaIndex::default(),
        support: SupportModel::default(),
        source: strict_ooxml_wml::model::document::DocumentSource {
            main_document: strict_ooxml_core::part::PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            footnotes: None,
            endnotes: None,
            theme: None,
            font_table: None,
        },
    }
}

/// The fixture as a PDF, written by this workspace's own renderers.
fn fixture_pdf() -> Vec<u8> {
    let options = strict_ooxml_render_svg::RenderOptions::default();
    let placed = strict_ooxml_render_svg::place_pages(&fixture(), &options, None).expect("place");
    strict_ooxml_render_pdf::render_with_source(&placed, &options, None)
        .expect("render")
        .bytes
}

/// The PDF of a corpus document, for the case our renderer cannot produce.
fn corpus_pdf(file: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    let bytes = std::fs::read(root.join(file)).expect("the fixture is present");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = strict_ooxml_render_svg::RenderOptions::default();
    let placed =
        strict_ooxml_render_svg::place_pages(&document, &options, Some(&package)).expect("place");
    strict_ooxml_render_pdf::render_with_source(&placed, &options, Some(&package))
        .expect("render")
        .bytes
}

/// The text of every block, tables included, in document order.
fn all_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => out.push_str(&paragraph_text(paragraph)),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        out.push_str(&all_text(&cell.blocks));
                    }
                    out.push('\n');
                }
            }
            _ => {}
        }
        out.push(' ');
    }
    out
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    let mut out = String::new();
    for inline in &paragraph.inlines {
        if let Inline::Run(run) = inline {
            for content in &run.content {
                if let RunContent::Text(node) = content {
                    out.push_str(&node.text);
                }
            }
        }
    }
    out
}

/// Converts the fixture and writes it out, as a caller would.
fn convert_fixture(options: &PdfOptions) -> (strict_ooxml_convert::Converted, Vec<u8>) {
    let pdf = fixture_pdf();
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted = convert(&mut reader, options).expect("convert");
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written = strict_ooxml_write::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .expect("write");
    (converted, written.bytes)
}

/// The only table of a converted document.
fn only_table(converted: &strict_ooxml_convert::Converted) -> &Table {
    let tables: Vec<&Table> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_table)
        .collect();
    assert_eq!(tables.len(), 1, "the fixture has one table");
    tables[0]
}

/// The cell texts of a table, row by row.
fn cell_texts(table: &Table) -> Vec<Vec<String>> {
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| all_text(&cell.blocks).trim().to_owned())
                .collect()
        })
        .collect()
}

/// The grid of ruling lines became a table, with the page's own text in it.
#[test]
fn a_grid_becomes_a_table() {
    let (converted, _bytes) = convert_fixture(&PdfOptions::default());
    assert_eq!(converted.report.tables(), 1, "{}", converted.report);
    let table = only_table(&converted);
    assert_eq!(table.rows.len(), 3, "{}", cell_texts(table).len());
    assert_eq!(table.grid.len(), 3, "three columns");
    // The rules gave the columns their widths, and the widths are the ones the
    // source document asked for: 2666 + 2666 + 2668 twips.
    let widths: Vec<i32> = table
        .grid
        .iter()
        .map(|column| column.width.map_or(0, |width| width.0))
        .collect();
    assert_eq!(widths, COLUMNS.to_vec());
    assert_eq!(
        cell_texts(table),
        vec![
            vec!["Merged header".to_owned()],
            vec!["Side".to_owned(), "B1".to_owned(), "C1".to_owned()],
            vec![String::new(), "B2".to_owned(), "C2".to_owned()],
        ]
    );
}

/// The table sits between the paragraphs that were above and below it, and none
/// of the page's text is left in the flow behind.
#[test]
fn the_table_sits_where_its_text_was() {
    let (converted, _bytes) = convert_fixture(&PdfOptions::default());
    let kinds: Vec<&str> = converted
        .document
        .body
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(_) => "paragraph",
            Block::Table(_) => "table",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["paragraph", "table", "paragraph"], "{kinds:?}");
    let flow: String = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .map(paragraph_text)
        .collect();
    assert_eq!(flow, "Before the tableAfter the table");
}

/// A cell's text is in the table exactly once: moving it out of the page's flow
/// must not move it out of the document.
#[test]
fn every_character_reaches_the_document() {
    let pdf = fixture_pdf();
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let expected = pdf_reading_text(&mut reader).expect("pages");
    let (converted, bytes) = convert_fixture(&PdfOptions::default());
    // The text of the *written package*, as a reader of the `.docx` sees it.
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let got = all_text(&document.body.blocks);
    let measured = score(&expected, &got);
    let ratio = measured
        .recall()
        .expect("the fixture carries no text to compare");
    assert!(
        ratio >= 0.999,
        "only {ratio:.4} of the PDF's {} characters reached the document",
        measured.want_len
    );
    assert_eq!(
        all_text(&converted.document.body.blocks).replace(' ', ""),
        got.replace(' ', ""),
        "what the converter built and what the writer wrote must be the same text"
    );
}

/// The merges the ink showed are merges in the output, and an independent XML
/// parser agrees.
#[test]
fn the_merges_survive_the_round_trip() {
    let (_converted, bytes) = convert_fixture(&PdfOptions::default());
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let Some(Block::Table(table)) = document
        .body
        .blocks
        .iter()
        .find(|block| block.as_table().is_some())
    else {
        panic!("the written package has no table");
    };
    assert_eq!(table.rows[0].cells.len(), 1, "one cell over three columns");
    assert_eq!(table.rows[0].cells[0].props.grid_span, Some(3));
    assert_eq!(
        table.rows[1].cells[0].props.vertical_merge,
        Some(VerticalMerge::Restart)
    );
    assert_eq!(
        table.rows[2].cells[0].props.vertical_merge,
        Some(VerticalMerge::Continue)
    );
    // `w:vMerge` with no `w:val` is the schema default for `continue`; a reader
    // that lost the bare element would report a row with a cell that merges into
    // nothing.
    let main = strict_ooxml_core::part::PartId::new("/word/document.xml");
    let xml = package.read_part(&main).expect("read the main part");
    let tree =
        roxmltree::Document::parse(std::str::from_utf8(&xml).expect("utf-8")).expect("well formed");
    let spans: Vec<String> = tree
        .descendants()
        .filter(|node| node.has_tag_name((W_NS, "gridSpan")))
        .map(|node| node.attribute((W_NS, "val")).unwrap_or_default().to_owned())
        .collect();
    assert_eq!(spans, vec!["3"], "the header spans three columns");
    let merges: Vec<Option<String>> = tree
        .descendants()
        .filter(|node| node.has_tag_name((W_NS, "vMerge")))
        .map(|node| node.attribute((W_NS, "val")).map(ToOwned::to_owned))
        .collect();
    assert_eq!(merges.len(), 2, "the merged column has two cells");
    assert_eq!(merges[0].as_deref(), Some("restart"));
    assert_eq!(merges[1], None, "`continue` is written bare");
}

/// The borders of the table come from the ink: the rules were half a point wide
/// and black, and that is what the cells are given.
#[test]
fn the_borders_are_the_ones_that_were_drawn() {
    let (converted, _bytes) = convert_fixture(&PdfOptions::default());
    let table = only_table(&converted);
    for edge in [&table.props.borders.top, &table.props.borders.start] {
        let edge = edge.as_ref().expect("a border");
        assert_eq!(edge.style, Some(BorderStyle::Single));
        assert_eq!(edge.size, Some(EighthsPoint(4)), "0.5 pt is four eighths");
        assert_eq!(
            edge.color.as_ref().map(ToString::to_string),
            Some("000000".to_owned())
        );
    }
}

/// Two runs of the converter on the same page give the same document and the
/// same report, byte for byte.
#[test]
fn the_conversion_is_reproducible() {
    let (first, first_bytes) = convert_fixture(&PdfOptions::default());
    let (second, second_bytes) = convert_fixture(&PdfOptions::default());
    assert_eq!(first_bytes, second_bytes, "SC-1: the same bytes");
    assert_eq!(first.report.to_string(), second.report.to_string());
    assert_eq!(first.document.body.blocks, second.document.body.blocks);
}

/// The rules are the converter's claims, and a caller that will not accept a
/// three-column grid gets paragraphs instead - and is told so.
#[test]
fn the_table_rules_are_load_bearing() {
    let (converted, _bytes) = convert_fixture(&PdfOptions::default().tables(TableRules {
        min_columns: 4,
        ..TableRules::default()
    }));
    assert_eq!(converted.report.tables(), 0);
    let text = all_text(&converted.document.body.blocks);
    for expected in ["Merged header", "B1", "C1", "B2", "C2"] {
        assert!(
            text.contains(expected),
            "{expected} is still in the document"
        );
    }
    assert!(
        converted.report.to_string().contains("table.detected"),
        "{}",
        converted.report
    );
}

/// A table whose borders the producer never drew cannot be read out of the ink:
/// the cells stay paragraphs, and that is the honest answer rather than a guess
/// at a structure the PDF does not carry.
#[test]
fn a_table_with_no_rules_stays_paragraphs() {
    // `strict-stage5.docx` carries a table with no `w:tblBorders`, which is the
    // common case for a table laid out by spacing alone.
    let pdf = corpus_pdf("strict-stage5.docx");
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted = convert(&mut reader, &PdfOptions::default()).expect("convert");
    assert_eq!(converted.report.tables(), 0, "{}", converted.report);
    assert!(
        converted
            .document
            .body
            .blocks
            .iter()
            .all(|block| !matches!(block, Block::Table(_))),
        "no table is invented"
    );
    let text = all_text(&converted.document.body.blocks);
    for expected in ["B1", "C1", "B2", "C2"] {
        assert!(text.contains(expected), "{expected} reached the document");
    }
}

/// The visual mode is geometry, not structure: it lays the page out as it was
/// and makes no claim about tables, so it must not gain one.
#[test]
fn the_visual_mode_makes_no_structural_claim() {
    let pdf = fixture_pdf();
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let converted =
        convert(&mut reader, &PdfOptions::default().mode(Mode::Visual)).expect("convert");
    assert!(
        converted
            .document
            .body
            .blocks
            .iter()
            .all(|block| !matches!(block, Block::Table(_))),
        "the visual mode reproduces the page, not its structure"
    );
    assert_eq!(converted.report.tables(), 0);
    let text = all_text(&converted.document.body.blocks);
    for expected in ["Merged header", "B1", "C1", "B2", "C2"] {
        assert!(text.contains(expected), "{expected} reached the document");
    }
}
