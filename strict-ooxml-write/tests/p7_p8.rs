//! P7–P8: theme colours and explicit colours survive a Strict rewrite.
//!
//! Positive controls keep `themeColor`/`themeTint`/`themeShade`, `themeFill*`,
//! hex `w:color/@val`, and DrawingML `a:schemeClr/@val`. Negative controls
//! change one slot or one hex digit and require the written package to show
//! that change. Lexical theme drop at the same hex is not equivalence (plan §8).

use std::path::{Path, PathBuf};

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::Inline;
use strict_ooxml_wml::model::values::{Color, ThemeColor, ThemeColorRef};
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

/// A document from the gitignored local corpus, or `None` (with a loud skip
/// line) when this checkout does not carry it.
fn local_corpus(relative: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!(
            "SKIP: local corpus document not present: {}",
            path.display()
        );
        None
    }
}

fn open(bytes: &[u8]) -> Result<Package, StrictError> {
    Package::open_reader(bytes, &OpenOptions::default())
}

fn parse(package: &Package) -> Result<Document, StrictError> {
    parse_document(package, &ParseOptions::default())
}

fn part_xml(package: &Package, part: &str) -> String {
    String::from_utf8(
        package
            .read_part(&PartId::new(part))
            .unwrap_or_else(|_| panic!("missing {part}")),
    )
    .expect("utf-8")
}

fn round_trip_styles(styles_inner: &str) -> (Document, String) {
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body("<w:p><w:r><w:t>x</w:t></w:r></w:p>")
        .rel("rIdStyles", "styles", "styles.xml")
        .content_type(
            "/word/styles.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        )
        .part_xml("word/styles.xml", "w:styles", styles_inner)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    (document, part_xml(&reopened, "/word/styles.xml"))
}

fn round_trip_body(body: &str) -> (Document, Document, String) {
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(body)
        .build();
    let package = open(&bytes).expect("open");
    let document = parse(&package).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    let reparsed = parse(&reopened).expect("reparse");
    (
        document,
        reparsed,
        part_xml(&reopened, "/word/document.xml"),
    )
}

/// T-P7-1: themeColor + tint/shade round-trip on run colour and table borders.
#[test]
fn t_p7_1_theme_color_tint_shade_round_trip() {
    let styles = "\
<w:style w:type=\"table\" w:styleId=\"Banded\">\
<w:name w:val=\"Banded\"/>\
<w:tblPr><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"4472C4\" w:themeColor=\"accent1\"/>\
<w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"4472C4\" w:themeColor=\"accent1\" w:themeShade=\"80\"/>\
</w:tblBorders></w:tblPr>\
<w:tcPr><w:tcBorders>\
<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"ED7D31\" w:themeColor=\"accent2\" w:themeTint=\"99\"/>\
</w:tcBorders>\
<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"D6DCE5\" w:themeFill=\"accent1\" w:themeFillTint=\"40\"/>\
</w:tcPr>\
<w:tblStylePr w:type=\"firstRow\">\
<w:rPr><w:color w:val=\"365F91\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/></w:rPr>\
<w:tcPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"4472C4\" w:themeFill=\"accent1\"/></w:tcPr>\
</w:tblStylePr>\
</w:style>";
    let (document, xml) = round_trip_styles(styles);
    let style = document
        .styles
        .iter()
        .find(|style| style.id.as_str() == "Banded")
        .expect("Banded");
    let top = style.table.borders.top.as_ref().expect("top");
    assert_eq!(
        top.theme_color.as_ref().map(|t| t.color.as_str()),
        Some("accent1")
    );
    let bottom = style.table.borders.bottom.as_ref().expect("bottom");
    assert_eq!(
        bottom.theme_color.as_ref().and_then(|t| t.shade.as_deref()),
        Some("80")
    );
    let fill = style.cell.shading.as_ref().expect("cell shd");
    assert_eq!(
        fill.theme_fill.as_ref().map(|t| t.color.as_str()),
        Some("accent1")
    );
    assert_eq!(
        fill.theme_fill.as_ref().and_then(|t| t.tint.as_deref()),
        Some("40")
    );
    let condition = &style.conditions[0];
    assert_eq!(
        condition.run.color_theme.as_ref().map(|t| t.color.as_str()),
        Some("accent1")
    );
    assert!(
        xml.contains(r#"w:themeColor="accent1""#),
        "themeColor kept: {xml}"
    );
    assert!(
        xml.contains(r#"w:themeShade="80""#),
        "themeShade kept: {xml}"
    );
    assert!(
        xml.contains(r#"w:themeFill="accent1""#),
        "themeFill kept: {xml}"
    );
    assert!(
        xml.contains(r#"w:themeFillTint="40""#),
        "themeFillTint kept: {xml}"
    );
    assert!(
        xml.contains(r#"w:themeTint="99""#),
        "border themeTint kept: {xml}"
    );
}

/// T-P7-2: same hex without themeColor is a different model (not a declared transform).
#[test]
fn t_p7_2_lexical_theme_drop_is_not_equivalent() {
    let body = "\
<w:p><w:r><w:rPr><w:color w:val=\"4472C4\" w:themeColor=\"accent1\"/></w:rPr>\
<w:t>c</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip_body(body);
    let theme = |document: &Document| {
        let paragraph = document.body.blocks[0].as_paragraph().expect("p");
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("run");
        };
        run.props.color_theme.clone()
    };
    assert!(theme(&before).is_some());
    assert!(theme(&after).is_some());
    assert!(xml.contains(r#"w:themeColor="accent1""#), "{xml}");

    let mut stripped = before;
    let Block::Paragraph(paragraph) = &mut stripped.body.blocks[0] else {
        panic!("p");
    };
    let Inline::Run(run) = &mut paragraph.inlines[0] else {
        panic!("run");
    };
    run.props.color_theme = None;
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written =
        write_package(&stripped, Some(&package), &WriteOptions::default()).expect("write");
    let xml = part_xml(&open(&written.bytes).expect("reopen"), "/word/document.xml");
    assert!(
        xml.contains(r#"w:val="4472C4""#) || xml.contains(r#"w:val="4472c4""#),
        "{xml}"
    );
    assert!(
        !xml.contains("themeColor"),
        "dropping themeColor must be visible in XML: {xml}"
    );
}

/// T-P7-3: accent1 → accent2 is a failing change.
#[test]
fn t_p7_3_accent_slot_change_is_visible() {
    let body = "\
<w:p><w:r><w:rPr><w:color w:val=\"4472C4\" w:themeColor=\"accent1\"/></w:rPr>\
<w:t>c</w:t></w:r></w:p>";
    let (before, _, _) = round_trip_body(body);
    let mut changed = before;
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("p");
    };
    let Inline::Run(run) = &mut paragraph.inlines[0] else {
        panic!("run");
    };
    run.props.color_theme = Some(ThemeColorRef {
        color: ThemeColor::new("accent2"),
        tint: None,
        shade: None,
    });
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let xml = part_xml(&open(&written.bytes).expect("reopen"), "/word/document.xml");
    assert!(xml.contains(r#"w:themeColor="accent2""#), "{xml}");
    assert!(!xml.contains(r#"w:themeColor="accent1""#), "{xml}");
}

/// Contoso styles: themeColor / themeFill token counts survive a Strict rewrite.
///
/// Counts are substring occurrences in `word/styles.xml` (themeFill includes
/// themeFillTint/Shade), matching the P7 receipt note (1277 / 784).
#[test]
fn t_p7_contoso_styles_theme_token_counts_match_source() {
    let Some(path) =
        local_corpus("../strict-ooxml-core/tests/docx/Contoso_Guest_WiFi_Connection_Guide.docx")
    else {
        return;
    };
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .normalization(TransitionalNormalizer::new());
    let package = Package::open_path(&path, &options).expect("open Contoso");
    let source_styles = part_xml(&package, "/word/styles.xml");
    let source_theme_color = source_styles.matches("themeColor").count();
    let source_theme_fill = source_styles.matches("themeFill").count();
    assert_eq!(source_theme_color, 1277, "Contoso source themeColor count");
    assert_eq!(source_theme_fill, 784, "Contoso source themeFill count");

    let document = parse(&package).expect("parse Contoso");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let written_styles = part_xml(&open(&written.bytes).expect("reopen"), "/word/styles.xml");
    assert_eq!(
        written_styles.matches("themeColor").count(),
        source_theme_color,
        "written themeColor count must match source"
    );
    assert_eq!(
        written_styles.matches("themeFill").count(),
        source_theme_fill,
        "written themeFill count must match source"
    );
}

/// Theme underline without `w:val` still keeps `themeColor`.
#[test]
fn t_p7_underline_theme_without_val_round_trips() {
    let body = "\
<w:p><w:r><w:rPr><w:u w:color=\"C0504D\" w:themeColor=\"accent2\"/></w:rPr>\
<w:t>u</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip_body(body);
    let theme = |document: &Document| {
        let paragraph = document.body.blocks[0].as_paragraph().expect("p");
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("run");
        };
        run.props.underline_theme.clone()
    };
    assert_eq!(
        theme(&before).as_ref().map(|t| t.color.as_str()),
        Some("accent2")
    );
    assert_eq!(
        theme(&after).as_ref().map(|t| t.color.as_str()),
        Some("accent2")
    );
    assert!(xml.contains(r#"w:themeColor="accent2""#), "{xml}");
    assert!(xml.contains(r#"w:color="C0504D""#), "{xml}");
}

/// T-P8-1: hex case folds; a real value change must remain visible.
#[test]
fn t_p8_1_hex_case_and_real_value_change() {
    let body = "\
<w:p><w:r><w:rPr><w:color w:val=\"AbCdEf\"/></w:rPr><w:t>c</w:t></w:r></w:p>";
    let (before, after, xml) = round_trip_body(body);
    let color = |document: &Document| {
        let paragraph = document.body.blocks[0].as_paragraph().expect("p");
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("run");
        };
        run.props.color.clone()
    };
    assert_eq!(color(&before).as_ref().map(Color::as_str), Some("AbCdEf"));
    assert_eq!(color(&after).as_ref().map(Color::as_str), Some("AbCdEf"));
    assert!(
        xml.contains(r#"w:val="AbCdEf""#) || xml.contains(r#"w:val="abcdef""#),
        "{xml}"
    );

    let mut changed = before;
    let Block::Paragraph(paragraph) = &mut changed.body.blocks[0] else {
        panic!("p");
    };
    let Inline::Run(run) = &mut paragraph.inlines[0] else {
        panic!("run");
    };
    run.props.color = Some(Color::new("AbCdEe"));
    let package = open(
        &strict_ooxml_testkit::DocxBuilder::strict()
            .body(body)
            .build(),
    )
    .expect("open");
    let written = write_package(&changed, Some(&package), &WriteOptions::default()).expect("write");
    let xml = part_xml(&open(&written.bytes).expect("reopen"), "/word/document.xml");
    assert!(xml.contains(r#"w:val="AbCdEe""#), "{xml}");
    assert!(!xml.contains(r#"w:val="AbCdEf""#), "{xml}");
}

/// T-P8-2: `auto` and `000000` keep their lexical policy.
#[test]
fn t_p8_2_auto_and_black_keep_policy() {
    let body = "\
<w:p><w:r><w:rPr><w:color w:val=\"auto\"/></w:rPr><w:t>a</w:t></w:r></w:p>\
<w:p><w:r><w:rPr><w:color w:val=\"000000\"/></w:rPr><w:t>b</w:t></w:r></w:p>";
    let (_, _, xml) = round_trip_body(body);
    assert!(xml.contains(r#"w:val="auto""#), "{xml}");
    assert!(xml.contains(r#"w:val="000000""#), "{xml}");
}

/// T-P8-3: `a:schemeClr/@val` on shape style refs is not rewritten to `phClr`.
#[test]
fn t_p8_3_scheme_clr_on_style_refs_round_trips() {
    let body = "\
<w:p><w:r><w:drawing><wp:inline>\
<wp:extent cx=\"914400\" cy=\"914400\"/><wp:docPr id=\"1\" name=\"s\"/>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/wordprocessingml/drawingml/wordprocessingShape\">\
<wps:wsp><wps:cNvPr id=\"1\" name=\"s\"/><wps:cNvSpPr/>\
<wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/><a:solidFill><a:schemeClr val=\"accent1\"/></a:solidFill>\
</wps:spPr>\
<wps:style>\
<a:lnRef idx=\"1\"><a:schemeClr val=\"accent2\"/></a:lnRef>\
<a:fillRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:fillRef>\
<a:effectRef idx=\"0\"><a:schemeClr val=\"dk1\"/></a:effectRef>\
<a:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></a:fontRef>\
</wps:style>\
</wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";
    let (before, after, xml) = round_trip_body(body);
    let style = |document: &Document| {
        let paragraph = document.body.blocks[0].as_paragraph().expect("p");
        let Inline::Run(run) = &paragraph.inlines[0] else {
            panic!("run");
        };
        run.content.iter().find_map(|content| match content {
            strict_ooxml_wml::model::inline::RunContent::Drawing(drawing) => match &drawing.kind {
                strict_ooxml_wml::model::drawing::DrawingKind::Inline(inline) => {
                    match inline.graphic.as_ref() {
                        strict_ooxml_wml::model::drawing::Graphic::Shape(shape) => {
                            shape.style.clone()
                        }
                        other => panic!("expected shape, got {other:?}"),
                    }
                }
                other => panic!("expected inline, got {other:?}"),
            },
            _ => None,
        })
    };
    let before_style = style(&before).expect("style before");
    let after_style = style(&after).expect("style after");
    assert_eq!(before_style.line_ref_color.as_deref(), Some("accent2"));
    assert_eq!(after_style.fill_ref_color.as_deref(), Some("accent1"));
    assert_eq!(after_style.effect_ref_color.as_deref(), Some("dk1"));
    assert_eq!(after_style.font_ref_color.as_deref(), Some("tx1"));
    assert_eq!(after_style.font_ref.as_deref(), Some("minor"));
    assert!(xml.contains(r#"val="accent2""#), "{xml}");
    assert!(xml.contains(r#"val="accent1""#), "{xml}");
    assert!(xml.contains(r#"val="dk1""#), "{xml}");
    assert!(xml.contains(r#"val="tx1""#), "{xml}");
    assert!(
        !xml.contains(r#"val="phClr""#),
        "must not invent phClr over a stored scheme: {xml}"
    );
}
