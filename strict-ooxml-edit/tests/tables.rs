//! K09: merging and splitting table cells on the logical grid.
//!
//! The expected topologies are written out by hand from the grid, not derived
//! from the code under test: which cells each row holds, their spans and
//! vertical merges, and where the content went.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{Address, Edit, EditError, EditLimits, Editor};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{
    Block, Document, Inline, RunContent, Table, TableCell, VerticalMerge, WidthKind,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn doc(body: &str) -> Document {
    let xml = format!(
        "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\">\
         <w:body>{body}<w:p/></w:body></w:document>"
    );
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

fn cell(text: &str) -> String {
    format!("<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>")
}

/// A table with grid columns of `widths` twips and `rows` of cell XML.
fn table(widths: &[i32], rows: &[String]) -> String {
    let grid: String = widths
        .iter()
        .map(|w| format!("<w:gridCol w:w=\"{w}\"/>"))
        .collect();
    let rows: String = rows.iter().map(|r| format!("<w:tr>{r}</w:tr>")).collect();
    format!("<w:tbl><w:tblGrid>{grid}</w:tblGrid>{rows}</w:tbl>")
}

fn first_table(document: &Document) -> &Table {
    match document.body.blocks.first() {
        Some(Block::Table(table)) => table,
        other => panic!("table expected, got {other:?}"),
    }
}

/// The text of each paragraph of a cell.
fn texts(cell: &TableCell) -> Vec<String> {
    cell.blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(
                p.inlines
                    .iter()
                    .filter_map(|inline| match inline {
                        Inline::Run(run) => Some(
                            run.content
                                .iter()
                                .filter_map(|c| match c {
                                    RunContent::Text(t) => Some(t.text.as_str()),
                                    _ => None,
                                })
                                .collect::<String>(),
                        ),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// Each row as `(span, vertical merge)` per cell.
fn shape(table: &Table) -> Vec<Vec<(u16, Option<VerticalMerge>)>> {
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|c| (c.props.grid_span.unwrap_or(1), c.props.vertical_merge))
                .collect()
        })
        .collect()
}

fn widths(table: &Table) -> Vec<i32> {
    table
        .grid
        .iter()
        .map(|g| g.width.map_or(0, |w| w.value()))
        .collect()
}

fn two_by_two() -> Document {
    doc(&table(
        &[2000, 2000],
        &[cell("a") + &cell("b"), cell("c") + &cell("d")],
    ))
}

#[test]
fn merging_a_two_by_two_table_keeps_every_cell_in_reading_order() {
    let mut d = two_by_two();
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let change = e
        .transact(
            0,
            &[Edit::MergeCells {
                at: Address::body(0),
                rows: 0..2,
                columns: 0..2,
            }],
        )
        .expect("merge");
    let t = first_table(e.document());
    assert_eq!(
        shape(t),
        [
            vec![(2, Some(VerticalMerge::Restart))],
            vec![(2, Some(VerticalMerge::Continue))]
        ]
    );
    assert_eq!(texts(&t.rows[0].cells[0]), ["a", "b", "c", "d"]);
    assert_eq!(texts(&t.rows[1].cells[0]), [""]);
    let width = t.rows[0].cells[0].props.width.expect("width");
    assert_eq!((width.kind, width.value), (WidthKind::Dxa, Some(4000)));
    assert!(e.validate().is_ok());
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().body, before, "undo restores the table exactly");
}

#[test]
fn a_cell_sticking_out_of_the_rectangle_refuses_the_merge() {
    let mut d = doc(&table(
        &[2000, 2000],
        &[
            "<w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc>".to_owned(),
            cell("c") + &cell("d"),
        ],
    ));
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let refused = e.transact(
        0,
        &[Edit::MergeCells {
            at: Address::body(0),
            rows: 0..2,
            columns: 0..1,
        }],
    );
    assert_eq!(refused.unwrap_err(), EditError::InvalidRange);
    assert_eq!(e.document().body, before);
    // The rectangle that is the merged cell already is left as it is.
    e.transact(
        0,
        &[Edit::MergeCells {
            at: Address::body(0),
            rows: 0..1,
            columns: 0..2,
        }],
    )
    .expect("whole merged cell");
    assert_eq!(e.document().body, before);
}

#[test]
fn a_rectangle_outside_the_table_is_refused() {
    let mut d = two_by_two();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    for (rows, columns) in [(0..3, 0..1), (0..1, 1..3), (1..1, 0..1)] {
        assert_eq!(
            e.transact(
                0,
                &[Edit::MergeCells {
                    at: Address::body(0),
                    rows,
                    columns,
                }],
            )
            .unwrap_err(),
            EditError::InvalidRange
        );
    }
}

#[test]
fn splitting_into_more_columns_cuts_the_grid_and_keeps_its_width() {
    let mut d = two_by_two();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::SplitCell {
            at: Address::body(0),
            row: 0,
            column: 0,
            rows: 1,
            columns: 3,
        }],
    )
    .expect("split");
    let t = first_table(e.document());
    assert_eq!(widths(t), [666, 666, 668, 2000]);
    assert_eq!(
        shape(t),
        [
            vec![(1, None), (1, None), (1, None), (1, None)],
            vec![(3, None), (1, None)]
        ]
    );
    assert_eq!(texts(&t.rows[0].cells[0]), ["a"]);
    assert_eq!(texts(&t.rows[0].cells[1]), [""]);
    assert_eq!(texts(&t.rows[0].cells[3]), ["b"]);
    assert_eq!(texts(&t.rows[1].cells[0]), ["c"]);
    assert!(e.validate().is_ok());
}

#[test]
fn splitting_into_more_rows_adds_rows_the_other_cells_continue_through() {
    let mut d = doc(&table(&[2000, 2000], &[cell("a") + &cell("b")]));
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let change = e
        .transact(
            0,
            &[Edit::SplitCell {
                at: Address::body(0),
                row: 0,
                column: 0,
                rows: 3,
                columns: 1,
            }],
        )
        .expect("split");
    let t = first_table(e.document());
    assert_eq!(
        shape(t),
        [
            vec![(1, None), (1, Some(VerticalMerge::Restart))],
            vec![(1, None), (1, Some(VerticalMerge::Continue))],
            vec![(1, None), (1, Some(VerticalMerge::Continue))],
        ]
    );
    assert_eq!(texts(&t.rows[0].cells[0]), ["a"]);
    assert_eq!(texts(&t.rows[0].cells[1]), ["b"]);
    assert_eq!(texts(&t.rows[2].cells[0]), [""]);
    assert!(e.validate().is_ok());
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().body, before);
}

#[test]
fn splitting_a_merged_cell_gives_back_the_grid_it_covered() {
    let mut d = two_by_two();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[
            Edit::MergeCells {
                at: Address::body(0),
                rows: 0..2,
                columns: 0..2,
            },
            Edit::SplitCell {
                at: Address::body(0),
                row: 1,
                column: 1,
                rows: 2,
                columns: 2,
            },
        ],
    )
    .expect("merge then split");
    let t = first_table(e.document());
    assert_eq!(
        shape(t),
        [vec![(1, None), (1, None)], vec![(1, None), (1, None)]]
    );
    assert_eq!(widths(t), [2000, 2000], "the grid is unchanged");
    assert_eq!(texts(&t.rows[0].cells[0]), ["a", "b", "c", "d"]);
    assert_eq!(texts(&t.rows[1].cells[1]), [""]);
    assert!(e.validate().is_ok());
}

#[test]
fn a_split_within_a_merged_cell_divides_its_rows_and_columns() {
    // One cell over three columns and four rows, split into 2 x 2: the
    // larger shares go first (2 + 1 columns, 2 + 2 rows).
    let top = "<w:tc><w:tcPr><w:gridSpan w:val=\"3\"/><w:vMerge w:val=\"restart\"/></w:tcPr>\
               <w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>";
    let rest = "<w:tc><w:tcPr><w:gridSpan w:val=\"3\"/><w:vMerge/></w:tcPr><w:p/></w:tc>";
    let mut d = doc(&table(
        &[1000, 1000, 1000],
        &[
            top.to_owned(),
            rest.to_owned(),
            rest.to_owned(),
            rest.to_owned(),
        ],
    ));
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::SplitCell {
            at: Address::body(0),
            row: 2,
            column: 2,
            rows: 2,
            columns: 2,
        }],
    )
    .expect("split");
    let t = first_table(e.document());
    let restart = Some(VerticalMerge::Restart);
    let cont = Some(VerticalMerge::Continue);
    assert_eq!(
        shape(t),
        [
            vec![(2, restart), (1, restart)],
            vec![(2, cont), (1, cont)],
            vec![(2, restart), (1, restart)],
            vec![(2, cont), (1, cont)],
        ]
    );
    assert_eq!(widths(t), [1000, 1000, 1000]);
    assert_eq!(texts(&t.rows[0].cells[0]), ["x"]);
    assert!(e.validate().is_ok());
}

#[test]
fn a_split_needing_a_column_that_cuts_another_cell_is_refused() {
    // The target spans columns 0..2. Splitting it into three needs a third
    // column there; row 2's first cell covers only column 0 of the two and
    // could get no share of the new one, so the split is refused.
    let mut d = doc(&table(
        &[1000, 1000, 1000],
        &[
            "<w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc>".to_owned() + &cell("z"),
            cell("p") + "<w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc>",
        ],
    ));
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let refused = e.transact(
        0,
        &[Edit::SplitCell {
            at: Address::body(0),
            row: 0,
            column: 0,
            rows: 1,
            columns: 3,
        }],
    );
    assert_eq!(refused.unwrap_err(), EditError::InvalidRange);
    assert_eq!(e.document().body, before);
}

#[test]
fn split_limits_are_checked() {
    let mut d = two_by_two();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    for (rows, columns, error) in [
        (0, 1, EditError::InvalidRange),
        (1, 0, EditError::InvalidRange),
        (64, 1, EditError::LimitExceeded),
        (1, 64, EditError::LimitExceeded),
    ] {
        assert_eq!(
            e.transact(
                0,
                &[Edit::SplitCell {
                    at: Address::body(0),
                    row: 0,
                    column: 0,
                    rows,
                    columns,
                }],
            )
            .unwrap_err(),
            error
        );
    }
    // A position outside the table.
    assert_eq!(
        e.transact(
            0,
            &[Edit::SplitCell {
                at: Address::body(0),
                row: 5,
                column: 0,
                rows: 2,
                columns: 1,
            }],
        )
        .unwrap_err(),
        EditError::InvalidRange
    );
}
