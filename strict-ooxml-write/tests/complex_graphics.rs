//! Unsupported path gradients remain intact when writing Strict packages.
#![allow(clippy::expect_used, clippy::doc_markdown)]
use std::io::Cursor;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::graphics::{fill_docx, GRADIENT};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

#[test]
fn unsupported_path_shading_survives_parse_write_and_reopen() {
    let fill = GRADIENT.replace(
        "<a:lin ang=\"0\" scaled=\"1\"/>",
        "<a:path path=\"circle\"><a:fillToRect l=\"50%\" t=\"50%\" r=\"50%\" b=\"50%\"/></a:path>",
    );
    let package = Package::open_reader(
        Cursor::new(fill_docx("ellipse", &fill)),
        &OpenOptions::default(),
    )
    .expect("open");
    let doc = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write_package(&doc, Some(&package), &WriteOptions::default()).expect("write");
    let reopened =
        Package::open_reader(Cursor::new(written.bytes), &OpenOptions::default()).expect("reopen");
    let xml = reopened
        .read_part(&PartId::new("/word/document.xml"))
        .expect("part");
    let xml = String::from_utf8(xml).expect("XML");
    assert!(xml.contains("<a:path path=\"circle\""));
    assert!(!xml.contains("<a:lin"));
    parse_document(&reopened, &ParseOptions::default()).expect("reparse");
}
