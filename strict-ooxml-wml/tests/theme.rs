#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal,
    clippy::too_many_lines
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

/// Parses a document whose theme part is `theme`.
fn parse_with_theme(theme: &[u8]) -> strict_ooxml_wml::model::Document {
    let rels = rels(&[("rIdTheme", THEME, "theme/theme1.xml")]);
    let parts = document_parts(
        "<w:p/>",
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/theme/theme1.xml", theme.to_vec()),
        ],
    );
    parse_parts(&parts).expect("parse")
}

/// A pretty-printed theme with what the minimal one leaves out: foreign
/// elements at each level, colour slots that are not `srgbClr`/`sysClr`, a
/// face per script, faces repeated, and object defaults.
#[test]
fn a_full_theme_keeps_scripts_object_defaults_and_skips_the_rest() {
    let theme = format!(
        r#"<?xml version="1.0"?>
<a:theme xmlns:a="{A_NS}" xmlns:x="urn:x" name="Office">
  <x:unknown/>
  <a:themeElements>
    <x:other/>
    <a:clrScheme name="Office">
      <x:slot/>
      <a:dk1>
        <a:sysClr val="windowText"/>
      </a:dk1>
      <a:accent1>
        <a:prstClr val="red"/>
      </a:accent1>
      <a:accent2>
        <a:srgbClr val="ED7D31"/>
        <a:srgbClr val="000000"/>
      </a:accent2>
    </a:clrScheme>
    <a:fontScheme name="Office">
      <a:extLst/>
      <a:majorFont>
        <a:latin typeface="Cambria" panose="02040503050406030204"/>
        <a:latin typeface="Ignored" pitchFamily="18" charset="0"/>
        <a:ea typeface=""/>
        <a:cs typeface="Times New Roman"/>
        <a:font script="Jpan" typeface="MS Mincho"/>
        <a:font script="Hang"/>
        <a:extLst/>
      </a:majorFont>
      <a:minorFont>
        <a:latin typeface="Calibri"/>
      </a:minorFont>
    </a:fontScheme>
  </a:themeElements>
  <a:objectDefaults>
    <a:spDef>
      <a:spPr/>
      <a:lstStyle>
        <a:lvl1pPr>
          <a:lnSpc><a:spcPct val="90000"/></a:lnSpc>
          <a:defRPr sz="1800" xml:lang="en-US">
            <a:latin typeface="Arial"/>
            <a:ea typeface="MS Gothic"/>
            <a:cs typeface="Tahoma"/>
            <a:solidFill><a:schemeClr val="tx1"/></a:solidFill>
            <a:sym typeface="Symbol">s&amp;t</a:sym>
            loose
          </a:defRPr>
        </a:lvl1pPr>
        <a:lvl2pPr>
          <a:defRPr><a:latin typeface="Second"/></a:defRPr>
        </a:lvl2pPr>
      </a:lstStyle>
      <x:foreign x:a="1">text</x:foreign>
      <plain xmlns="urn:y"/>
    </a:spDef>
    <a:lnDef><a:spPr><a:ln><a:miter lim="800000"/></a:ln></a:spPr></a:lnDef>
    <a:txDef>
      <a:lstStyle><a:lvl1pPr><a:defRPr><a:latin typeface="Times"/></a:defRPr></a:lvl1pPr></a:lstStyle>
    </a:txDef>
  </a:objectDefaults>
</a:theme>"#
    );
    let document = parse_with_theme(theme.as_bytes());
    let theme = document.theme.as_ref().expect("theme parsed");

    // Colours: a system colour without lastClr is not a hex value; an
    // unknown colour element yields nothing; the first colour wins.
    assert_eq!(theme.colors.system("dk1"), Some("windowText"));
    assert_eq!(theme.colors.slot("dk1"), None);
    assert_eq!(theme.colors.slot("accent1"), None);
    assert_eq!(theme.colors.slot("accent2"), Some("#ed7d31"));

    // Fonts: the first face wins, later attributes fill its empty slots.
    let major = &theme.fonts.major;
    assert_eq!(major.latin.name.as_deref(), Some("Cambria"));
    assert_eq!(major.latin.panose.as_deref(), Some("02040503050406030204"));
    assert_eq!(major.latin.pitch_family.as_deref(), Some("18"));
    assert_eq!(major.latin.charset.as_deref(), Some("0"));
    assert!(major.east_asia.name.is_none());
    assert_eq!(major.cs.name.as_deref(), Some("Times New Roman"));
    assert_eq!(major.scripts.len(), 1);
    assert_eq!(&*major.scripts[0].0, "Jpan");
    assert_eq!(&*major.scripts[0].1, "MS Mincho");
    assert_eq!(theme.fonts.minor.latin.name.as_deref(), Some("Calibri"));

    // Object defaults: the first defRPr under each of spDef and txDef.
    let shape = theme.shape_defaults.as_ref().expect("shape defaults");
    assert_eq!(shape.latin.name.as_deref(), Some("Arial"));
    assert_eq!(shape.east_asia.name.as_deref(), Some("MS Gothic"));
    assert_eq!(shape.cs.name.as_deref(), Some("Tahoma"));
    let text = theme.text_defaults.as_ref().expect("text defaults");
    assert_eq!(text.latin.name.as_deref(), Some("Times"));

    let xml = theme.object_defaults_xml.as_deref().expect("copied");
    for expected in [
        "<a:objectDefaults>",
        r#"<a:spcPct val="90%">"#,
        r#"<a:defRPr sz="1800" xml:lang="en-US">"#,
        "s&amp;t</a:sym>",
        "loose",
        r#"<x:foreign x:a="1">text</x:foreign>"#,
        "<plain></plain>",
        r#"<a:miter lim="800%">"#,
        r#"<a:latin typeface="Second">"#,
        "</a:objectDefaults>",
    ] {
        assert!(xml.contains(expected), "{expected} in {xml}");
    }
    assert!(document.support.get("a:theme").is_some());
}

/// A theme fill that needs a relationship is replaced by a solid fill over
/// `phClr` and named; an Office extension outside Strict is dropped with
/// its `a:extLst`; a Strict extension, text and `xml:` attributes are kept.
#[test]
fn a_format_scheme_replaces_relationship_fills_and_drops_foreign_extensions() {
    let scheme = r#"<a:fmtScheme name="Office" xmlns:r="http://purl.oclc.org/ooxml/officeDocument/relationships" xmlns:o="urn:office"><a:fillStyleLst>
<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
<a:blipFill><a:blip r:embed="rId1"><a:alphaModFix amt="50000"/></a:blip><a:stretch/></a:blipFill>
<a:blipFill><a:blip r:link="rId2"/></a:blipFill>
</a:fillStyleLst>
<a:lnStyleLst><a:ln w="9525" xml:space="preserve">x</a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/>
<a:extLst><a:ext uri="{OFFICE}"><o:thing/></a:ext></a:extLst>
<a:extLst><a:ext uri="{STRICT}"><a:kept/></a:ext><a:ext uri="{GONE}"><a:x o:attr="1"/></a:ext></a:extLst>
</a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>"#;
    let document = parse_with_theme(&theme_with_format_scheme(scheme));
    let theme = document.theme.as_ref().expect("theme parsed");
    let kept = theme.format_scheme_xml.as_deref().expect("kept");
    assert_eq!(
        kept.matches(r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#)
            .count(),
        4,
        "{kept}"
    );
    assert!(!kept.contains("blipFill"), "{kept}");
    assert!(
        kept.contains(r#"<a:ln w="9525" xml:space="preserve">x</a:ln>"#),
        "{kept}"
    );
    assert!(
        kept.contains(r#"<a:ext uri="{STRICT}"><a:kept/></a:ext>"#),
        "{kept}"
    );
    assert!(!kept.contains("OFFICE") && !kept.contains("GONE"), "{kept}");
    assert_eq!(kept.matches("<a:extLst>").count(), 1, "{kept}");

    let scheme_use = document.support.get("a:fmtScheme").expect("recorded");
    let message = scheme_use.message.as_deref().unwrap_or_default();
    assert!(message.contains("a:blip"), "{message}");
    assert!(document.support.get("a:ext").is_some());
}

/// Anything else outside DrawingML in the scheme cannot be written under the
/// `a:` root, so the placeholder stands in and the scheme is cited.
#[test]
fn a_format_scheme_with_foreign_markup_falls_back_to_the_placeholder() {
    for scheme in [
        r#"<a:fmtScheme name="Office" xmlns:o="urn:office"><o:thing/></a:fmtScheme>"#,
        r#"<a:fmtScheme name="Office" xmlns:o="urn:office"><a:fillStyleLst o:a="1"/></a:fmtScheme>"#,
    ] {
        let document = parse_with_theme(&theme_with_format_scheme(scheme));
        let theme = document.theme.as_ref().expect("theme parsed");
        assert!(theme.format_scheme_xml.is_none(), "{scheme}");
        let scheme_use = document.support.get("a:fmtScheme").expect("recorded");
        assert!(
            scheme_use
                .message
                .as_deref()
                .is_some_and(|message| message.contains("placeholder")),
            "{scheme}"
        );
    }
}
