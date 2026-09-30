//! Prints where a written document stops matching the one it was parsed from.
//!
//! A development aid for `STAGE-8-TASK.md` SC-3: the round-trip test says
//! *that* a document changed, this says *where*.

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: roundtrip_diff <file.docx>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&path).expect("read");
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened =
        Package::open_reader(&written.bytes[..], &OpenOptions::default()).expect("reopen");
    let reparsed = parse_document(&reopened, &ParseOptions::default()).expect("reparse");

    println!(
        "blocks: {} -> {}",
        document.body.blocks.len(),
        reparsed.body.blocks.len()
    );
    println!("report:\n{}", written.report);

    for (index, (left, right)) in document
        .body
        .blocks
        .iter()
        .zip(reparsed.body.blocks.iter())
        .enumerate()
    {
        if left == right {
            continue;
        }
        println!("--- block {index} differs");
        report_block(left, right, 0);
        break;
    }

    if document.sections != reparsed.sections {
        println!("--- sections differ");
        for (index, (left, right)) in document
            .sections
            .iter()
            .zip(reparsed.sections.iter())
            .enumerate()
        {
            if left != right {
                println!("section {index}:\n  A: {left:#?}\n  B: {right:#?}");
            }
        }
    }
    if document.styles.len() != reparsed.styles.len() {
        println!(
            "--- styles: {} -> {}",
            document.styles.len(),
            reparsed.styles.len()
        );
    }
}

fn report_block(left: &Block, right: &Block, depth: usize) {
    let pad = "  ".repeat(depth);
    match (left, right) {
        (Block::Paragraph(a), Block::Paragraph(b)) => {
            if a.props != b.props {
                println!("{pad}pPr:\n  A: {:#?}\n  B: {:#?}", a.props, b.props);
            }
            if a.inlines != b.inlines {
                println!("{pad}inlines: {} -> {}", a.inlines.len(), b.inlines.len());
                for (index, (x, y)) in a.inlines.iter().zip(b.inlines.iter()).enumerate() {
                    if x != y {
                        println!("{pad}  inline {index}:\n    A: {x:#?}\n    B: {y:#?}");
                        if let (Inline::Run(xr), Inline::Run(yr)) = (x, y) {
                            if xr.props != yr.props {
                                println!(
                                    "{pad}    rPr:\n      A: {:#?}\n      B: {:#?}",
                                    xr.props, yr.props
                                );
                            }
                            if xr.content != yr.content {
                                println!(
                                    "{pad}    content:\n      A: {:#?}\n      B: {:#?}",
                                    xr.content, yr.content
                                );
                            }
                        }
                        break;
                    }
                }
            }
            if a.rsids != b.rsids {
                println!("{pad}rsids:\n  A: {:#?}\n  B: {:#?}", a.rsids, b.rsids);
            }
        }
        (Block::Table(a), Block::Table(b)) => {
            if a.props != b.props {
                println!("{pad}tblPr differs");
            }
            for (row, (x, y)) in a.rows.iter().zip(b.rows.iter()).enumerate() {
                if x != y {
                    println!("{pad}row {row} differs");
                }
                for (cell, (x, y)) in x.cells.iter().zip(y.cells.iter()).enumerate() {
                    if x.props != y.props {
                        println!(
                            "{pad}  cell {cell} tcPr:\n    A: {:#?}\n    B: {:#?}",
                            x.props, y.props
                        );
                    }
                    for (block, (x, y)) in x.blocks.iter().zip(y.blocks.iter()).enumerate() {
                        if x != y {
                            println!("{pad}  cell {cell} block {block}:");
                            report_block(x, y, depth + 3);
                        }
                    }
                }
            }
        }
        _ => println!("{pad}A: {left:#?}\n{pad}B: {right:#?}"),
    }
}
