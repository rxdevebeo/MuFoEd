#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Coverage tests for property parsing (`pPr`/`rPr`/`tblPr`/`trPr`/`tcPr`/`sectPr`).

mod common;

use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{
    BorderStyle, BreakKind, HeightRule, Highlight, Justification, LineSpacingRule, TabAlignment,
    TabLeader, TextDirection, TriState, Underline, VerticalJc, VerticalMerge, WidthKind,
};

use common::{document_parts, parse_parts};

fn paragraph_inline(body: &str) -> strict_ooxml_wml::model::Document {
    parse_parts(&document_parts(body, &[])).expect("parse")
}

fn run(document: &strict_ooxml_wml::model::Document) -> &Run {
    let Inline::Run(run) = &document.body.blocks[0].as_paragraph().unwrap().inlines[0] else {
        panic!("expected run");
    };
    run
}

#[test]
fn parses_rich_paragraph_properties() {
    let body = "<w:p><w:pPr>\
<w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\" w:shadow=\"true\"/>\
<w:start w:val=\"dashed\"/><w:end w:val=\"dotDash\"/><w:bottom w:val=\"double\"/>\
<w:insideH w:val=\"nil\"/><w:insideV w:val=\"wave\"/></w:pBdr>\
<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FFFF00\"/>\
<w:tabs><w:tab w:pos=\"720\" w:val=\"center\" w:leader=\"dot\"/><w:tab w:pos=\"-1\" w:val=\"clear\"/></w:tabs>\
<w:spacing w:before=\"120\" w:after=\"240\" w:line=\"360\" w:lineRule=\"exact\" w:beforeAutospacing=\"1\" w:afterAutospacing=\"on\"/>\
<w:ind w:start=\"720\" w:end=\"360\" w:hanging=\"180\" w:startChars=\"100\" w:endChars=\"50\" w:firstLineChars=\"0\" w:hangingChars=\"25\"/>\
<w:jc w:val=\"both\"/><w:outlineLvl w:val=\"2\"/><w:textDirection w:val=\"rl\"/>\
<w:wordWrap w:val=\"false\"/><w:snapToGrid w:val=\"true\"/><w:widowControl w:val=\"0\"/>\
<w:bidi/><w:contextualSpacing/><w:suppressLineNumbers/><w:keepLines/><w:keepNext/><w:pageBreakBefore/>\
<w:numPr><w:ilvl w:val=\"3\"/><w:numId w:val=\"9\"/></w:numPr>\
</w:pPr><w:r><w:t>x</w:t></w:r></w:p>";
    let document = paragraph_inline(body);
    let props = &document.body.blocks[0].as_paragraph().unwrap().props;
    let borders = &props.borders;
    assert_eq!(
        borders.top.as_ref().unwrap().style,
        Some(BorderStyle::Single)
    );
    assert!(borders.top.as_ref().unwrap().shadow);
    assert_eq!(
        borders.start.as_ref().unwrap().style,
        Some(BorderStyle::Dashed)
    );
    assert_eq!(
        borders.end.as_ref().unwrap().style,
        Some(BorderStyle::DotDash)
    );
    assert_eq!(
        borders.bottom.as_ref().unwrap().style,
        Some(BorderStyle::Double)
    );
    assert_eq!(
        borders.inside_horizontal.as_ref().unwrap().style,
        Some(BorderStyle::Nil)
    );
    assert_eq!(
        borders.inside_vertical.as_ref().unwrap().style,
        Some(BorderStyle::Wave)
    );
    let shading = props.shading.as_ref().unwrap();
    assert_eq!(shading.fill.as_ref().unwrap().as_str(), "FFFF00");
    assert_eq!(props.tabs.len(), 2);
    assert_eq!(props.tabs[0].position.value(), 720);
    assert_eq!(props.tabs[0].alignment, TabAlignment::Center);
    assert_eq!(props.tabs[0].leader, Some(TabLeader::Dot));
    assert_eq!(props.tabs[1].position.value(), -1);
    assert_eq!(props.tabs[1].alignment, TabAlignment::Clear);
    let spacing = props.spacing.unwrap();
    assert_eq!(spacing.line_rule, Some(LineSpacingRule::Exact));
    assert_eq!(spacing.before_autospacing, Some(true));
    assert_eq!(spacing.after_autospacing, Some(true));
    let indentation = props.indentation.unwrap();
    assert_eq!(indentation.start.unwrap().value(), 720);
    assert_eq!(indentation.end.unwrap().value(), 360);
    assert_eq!(indentation.start_chars, Some(100));
    assert_eq!(indentation.hanging_chars, Some(25));
    assert_eq!(props.alignment, Some(Justification::Both));
    assert_eq!(props.outline_level, Some(2));
    assert_eq!(props.text_direction, Some(TextDirection::TbRl));
    assert_eq!(props.word_wrap, TriState::Off);
    assert_eq!(props.snap_to_grid, TriState::On);
    assert_eq!(props.widow_control, TriState::Off);
    assert!(
        props.bidi.is_on()
            && props.contextual_spacing.is_on()
            && props.suppress_line_numbers.is_on()
    );
    assert!(props.keep_lines.is_on() && props.keep_next.is_on() && props.page_break_before.is_on());
    let numbering = props.numbering.unwrap();
    assert_eq!(numbering.ilvl.unwrap().0, 3);
    assert_eq!(numbering.num_id.unwrap().0, 9);
}

#[test]
fn parses_rich_run_properties() {
    let body = "<w:p><w:r><w:rPr>\
<w:u w:val=\"wave\" w:color=\"0000FF\"/><w:dstrike/>\
<w:highlight w:val=\"yellow\"/><w:szCs w:val=\"32\"/><w:vertAlign w:val=\"superscript\"/>\
<w:spacing w:val=\"20\"/><w:position w:val=\"6\"/><w:w w:val=\"90\"/><w:kern w:val=\"16\"/>\
<w:em w:val=\"dot\"/><w:lang w:val=\"en-US\" w:eastAsia=\"ja-JP\" w:bidi=\"ar-SA\"/>\
<w:caps/><w:smallCaps/><w:rtl/><w:vanish/><w:emboss/><w:imprint/><w:outline/><w:shadow/>\
<w:noProof/><w:snapToGrid w:val=\"false\"/><w:shd w:val=\"clear\" w:fill=\"EEEEEE\"/><w:bdr w:val=\"single\"/>\
<w:strike w:val=\"false\"/><w:i/><w:b w:val=\"1\"/>\
</w:rPr><w:t>x</w:t></w:r></w:p>";
    let document = paragraph_inline(body);
    let run = run(&document);
    assert_eq!(run.props.underline, Some(Underline::Wave));
    assert_eq!(
        run.props.underline_color.as_ref().unwrap().as_str(),
        "0000FF"
    );
    assert_eq!(run.props.double_strike, TriState::On);
    assert_eq!(run.props.highlight, Some(Highlight::Yellow));
    assert_eq!(run.props.size_cs.unwrap().value(), 32);
    assert_eq!(
        run.props.vert_align,
        Some(strict_ooxml_wml::model::values::VertAlign::Superscript)
    );
    assert_eq!(run.props.spacing.unwrap().value(), 20);
    assert_eq!(run.props.position.unwrap().value(), 6);
    assert_eq!(run.props.scale, Some(90));
    assert_eq!(run.props.kerning.unwrap().value(), 16);
    assert_eq!(run.props.emphasis.as_deref(), Some("dot"));
    let language = run.props.language.as_ref().unwrap();
    assert_eq!(language.val.as_deref(), Some("en-US"));
    assert_eq!(language.east_asia.as_deref(), Some("ja-JP"));
    assert_eq!(language.bidi.as_deref(), Some("ar-SA"));
    assert!(
        run.props.caps.is_on()
            && run.props.small_caps.is_on()
            && run.props.rtl.is_on()
            && run.props.vanish.is_on()
    );
    assert!(
        run.props.emboss.is_on()
            && run.props.imprint.is_on()
            && run.props.outline.is_on()
            && run.props.shadow.is_on()
    );
    assert!(run.props.no_proof.is_on());
    assert_eq!(run.props.snap_to_grid, TriState::Off);
    assert_eq!(run.props.strike, TriState::Off);
    assert_eq!(run.props.italic, TriState::On);
    assert_eq!(run.props.bold, TriState::On);
    assert!(document.support.get("w:bdr").is_some());
}

#[test]
fn parses_rich_table_properties() {
    let body = "<w:tbl><w:tblPr>\
<w:tblStyle w:val=\"Grid\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/>\
<w:jc w:val=\"end\"/><w:tblLayout w:type=\"fixed\"/><w:tblInd w:w=\"120\"/>\
<w:bidiVisual/><w:shd w:val=\"clear\" w:fill=\"DDDDDD\"/>\
<w:tblBorders><w:top w:val=\"single\"/><w:bottom w:val=\"single\"/><w:start w:val=\"single\"/><w:end w:val=\"single\"/></w:tblBorders>\
<w:tblCellMar><w:top w:w=\"10\"/><w:start w:w=\"20\"/><w:bottom w:w=\"10\"/><w:end w:w=\"20\"/></w:tblCellMar>\
<w:tblLook w:firstRow=\"1\" w:lastRow=\"1\" w:firstColumn=\"1\" w:lastColumn=\"1\" w:noHBand=\"0\" w:noVBand=\"1\"/>\
</w:tblPr><w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>\
<w:tr><w:trPr><w:trHeight w:val=\"400\" w:hRule=\"exact\"/><w:tblHeader/><w:cantSplit/>\
<w:gridBefore w:val=\"1\"/><w:gridAfter w:val=\"1\"/><w:wBefore w:w=\"100\" w:type=\"dxa\"/><w:wAfter w:w=\"100\" w:type=\"dxa\"/>\
<w:rsid w:val=\"00AB\"/><w:tblCellMar><w:top w:w=\"5\"/></w:tblCellMar></w:trPr>\
<w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/><w:gridSpan w:val=\"2\"/><w:vMerge w:val=\"continue\"/>\
<w:vAlign w:val=\"center\"/><w:textDirection w:val=\"lr\"/><w:noWrap/><w:hideMark/><w:tcFitText/>\
<w:tcBorders><w:top w:val=\"single\"/></w:tcBorders><w:shd w:val=\"clear\" w:fill=\"FFFFFF\"/>\
<w:tcMar><w:top w:w=\"11\"/><w:start w:w=\"22\"/><w:bottom w:w=\"11\"/><w:end w:w=\"22\"/></w:tcMar>\
</w:tcPr><w:p/></w:tc></w:tr></w:tbl>";
    let document = paragraph_inline(body);
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    let props = &table.props;
    assert_eq!(props.style.as_ref().unwrap().as_str(), "Grid");
    assert_eq!(props.width.unwrap().kind, WidthKind::Pct);
    assert_eq!(props.alignment, Some(Justification::End));
    assert_eq!(
        props.layout,
        Some(strict_ooxml_wml::model::values::TableLayout::Fixed)
    );
    assert_eq!(props.indent.unwrap().value(), 120);
    assert!(props.bidi_visual);
    assert_eq!(props.cell_margins.top.unwrap().value(), 10);
    let look = props.look.unwrap();
    assert!(look.first_row && look.last_row && look.first_column && look.last_column);
    assert!(!look.no_h_band && look.no_v_band);

    let row = &table.rows[0];
    assert_eq!(row.props.height.unwrap().rule, Some(HeightRule::Exact));
    assert!(row.props.header && row.props.cant_split);
    assert_eq!(row.props.grid_before, Some(1));
    assert_eq!(row.props.grid_after, Some(1));
    assert_eq!(row.props.rsid.as_deref(), Some("00AB"));
    assert_eq!(row.props.cell_margins.top.unwrap().value(), 5);
    let cell = &row.cells[0];
    assert_eq!(cell.props.width.unwrap().kind, WidthKind::Dxa);
    assert_eq!(cell.props.grid_span, Some(2));
    assert_eq!(cell.props.vertical_merge, Some(VerticalMerge::Continue));
    assert_eq!(cell.props.vertical_align, Some(VerticalJc::Center));
    assert_eq!(cell.props.text_direction, Some(TextDirection::BtLr));
    assert!(cell.props.no_wrap && cell.props.hide_mark && cell.props.fit_text);
    assert_eq!(cell.props.margins.top.unwrap().value(), 11);
    assert_eq!(
        cell.props
            .shading
            .as_ref()
            .unwrap()
            .fill
            .as_ref()
            .unwrap()
            .as_str(),
        "FFFFFF"
    );
}

#[test]
fn parses_richer_section_properties() {
    let body = "<w:p/><w:sectPr>\
<w:lnNumType w:countBy=\"5\" w:start=\"1\" w:restart=\"newPage\" w:distance=\"240\"/>\
<w:rtlGutter/><w:gutterAtTop/><w:bidi/><w:vAlign w:val=\"bottom\"/><w:textDirection w:val=\"tb\"/>\
<w:pgBorders><w:top w:val=\"single\"/></w:pgBorders>\
<w:cols w:num=\"2\" w:space=\"425\" w:equalWidth=\"false\" w:sep=\"true\"><w:col w:w=\"4000\"/></w:cols>\
</w:sectPr>";
    let document = paragraph_inline(body);
    let section = &document.sections[0].properties;
    let line = section.line_numbering.unwrap();
    assert_eq!(line.count_by, Some(5));
    assert_eq!(line.start, Some(1));
    assert_eq!(line.distance.unwrap().value(), 240);
    assert!(section.rtl_gutter && section.bidi);
    // `w:gutterAtTop` is a document setting in Strict and a section child in
    // Transitional, so a `w:sectPr` that carries it sets `Settings`, not the
    // section. See `Settings::gutter_at_top`.
    assert!(document.settings.gutter_at_top);
    assert_eq!(section.vertical_align, Some(VerticalJc::Bottom));
    assert_eq!(section.text_direction, Some(TextDirection::LrTb));
    let columns = section.columns.as_ref().unwrap();
    assert!(!columns.equal_width);
    assert!(columns.separator);
    assert_eq!(columns.columns.len(), 1);
    // pgBorders is parsed in Stage 5B.
    let borders = section.page_borders.as_ref().expect("page borders");
    assert_eq!(
        borders.top.as_ref().unwrap().style,
        Some(strict_ooxml_wml::model::values::BorderStyle::Single)
    );
    assert_eq!(
        document.support.get("w:pgBorders").unwrap().status,
        SupportStatus::Supported
    );
}

#[test]
fn parses_run_content_variants() {
    let body =
        "<w:p><w:r><w:delText>gone</w:delText><w:cr/><w:sym w:font=\"Wingdings\" w:char=\"0041\"/>\
<w:lastRenderedPageBreak/><w:noBreakHyphen/><w:softHyphen/>\
<w:fldChar w:fldCharType=\"separate\" w:dirty=\"true\"/></w:r></w:p>";
    let document = paragraph_inline(body);
    let run = run(&document);
    assert!(matches!(run.content[0], RunContent::Text(_)));
    assert!(matches!(run.content[1], RunContent::CarriageReturn));
    match &run.content[2] {
        RunContent::Symbol(symbol) => {
            assert_eq!(symbol.font.as_ref(), "Wingdings");
            assert_eq!(symbol.character, 'A');
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(run.content[3], RunContent::LastRenderedPageBreak));
    assert!(matches!(run.content[4], RunContent::NoBreakHyphen));
    assert!(matches!(run.content[5], RunContent::SoftHyphen));
    match &run.content[6] {
        RunContent::FieldChar(field) => {
            assert_eq!(
                field.kind,
                strict_ooxml_wml::model::values::FieldCharType::Separate
            );
            assert!(field.dirty);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn tab_stops_use_schema_attributes() {
    // CT_TabStop: position = `w:pos`, alignment = `w:val` (ST_TabJc), fill =
    // `w:leader`; there is no `w:jc` on `w:tab` (D-1).
    let body = "<w:p><w:pPr><w:tabs>\
<w:tab w:val=\"right\" w:leader=\"none\" w:pos=\"9360\"/>\
<w:tab w:val=\"center\" w:pos=\"4680\"/>\
<w:tab w:val=\"num\" w:pos=\"720\"/>\
<w:tab w:val=\"clear\" w:pos=\"0\"/>\
<w:tab w:val=\"decimal\" w:pos=\"1000\"/>\
<w:tab w:val=\"bar\" w:pos=\"2000\"/>\
<w:tab w:val=\"start\" w:pos=\"3000\"/>\
<w:tab w:val=\"end\" w:pos=\"4000\"/>\
</w:tabs></w:pPr></w:p>";
    let document = paragraph_inline(body);
    let tabs = &document.body.blocks[0].as_paragraph().unwrap().props.tabs;
    assert_eq!(tabs.len(), 8);
    assert_eq!(tabs[0].position.value(), 9360);
    assert_eq!(tabs[0].alignment, TabAlignment::End); // legacy `right` -> end
    assert_eq!(tabs[0].leader, Some(TabLeader::None));
    assert_eq!(tabs[1].alignment, TabAlignment::Center);
    assert_eq!(tabs[2].alignment, TabAlignment::Num);
    assert_eq!(tabs[3].alignment, TabAlignment::Clear);
    assert_eq!(tabs[4].alignment, TabAlignment::Decimal);
    assert_eq!(tabs[5].alignment, TabAlignment::Bar);
    assert_eq!(tabs[6].alignment, TabAlignment::Start);
    assert_eq!(tabs[7].alignment, TabAlignment::End);
    assert!(document.support.get("w:tab").is_none(), "unexpected loss");
}

#[test]
fn tab_stop_losses_are_recorded_not_dropped() {
    let body = "<w:p><w:pPr><w:tabs><w:tab w:val=\"bogus\" w:pos=\"nope\"/></w:tabs></w:pPr></w:p>";
    let document = paragraph_inline(body);
    let tabs = &document.body.blocks[0].as_paragraph().unwrap().props.tabs;
    // The stop is retained; the unreadable values are recorded.
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].position.value(), 0);
    assert_eq!(tabs[0].alignment, TabAlignment::Start);
    assert_eq!(
        document.support.get("w:tab").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn decimal_measurements_are_applied_not_lost() {
    let body = "<w:p><w:pPr><w:ind w:start=\"1872.0000000000002\" w:hanging=\"180.5\"/>\
<w:spacing w:before=\"12.5\"/></w:pPr></w:p>\
<w:tbl><w:tblPr><w:tblW w:w=\"1872.0000000000002\" w:type=\"dxa\"/>\
<w:tblInd w:w=\"-180.0\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2500.5\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"50%\" w:type=\"pct\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>\
<w:sectPr><w:pgMar w:left=\"1872.0000000000002\" w:right=\"17.99999999999983\"/>\
<w:pgSz w:w=\"11906.0\" w:h=\"16838.0\"/></w:sectPr>";
    let document = paragraph_inline(body);
    let props = &document.body.blocks[0].as_paragraph().unwrap().props;
    assert_eq!(props.indentation.unwrap().start.unwrap().value(), 1872);
    assert_eq!(props.indentation.unwrap().hanging.unwrap().value(), 181);
    assert_eq!(props.spacing.unwrap().before.unwrap().value(), 13);
    let Block::Table(table) = &document.body.blocks[1] else {
        panic!("expected table");
    };
    assert_eq!(table.props.width.unwrap().value, Some(1872));
    assert_eq!(table.props.indent.unwrap().value(), -180);
    assert_eq!(table.grid[0].width.unwrap().value(), 2501);
    // `50%` -> fiftieths of a percent.
    assert_eq!(
        table.rows[0].cells[0].props.width.unwrap().value,
        Some(2500)
    );
    let section = &document.sections[0].properties;
    assert_eq!(section.page_margins.unwrap().left.unwrap().value(), 1872);
    assert_eq!(section.page_margins.unwrap().right.unwrap().value(), 18);
    assert_eq!(section.page_size.unwrap().width.unwrap().value(), 11906);
    assert!(document.support.is_empty(), "no losses expected");
}

#[test]
fn percent_measurements_are_applied() {
    // `ST_MeasurementOrPercent` applies to `w:tblInd` as well as `w:tblW`.
    let body = "<w:tbl><w:tblPr><w:tblInd w:w=\"50%\" w:type=\"pct\"/>\
<w:tblW w:w=\"50%\" w:type=\"pct\"/></w:tblPr></w:tbl>";
    let document = paragraph_inline(body);
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    assert_eq!(table.props.indent.unwrap().value(), 2500);
    assert_eq!(table.props.width.unwrap().value, Some(2500));
    assert!(document.support.is_empty(), "no losses expected");
}

#[test]
fn unreadable_measurements_are_recorded() {
    let body = "<w:tbl><w:tblPr><w:tblInd w:w=\"abc\"/></w:tblPr></w:tbl>";
    let document = paragraph_inline(body);
    assert_eq!(
        document.support.get("w:tblInd").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn records_invalid_numeric_and_enum_values() {
    let body = "<w:p><w:pPr><w:outlineLvl w:val=\"abc\"/></w:pPr>\
<w:r><w:rPr><w:sz w:val=\"notanumber\"/></w:rPr><w:t>x</w:t></w:r></w:p>\
<w:tbl><w:tblGrid><w:gridCol w:w=\"bad\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"x\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>";
    let document = paragraph_inline(body);
    assert_eq!(
        document.support.get("w:outlineLvl").unwrap().status,
        SupportStatus::Partial
    );
    assert_eq!(
        document.support.get("w:sz").unwrap().status,
        SupportStatus::Partial
    );
    assert_eq!(
        document.support.get("w:gridSpan").unwrap().status,
        SupportStatus::Partial
    );
    // The unknown break type falls back to textWrapping.
    let document = paragraph_inline("<w:p><w:r><w:br w:type=\"bogus\"/></w:r></w:p>");
    let run = run(&document);
    assert!(matches!(
        run.content[0],
        RunContent::Break(BreakKind::TextWrapping)
    ));
}
