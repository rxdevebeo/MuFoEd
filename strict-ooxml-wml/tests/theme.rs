#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Integration tests for the theme part (`theme1.xml`).

mod common;

use common::{document_parts, parse_parts, rels, W_NS};

const THEME: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/theme";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";

/// A minimal DrawingML Strict theme.
fn theme_xml() -> Vec<u8> {
    theme_with_format_scheme("<a:fmtScheme name=\"Office\"/>")
}

/// The same theme with `format_scheme` as its `a:fmtScheme`.
fn theme_with_format_scheme(format_scheme: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><a:theme xmlns:a=\"{A_NS}\" name=\"Office\"><a:themeElements>\
<a:clrScheme name=\"Office\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"44546A\"/></a:dk2>\
<a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>\
<a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
<a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3>\
<a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
<a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5>\
<a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink>\
<a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink>\
</a:clrScheme>\
<a:fontScheme name=\"Office\">\
<a:majorFont><a:latin typeface=\"Cambria\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont>\
</a:fontScheme>\
{format_scheme}\
</a:themeElements></a:theme>"
    )
    .into_bytes()
}

#[test]
fn parses_theme_fonts_and_colors() {
    let body = "<w:p/>";
    let rels = rels(&[("rIdTheme", THEME, "theme/theme1.xml")]);
    let parts = document_parts(
        body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/theme/theme1.xml", theme_xml()),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let theme = document.theme.as_ref().expect("theme parsed");
    assert_eq!(theme.font("minorHAnsi").map(AsRef::as_ref), Some("Calibri"));
    assert_eq!(theme.font("majorAscii").map(AsRef::as_ref), Some("Cambria"));
    assert_eq!(theme.color("accent1"), Some("#4472c4"));
    assert_eq!(theme.color("dark1"), Some("#000000"));
    assert_eq!(theme.color("light1"), Some("#ffffff"));
    assert_eq!(theme.color("hyperlink"), Some("#0563c1"));
    assert!(document.source.theme.is_some());
    assert!(document.support.get("a:theme").is_some());
    let _ = W_NS;
}

#[test]
fn parses_run_theme_font_and_color_references() {
    let body = "<w:p><w:r><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\"/><w:color w:themeColor=\"accent1\" w:themeShade=\"80\"/></w:rPr><w:t>x</w:t></w:r></w:p>";
    let parts = document_parts(body, &[]);
    let document = parse_parts(&parts).expect("parse");
    let strict_ooxml_wml::model::Block::Paragraph(paragraph) = &document.body.blocks[0] else {
        panic!("expected paragraph");
    };
    let strict_ooxml_wml::model::Inline::Run(run) = &paragraph.inlines[0] else {
        panic!("expected run");
    };
    let fonts = run.props.fonts.as_ref().expect("fonts");
    assert_eq!(fonts.ascii_theme.as_deref(), Some("minorHAnsi"));
    let color = run.props.color_theme.as_ref().expect("theme color");
    assert_eq!(color.color.as_str(), "accent1");
    assert_eq!(color.shade.as_deref(), Some("80"));
}

/// A Strict theme whose fill styles carry the SDK's bare thousandths
/// (`pos="35000"`, `val="130000"`) keeps them as the Strict percentages they
/// mean, so the written theme validates; percentages already written as such,
/// and angles that only look like thousandths, are kept as read.
#[test]
fn a_kept_format_scheme_writes_strict_percentages() {
    let scheme = "<a:fmtScheme name=\"Office\"><a:fillStyleLst>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:gradFill rotWithShape=\"1\"><a:gsLst>\
<a:gs pos=\"0\"><a:schemeClr val=\"phClr\"><a:tint val=\"50000\"/><a:satMod val=\"300000\"/></a:schemeClr></a:gs>\
<a:gs pos=\"35000\"><a:schemeClr val=\"phClr\"><a:tint val=\"37%\"/></a:schemeClr></a:gs>\
</a:gsLst><a:lin ang=\"16200000\" scaled=\"1\"/></a:gradFill>\
<a:gradFill rotWithShape=\"1\"><a:gsLst>\
<a:gs pos=\"100000\"><a:schemeClr val=\"phClr\"><a:shade val=\"30000\"/></a:schemeClr></a:gs>\
</a:gsLst><a:path path=\"circle\"><a:fillToRect l=\"50000\" t=\"-80000\" r=\"50000\" b=\"180000\"/></a:path></a:gradFill>\
</a:fillStyleLst><a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>";
    let rels = rels(&[("rIdTheme", THEME, "theme/theme1.xml")]);
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/theme/theme1.xml", theme_with_format_scheme(scheme)),
        ],
    );
    let document = parse_parts(&parts).expect("parse");
    let theme = document.theme.as_ref().expect("theme parsed");
    let kept = theme
        .format_scheme_xml
        .as_deref()
        .expect("the scheme is kept");
    for expected in [
        r#"<a:gs pos="0%">"#,
        r#"<a:gs pos="35%">"#,
        r#"<a:gs pos="100%">"#,
        r#"<a:tint val="50%"/>"#,
        r#"<a:satMod val="300%"/>"#,
        r#"<a:shade val="30%"/>"#,
        r#"<a:fillToRect l="50%" t="-80%" r="50%" b="180%"/>"#,
        r#"<a:tint val="37%"/>"#,
        r#"<a:lin ang="16200000" scaled="1"/>"#,
        r#"<a:ln w="9525">"#,
    ] {
        assert!(kept.contains(expected), "{expected} in {kept}");
    }
}
