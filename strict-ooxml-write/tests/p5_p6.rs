//! P5–P6: run metrics and table widths survive a Strict rewrite.
//!
//! Positive controls keep half-points, twips and grid columns. The negative
//! controls change one half-point or one twip and require the written package
//! to show that change. One twip is about 0.067 px at 96 dpi, under the 0.25 px
//! paint tolerance, so the failure has to be visible here in the model.

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::values::{EighthsPoint, HalfPoints, TabAlignment, Twips};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn open(bytes: &[u8]) -> Result<Package, StrictError> {
    Package::open_reader(bytes, &OpenOptions::default())
}

fn parse(package: &Package) -> Result<Document, StrictError> {
    parse_document(package, &ParseOptions::default())
}

fn document_xml(package: &Package) -> String {
    String::from_utf8(
        package
            .read_part(&PartId::new("/word/document.xml"))
            .expect("document"),
    )
    .expect("utf-8")
}

fn round_trip(body: &str) -> (Document, Document, String) {
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    (document, reparsed, document_xml(&reopened))
}

/// T-P5-1: `w:sz` / `w:szCs` stay half-points, and `w:w` is rewritten as `N%`.
#[test]
fn t_p5_1_half_points_and_text_scale_round_trip() {
    let body = "\
<w:p><w:r><w:rPr><w:sz w:val=\"24\"/><w:szCs w:val=\"20\"/><w:w w:val=\"90\"/></w:rPr>\
<w:t>Metric</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip(body);
    let metrics = |document: &Document| {
        let paragraph = document.body.blocks[0].as_paragraph().expect("paragraph");
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("expected a run");
        };
        (run.props.size, run.props.size_cs, run.props.scale)
    };
    assert_eq!(
        metrics(&before),
        (Some(HalfPoints(24)), Some(HalfPoints(20)), Some(90))
    );
    assert_eq!(
        metrics(&after),
        (Some(HalfPoints(24)), Some(HalfPoints(20)), Some(90))
    );
    assert!(xml.contains(r#"<w:sz w:val="24"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:szCs w:val="20"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:w w:val="90%"/>"#), "{xml}");

    let mut changed = before;
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let Inline::Run(run) = &mut paragraph.inlines[0] else {
        panic!("expected a run");
    };
    run.props.size = Some(HalfPoints(23));
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let xml = document_xml(&open(&written.bytes).expect("reopen"));
    assert!(xml.contains(r#"<w:sz w:val="23"/>"#), "{xml}");
    assert!(!xml.contains(r#"<w:sz w:val="24"/>"#), "{xml}");
}

/// Placeholder `w:sdtPr/w:rPr` keeps the half-point size Word stores there.
#[test]
fn t_p5_1_sdt_placeholder_size_round_trips() {
    let body = "\
<w:sdt><w:sdtPr><w:rPr><w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/></w:rPr>\
<w:alias w:val=\"ph\"/></w:sdtPr><w:sdtContent>\
<w:p><w:r><w:rPr><w:sz w:val=\"24\"/></w:rPr><w:t>x</w:t></w:r></w:p>\
</w:sdtContent></w:sdt>";
    let (before, after, xml) = round_trip(body);
    let size = |document: &Document| {
        let Block::SdtBlock(sdt) = &document.body.blocks[0] else {
            panic!("expected a content control");
        };
        sdt.run_props.as_ref().and_then(|props| props.size)
    };
    assert_eq!(size(&before), Some(HalfPoints(28)));
    assert_eq!(size(&after), Some(HalfPoints(28)));
    let sdt_pr = xml.split("<w:sdtPr>").nth(1).expect("sdtPr");
    let sdt_pr = sdt_pr.split("</w:sdtPr>").next().expect("sdtPr end");
    assert!(sdt_pr.contains(r#"<w:sz w:val="28"/>"#), "{xml}");
    assert!(sdt_pr.contains(r#"<w:szCs w:val="28"/>"#), "{xml}");
    assert!(
        sdt_pr.find("<w:rPr>").unwrap() < sdt_pr.find("<w:alias").unwrap(),
        "CT_SdtPr writes rPr before alias: {sdt_pr}"
    );
}

/// T-P5-2: `12pt` and a bare twip count are one length, written back as twips.
#[test]
fn t_p5_2_spacing_points_round_trip_as_twips() {
    let body = "\
<w:p><w:pPr><w:spacing w:after=\"12pt\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:t>A</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip(body);
    let spacing = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .spacing
            .expect("spacing")
    };
    assert_eq!(spacing(&before).after, Some(Twips(240)));
    assert_eq!(spacing(&before).line, Some(Twips(240)));
    assert_eq!(spacing(&after).after, Some(Twips(240)));
    assert_eq!(spacing(&after).line, Some(Twips(240)));
    assert!(xml.contains(r#"w:after="240""#), "{xml}");
    assert!(xml.contains(r#"w:line="240""#), "{xml}");
    assert!(!xml.contains("12pt"), "{xml}");
}

/// T-P5-3: a tab stop keeps its position. Legacy `left` is written as `start`.
#[test]
fn t_p5_3_tab_position_round_trips() {
    let body = "\
<w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"1440\"/></w:tabs></w:pPr>\
<w:r><w:t>Title</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t>1</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip(body);
    let tab = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .tabs[0]
    };
    assert_eq!(tab(&before).position, Twips(1440));
    assert_eq!(tab(&before).alignment, TabAlignment::Start);
    assert_eq!(tab(&after).position, Twips(1440));
    assert_eq!(tab(&after).alignment, TabAlignment::Start);
    assert!(xml.contains(r#"w:pos="1440""#), "{xml}");
    assert!(xml.contains(r#"w:val="start""#), "{xml}");
}

/// `w:ind/@w:left` is the leading indent. Strict writes it as `w:start`.
#[test]
fn t_p5_indent_left_round_trips_as_start() {
    let body = "\
<w:p><w:pPr><w:ind w:left=\"720\" w:right=\"360\"/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip(body);
    let indent = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .indentation
            .expect("indent")
    };
    assert_eq!(indent(&before).start, Some(Twips(720)));
    assert_eq!(indent(&before).end, Some(Twips(360)));
    assert_eq!(indent(&after).start, Some(Twips(720)));
    assert_eq!(indent(&after).end, Some(Twips(360)));
    assert!(xml.contains(r#"w:start="720""#), "{xml}");
    assert!(xml.contains(r#"w:end="360""#), "{xml}");
    assert!(!xml.contains("w:left="), "{xml}");

    let mut changed = before;
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    paragraph.props.indentation.as_mut().expect("indent").start = Some(Twips(719));
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let xml = document_xml(&open(&written.bytes).expect("reopen"));
    assert!(xml.contains(r#"w:start="719""#), "{xml}");
    assert!(!xml.contains(r#"w:start="720""#), "{xml}");
}

/// Paragraph and table border widths stay eighths of a point.
#[test]
fn t_p6_border_width_round_trips() {
    let body = "\
<w:p><w:pPr><w:pBdr><w:top w:val=\"single\" w:sz=\"12\" w:space=\"1\" w:color=\"FF0000\"/>\
<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"000000\"/></w:pBdr></w:pPr>\
<w:r><w:t>A</w:t></w:r></w:p>\
<w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"000000\"/>\
<w:left w:val=\"single\" w:sz=\"6\" w:space=\"0\" w:color=\"000000\"/></w:tblBorders></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>";
    let (before, after, xml) = round_trip(body);
    let paragraph_top = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .borders
            .top
            .as_ref()
            .and_then(|border| border.size)
    };
    let table_top = |document: &Document| {
        document.body.blocks[1]
            .as_table()
            .expect("table")
            .props
            .borders
            .top
            .as_ref()
            .and_then(|border| border.size)
    };
    assert_eq!(paragraph_top(&before), Some(EighthsPoint(12)));
    assert_eq!(paragraph_top(&after), Some(EighthsPoint(12)));
    assert_eq!(table_top(&before), Some(EighthsPoint(8)));
    assert_eq!(table_top(&after), Some(EighthsPoint(8)));
    assert!(xml.contains(r#"w:sz="12""#), "{xml}");
    assert!(xml.contains(r#"w:sz="8""#), "{xml}");
    // Paragraph edges stay left/right. Table edges are start/end.
    assert!(xml.contains("<w:left "), "{xml}");
    assert!(xml.contains("<w:start "), "{xml}");

    let mut changed = before;
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    paragraph.props.borders.top.as_mut().expect("top").size = Some(EighthsPoint(11));
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let xml = document_xml(&open(&written.bytes).expect("reopen"));
    assert!(xml.contains(r#"w:sz="11""#), "{xml}");
    assert!(!xml.contains(r#"w:sz="12""#), "{xml}");
}

/// T-P6-1: grid columns stay twips; a dxa cell width written as points parses back.
#[test]
fn t_p6_1_grid_and_cell_widths_round_trip() {
    let body = "\
<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"3125\"/><w:gridCol w:w=\"1875\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"3125\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc>\
<w:tc><w:tcPr><w:tcW w:w=\"1875\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>";
    let (before, after, xml) = round_trip(body);
    let widths = |document: &Document| {
        let table = document.body.blocks[0].as_table().expect("table");
        (
            table.grid[0].width,
            table.grid[1].width,
            table.rows[0].cells[0].props.width.unwrap().value,
        )
    };
    assert_eq!(
        widths(&before),
        (Some(Twips(3125)), Some(Twips(1875)), Some(3125))
    );
    assert_eq!(
        widths(&after),
        (Some(Twips(3125)), Some(Twips(1875)), Some(3125))
    );
    assert!(xml.contains(r#"<w:gridCol w:w="3125"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:gridCol w:w="1875"/>"#), "{xml}");
    assert!(
        xml.contains(r#"w:w="156.25pt""#) && xml.contains(r#"w:type="dxa""#),
        "{xml}"
    );
}

/// A tracked `w:tblGridChange` keeps the previous column widths.
#[test]
fn t_p6_1_previous_grid_round_trips() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"1500\"/>\
<w:tblGridChange w:id=\"7\"><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2500\"/></w:tblGrid></w:tblGridChange>\
</w:tblGrid><w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>";
    let (before, after, xml) = round_trip(body);
    let change = |document: &Document| {
        document.body.blocks[0]
            .as_table()
            .expect("table")
            .grid_change
            .clone()
            .expect("grid change")
    };
    assert_eq!(change(&before).id, 7);
    assert_eq!(
        change(&before)
            .grid
            .iter()
            .map(|column| column.width)
            .collect::<Vec<_>>(),
        vec![Some(Twips(2000)), Some(Twips(2500))]
    );
    assert_eq!(change(&after).id, 7);
    assert_eq!(
        change(&after)
            .grid
            .iter()
            .map(|column| column.width)
            .collect::<Vec<_>>(),
        vec![Some(Twips(2000)), Some(Twips(2500))]
    );
    assert!(xml.contains(r#"<w:tblGridChange w:id="7">"#), "{xml}");
    assert!(xml.contains(r#"<w:gridCol w:w="2000"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:gridCol w:w="3000"/>"#), "{xml}");
}

/// T-P6-3: shrinking a column by one twip is a different width.
#[test]
fn t_p6_3_one_twip_is_a_different_column() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"3125\"/></w:tblGrid>\
<w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>";
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let original = parse(&package).expect("parse");
    let mut shrunk = original.clone();
    let Block::Table(table) = &mut shrunk.body.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(table.grid[0].width, Some(Twips(3125)));
    table.grid[0].width = Some(Twips(3124));
    assert_ne!(
        original.body.blocks[0].as_table().unwrap().grid[0].width,
        shrunk.body.blocks[0].as_table().unwrap().grid[0].width
    );
    let written = write_package(&shrunk, Some(&package), &WriteOptions::default()).expect("write");
    let xml = document_xml(&open(&written.bytes).expect("reopen"));
    assert!(xml.contains(r#"<w:gridCol w:w="3124"/>"#), "{xml}");
    assert!(!xml.contains(r#"w:w="3125""#), "{xml}");
}

/// Explicit off, line-count spacing, and a legacy character indent survive.
#[test]
fn t_p5_spacing_flags_and_character_indent_round_trip() {
    let body = "\
<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:beforeLines=\"60\" w:afterLines=\"40\" \
w:beforeAutospacing=\"0\" w:afterAutospacing=\"false\" w:line=\"240\" w:lineRule=\"auto\"/>\
<w:ind w:left=\"720\" w:leftChars=\"0\" w:rightChars=\"-25\"/></w:pPr>\
<w:r><w:rPr><w:spacing w:val=\"0\"/></w:rPr><w:t>A</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip(body);
    let spacing = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .spacing
            .expect("spacing")
    };
    let indent = |document: &Document| {
        document.body.blocks[0]
            .as_paragraph()
            .expect("paragraph")
            .props
            .indentation
            .expect("indent")
    };
    assert_eq!(spacing(&before).before_lines, Some(60));
    assert_eq!(spacing(&before).after_lines, Some(40));
    assert_eq!(spacing(&before).before_autospacing, Some(false));
    assert_eq!(spacing(&before).after_autospacing, Some(false));
    assert_eq!(spacing(&after).before_lines, Some(60));
    assert_eq!(spacing(&after).after_lines, Some(40));
    assert_eq!(spacing(&after).before_autospacing, Some(false));
    assert_eq!(indent(&before).start_chars, Some(0));
    assert_eq!(indent(&before).end_chars, Some(-25));
    assert_eq!(indent(&after).start_chars, Some(0));
    assert_eq!(indent(&after).end_chars, Some(-25));
    assert!(xml.contains(r#"w:beforeLines="60""#), "{xml}");
    assert!(xml.contains(r#"w:afterLines="40""#), "{xml}");
    assert!(xml.contains(r#"w:beforeAutospacing="false""#), "{xml}");
    assert!(xml.contains(r#"w:afterAutospacing="false""#), "{xml}");
    assert!(xml.contains(r#"w:startChars="0""#), "{xml}");
    assert!(xml.contains(r#"w:endChars="-25""#), "{xml}");
    assert!(xml.contains(r#"<w:spacing w:val="0"/>"#), "{xml}");
}

/// A row's `w:tblPrEx` borders are not the table's borders.
#[test]
fn t_p6_row_exception_border_round_trips() {
    let body = "\
<w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:tblPrEx><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
<w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
</w:tblBorders></w:tblPrEx><w:tc><w:p/></w:tc></w:tr></w:tbl>";
    let (before, after, xml) = round_trip(body);
    let size = |document: &Document| {
        document.body.blocks[0].as_table().expect("table").rows[0]
            .props
            .exception_borders
            .top
            .as_ref()
            .and_then(|border| border.size)
    };
    assert_eq!(size(&before), Some(EighthsPoint(4)));
    assert_eq!(size(&after), Some(EighthsPoint(4)));
    assert!(xml.contains("<w:tblPrEx>"), "{xml}");
    assert!(xml.contains(r#"w:sz="4""#), "{xml}");
    assert!(xml.contains(r#"w:space="0""#), "{xml}");
    assert!(xml.contains("<w:start "), "{xml}");
}

/// `w:sdtEndPr/w:rPr/w:sz` is the end marker, distinct from the placeholder size.
#[test]
fn t_p5_sdt_end_marker_size_round_trips() {
    let body = "\
<w:sdt>\
<w:sdtPr><w:rPr><w:sz w:val=\"28\"/></w:rPr></w:sdtPr>\
<w:sdtEndPr><w:rPr><w:sz w:val=\"20\"/></w:rPr></w:sdtEndPr>\
<w:sdtContent><w:p><w:r><w:t>End</w:t></w:r></w:p></w:sdtContent>\
</w:sdt>";
    let (before, after, xml) = round_trip(body);
    let end_size = |document: &Document| {
        let Block::SdtBlock(sdt) = &document.body.blocks[0] else {
            panic!("expected a structured document tag");
        };
        sdt.end_run_props.as_ref().and_then(|props| props.size)
    };
    assert_eq!(end_size(&before), Some(HalfPoints(20)));
    assert_eq!(end_size(&after), Some(HalfPoints(20)));
    assert!(xml.contains("<w:sdtEndPr>"), "{xml}");
    assert!(xml.contains(r#"<w:sz w:val="20"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:sz w:val="28"/>"#), "{xml}");

    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let mut changed = parse(&package).expect("parse");
    let Block::SdtBlock(sdt) = &mut changed.body.blocks[0] else {
        panic!("expected a structured document tag");
    };
    sdt.end_run_props.as_mut().expect("end props").size = Some(HalfPoints(19));
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    let negative = document_xml(&reopened);
    assert!(negative.contains(r#"<w:sz w:val="19"/>"#), "{negative}");
    assert!(!negative.contains(r#"<w:sz w:val="20"/>"#), "{negative}");
}

/// A subscript control size stays under `m:sSubPr`, not a superscript property.
#[test]
fn t_p5_subscript_control_size_keeps_its_parent() {
    use strict_ooxml_wml::model::math::MathNode;

    let body = "\
<w:p><m:oMath><m:sSub>\
<m:sSubPr><m:ctrlPr><w:rPr><w:sz w:val=\"22\"/></w:rPr></m:ctrlPr></m:sSubPr>\
<m:e><m:r><m:t>x</m:t></m:r></m:e>\
<m:sub><m:r><m:t>i</m:t></m:r></m:sub>\
</m:sSub></m:oMath></w:p>";
    let (before, _, xml) = round_trip(body);
    assert!(xml.contains("<m:sSubPr>"), "{xml}");
    assert!(xml.contains(r#"<w:sz w:val="22"/>"#), "{xml}");
    assert!(!xml.contains("<m:sSupPr>"), "{xml}");

    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let mut changed = parse(&package).expect("parse");
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("expected a paragraph");
    };
    let Inline::Math(expression) = &mut paragraph.inlines[0] else {
        panic!("expected a formula");
    };
    let MathNode::Subscript(script) = &mut expression.nodes[0] else {
        panic!("expected a subscript");
    };
    let before_size = {
        let Block::Paragraph(paragraph) = &before.body.blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Math(expression) = &paragraph.inlines[0] else {
            panic!("expected a formula");
        };
        let MathNode::Subscript(script) = &expression.nodes[0] else {
            panic!("expected a subscript");
        };
        script.control.as_ref().and_then(|props| props.size)
    };
    assert_eq!(before_size, Some(HalfPoints(22)));
    script.control.as_mut().expect("control").size = Some(HalfPoints(21));
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    let negative = document_xml(&reopened);
    assert!(negative.contains("<m:sSubPr>"), "{negative}");
    assert!(negative.contains(r#"<w:sz w:val="21"/>"#), "{negative}");
    assert!(!negative.contains(r#"<w:sz w:val="22"/>"#), "{negative}");
}
