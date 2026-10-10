//! Census 2026-10-10 — settings, numbering and theme rows.
//!
//! Each test is one family of `signal = "element"` census rows:
//!
//! * `w:mirrorMargins` / `w:autoHyphenation` written as `w:val="0"` were folded
//!   into "absent" and left `settings.xml` without a word in the report;
//! * `w:activeWritingStyle`, `w:stylePaneSortMethod`, `w:smartTagType` and the
//!   two `w:drawingGrid*Origin` children are `CT_Settings` members Strict
//!   declares, and were dropped as foreign;
//! * `w:numPicBullet` / `w:lvlPicBulletId` were skipped silently (they are now
//!   carried; see `picture_bullets_2026_10_10.rs`);
//! * one `a:blipFill` with `r:embed` in a theme's background fills threw the
//!   whole `a:fmtScheme` away for the placeholder.
#![allow(clippy::doc_markdown, clippy::format_push_string)]

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::document::Document;
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

const SETTINGS_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const THEME_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// Opens `bytes` through the Transitional normalizer, as `write --transitional` does.
fn open_transitional(bytes: &[u8]) -> Package {
    Package::open_reader(
        bytes,
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .normalization(TransitionalNormalizer::new()),
    )
    .expect("open")
}

fn open_strict(bytes: &[u8]) -> Package {
    Package::open_reader(bytes, &OpenOptions::default()).expect("open")
}

fn part_text(bytes: &[u8], name: &str) -> String {
    let package = open_strict(bytes);
    let raw = package.read_part(&PartId::new(name)).expect(name);
    String::from_utf8(raw).expect("utf-8")
}

/// Parses and writes `package`; returns the document and the written bytes.
fn round_trip(package: &Package) -> (Document, Vec<u8>) {
    let document = parse_document(package, &ParseOptions::default()).expect("parse");
    let written = write_package(&document, Some(package), &WriteOptions::default()).expect("write");
    (document, written.bytes)
}

/// Writes the written package once more and returns the named part, so a test
/// can check that the kept markup is a fixed point of read + write.
fn rewritten_part(written: &[u8], name: &str) -> String {
    let reopened = open_strict(written);
    let (_, again) = round_trip(&reopened);
    part_text(&again, name)
}

fn transitional_settings(inner: &str) -> Vec<u8> {
    DocxBuilder::transitional()
        .body("<w:p><w:r><w:t>settings</w:t></w:r></w:p>")
        .rel("rIdSettings", "settings", "settings.xml")
        .content_type("/word/settings.xml", SETTINGS_TYPE)
        .part_xml("word/settings.xml", "w:settings", inner)
        .build()
}

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("missing {needle} in {text}"))
}

/// DOCX_Mathematical_Equations: `w:val="0"` is an explicit off, not "absent".
#[test]
fn census_settings_explicit_off_flags_are_kept() {
    let bytes = transitional_settings(
        "<w:mirrorMargins w:val=\"0\"/><w:defaultTabStop w:val=\"720\"/>\
<w:autoHyphenation w:val=\"0\"/>",
    );
    let (document, written) = round_trip(&open_transitional(&bytes));
    assert!(document.settings.mirror_margins_off);
    assert!(!document.settings.mirror_margins);
    assert!(document.settings.auto_hyphenation_off);
    assert!(!document.settings.auto_hyphenation);
    let settings = part_text(&written, "/word/settings.xml");
    assert!(
        settings.contains(r#"<w:mirrorMargins w:val="false"/>"#),
        "{settings}"
    );
    assert!(
        settings.contains(r#"<w:autoHyphenation w:val="false"/>"#),
        "{settings}"
    );
    assert_eq!(
        rewritten_part(&written, "/word/settings.xml"),
        settings,
        "not a fixed point"
    );

    // A bare element is still on, and still written bare.
    let bytes = transitional_settings("<w:mirrorMargins/>");
    let (_, written) = round_trip(&open_transitional(&bytes));
    let settings = part_text(&written, "/word/settings.xml");
    assert!(settings.contains("<w:mirrorMargins/>"), "{settings}");
}

/// 035_Scarlat, 044_Yekuana, 047_uz25a and 070_Innovations: `CT_Settings`
/// children Strict declares are carried, in `xsd:sequence` order.
#[test]
fn census_settings_declared_children_are_kept_in_schema_order() {
    // Input order is deliberately not the schema's.
    let bytes = transitional_settings(
        "<w:smartTagType w:namespaceuri=\"urn:schemas-microsoft-com:office:smarttags\" w:name=\"date\"/>\
<w:smartTagType w:namespaceuri=\"urn:schemas-microsoft-com:office:smarttags\" w:name=\"place\"/>\
<w:drawingGridVerticalOrigin w:val=\"0\"/>\
<w:drawingGridHorizontalOrigin w:val=\"0\"/>\
<w:stylePaneSortMethod w:val=\"0001\"/>\
<w:activeWritingStyle w:appName=\"MSWord\" w:lang=\"en-US\" w:vendorID=\"64\" \
w:dllVersion=\"6\" w:nlCheck=\"1\" w:checkStyle=\"0\"/>\
<w:activeWritingStyle w:appName=\"MSWord\" w:lang=\"en-US\" w:vendorID=\"64\" \
w:dllVersion=\"0\" w:nlCheck=\"1\" w:checkStyle=\"0\"/>\
<w:mirrorMargins/>",
    );
    let (document, written) = round_trip(&open_transitional(&bytes));
    assert_eq!(document.settings.active_writing_styles.len(), 2);
    assert_eq!(document.settings.smart_tag_types.len(), 2);
    for feature in [
        "w:activeWritingStyle",
        "w:smartTagType",
        "w:stylePaneSortMethod",
        "w:drawingGridHorizontalOrigin",
        "w:drawingGridVerticalOrigin",
    ] {
        assert!(
            document
                .support
                .get(feature)
                .is_none_or(|used| used.status == SupportStatus::Supported),
            "{feature} must no longer be recorded as foreign"
        );
    }
    let settings = part_text(&written, "/word/settings.xml");
    assert_eq!(
        settings.matches("<w:activeWritingStyle ").count(),
        2,
        "{settings}"
    );
    assert!(settings.contains(r#"w:dllVersion="6""#), "{settings}");
    assert!(settings.contains(r#"w:checkStyle="0""#), "{settings}");
    assert!(
        settings.contains(r#"<w:stylePaneSortMethod w:val="priority"/>"#),
        "Strict ST_StylePaneSortMethods has the name, not the code: {settings}"
    );
    assert!(
        settings.contains(r#"<w:drawingGridHorizontalOrigin w:val="0"/>"#),
        "{settings}"
    );
    assert!(
        settings.contains(r#"<w:drawingGridVerticalOrigin w:val="0"/>"#),
        "{settings}"
    );
    assert_eq!(
        settings.matches("<w:smartTagType ").count(),
        2,
        "{settings}"
    );
    assert!(settings.contains(r#"w:name="place""#), "{settings}");
    let order = [
        "<w:mirrorMargins",
        "<w:activeWritingStyle",
        "<w:stylePaneSortMethod",
        "<w:drawingGridHorizontalOrigin",
        "<w:drawingGridVerticalOrigin",
        "<w:smartTagType",
    ];
    for pair in order.windows(2) {
        if let [before, after] = pair {
            assert!(
                position(&settings, before) < position(&settings, after),
                "{before} must precede {after}: {settings}"
            );
        }
    }
    assert_eq!(
        rewritten_part(&written, "/word/settings.xml"),
        settings,
        "not a fixed point"
    );
}

/// The theme of 090_Gritos, cut down: the background fills end with a texture
/// (`a:blipFill` + `r:embed`), and the third effect style has a bevel with a
/// contour colour.
const GRITOS_FORMAT_SCHEME: &str = "<a:fmtScheme name=\"Urban\">\
<a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\"><a:schemeClr val=\"phClr\">\
<a:tint val=\"1000\"/></a:schemeClr></a:gs><a:gs pos=\"100000\"><a:schemeClr val=\"phClr\">\
<a:tint val=\"45000\"/></a:schemeClr></a:gs></a:gsLst><a:lin ang=\"5400000\" scaled=\"0\"/>\
</a:gradFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
<a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
<a:ln w=\"19050\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>\
<a:ln w=\"31750\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle>\
<a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/>\
<a:sp3d contourW=\"12700\" prstMaterial=\"dkEdge\"><a:bevelT w=\"25400\" h=\"38100\" prst=\"convex\"/>\
<a:contourClr><a:schemeClr val=\"phClr\"><a:satMod val=\"115000\"/></a:schemeClr></a:contourClr>\
</a:sp3d></a:effectStyle></a:effectStyleLst>\
<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>\
<a:blipFill><a:blip r:embed=\"rId1\"><a:duotone><a:schemeClr val=\"phClr\"><a:shade val=\"48000\"/>\
</a:schemeClr><a:schemeClr val=\"phClr\"><a:tint val=\"96000\"/></a:schemeClr></a:duotone></a:blip>\
<a:tile tx=\"0\" ty=\"0\" sx=\"80000\" sy=\"80000\" flip=\"none\" algn=\"tl\"/></a:blipFill>\
</a:bgFillStyleLst></a:fmtScheme>";

fn theme_package(format_scheme: &str) -> Vec<u8> {
    let mut colors = String::new();
    for slot in [
        "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ] {
        colors.push_str(&format!("<a:{slot}><a:srgbClr val=\"000000\"/></a:{slot}>"));
    }
    let fonts = "<a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/>";
    let theme = format!(
        "<a:themeElements><a:clrScheme name=\"c\">{colors}</a:clrScheme>\
<a:fontScheme name=\"f\"><a:majorFont>{fonts}</a:majorFont><a:minorFont>{fonts}</a:minorFont>\
</a:fontScheme>{format_scheme}</a:themeElements>"
    );
    DocxBuilder::transitional()
        .body("<w:p><w:r><w:t>theme</w:t></w:r></w:p>")
        .part_xml("word/theme/theme1.xml", "a:theme", &theme)
        .rel("rIdTheme", "theme", "theme/theme1.xml")
        .content_type("/word/theme/theme1.xml", THEME_TYPE)
        .build()
}

/// 090_Gritos: only the texture fill is replaced; the rest of the scheme stays.
#[test]
fn census_theme_texture_fill_does_not_cost_the_whole_format_scheme() {
    let bytes = theme_package(GRITOS_FORMAT_SCHEME);
    let (document, written) = round_trip(&open_transitional(&bytes));
    let scheme = document
        .support
        .get("a:fmtScheme")
        .expect("the replaced texture is named under a:fmtScheme");
    assert_eq!(scheme.status, SupportStatus::Partial);
    let message = scheme.message.as_deref().unwrap_or_default();
    for token in ["a:blipFill", "a:blip", "a:duotone", "a:tile"] {
        assert!(message.contains(token), "{token} missing from {message}");
    }
    assert!(
        message.contains("replaced"),
        "the kept-scheme message, not the placeholder one: {message}"
    );
    let theme = part_text(&written, "/word/theme/theme1.xml");
    // Kept as read: the scheme's name, the gradient, the bevel and its colour.
    assert!(theme.contains(r#"<a:fmtScheme name="Urban">"#), "{theme}");
    assert!(theme.contains("<a:gradFill"), "{theme}");
    assert!(theme.contains(r#"prstMaterial="dkEdge""#), "{theme}");
    assert!(theme.contains("<a:contourClr>"), "{theme}");
    assert!(theme.contains("<a:bevelT "), "{theme}");
    // Strict percentages, whichever stage converted them.
    assert!(!theme.contains(r#"<a:tint val="45000"/>"#), "{theme}");
    // The texture cannot be kept: the theme part has no relationships.
    for gone in ["blipFill", "a:blip", "duotone", "a:tile", "r:embed"] {
        assert!(!theme.contains(gone), "{gone} must not be written: {theme}");
    }
    // `EG_FillProperties` keeps its three entries.
    let background = theme
        .split("<a:bgFillStyleLst>")
        .nth(1)
        .and_then(|rest| rest.split("</a:bgFillStyleLst>").next())
        .expect("bgFillStyleLst");
    assert_eq!(
        background.matches("<a:solidFill>").count(),
        3,
        "{background}"
    );
    assert_eq!(
        rewritten_part(&written, "/word/theme/theme1.xml"),
        theme,
        "not a fixed point"
    );
}

/// A scheme without a texture is kept as read and records nothing.
#[test]
fn census_theme_plain_format_scheme_is_kept_without_a_record() {
    let plain = GRITOS_FORMAT_SCHEME.replace(
        "<a:blipFill><a:blip r:embed=\"rId1\"><a:duotone><a:schemeClr val=\"phClr\"><a:shade val=\"48000\"/>\
</a:schemeClr><a:schemeClr val=\"phClr\"><a:tint val=\"96000\"/></a:schemeClr></a:duotone></a:blip>\
<a:tile tx=\"0\" ty=\"0\" sx=\"80000\" sy=\"80000\" flip=\"none\" algn=\"tl\"/></a:blipFill>",
        "<a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill>",
    );
    assert_ne!(plain, GRITOS_FORMAT_SCHEME, "the replacement must apply");
    let bytes = theme_package(&plain);
    let (document, written) = round_trip(&open_transitional(&bytes));
    assert!(
        document.support.get("a:fmtScheme").is_none(),
        "{:?}",
        document.support.get("a:fmtScheme")
    );
    let theme = part_text(&written, "/word/theme/theme1.xml");
    assert!(theme.contains(r#"<a:fmtScheme name="Urban">"#), "{theme}");
    assert!(theme.contains(r#"<a:srgbClr val="FF0000"/>"#), "{theme}");
    assert!(theme.contains("<a:contourClr>"), "{theme}");
}

/// 001_wajiz, 090_Gritos, ...: `w:shapeDefaults` holds VML only, which Strict
/// does not carry. The parser skips it; the write names it and the VML children
/// that went with it, so the loss is in this document's own report.
#[test]
fn census_settings_shape_defaults_are_named_with_their_vml_children() {
    let bytes = transitional_settings(
        "<w:shapeDefaults>\
<o:shapedefaults xmlns:o=\"urn:schemas-microsoft-com:office:office\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" v:ext=\"edit\" spidmax=\"2049\"><v:fill color=\"white\"/></o:shapedefaults>\
<o:shapelayout xmlns:o=\"urn:schemas-microsoft-com:office:office\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" v:ext=\"edit\"><o:rules v:ext=\"edit\">\
<o:r id=\"V:Rule1\" type=\"connector\" idref=\"#_x0000_s1\"/></o:rules></o:shapelayout>\
</w:shapeDefaults>",
    );
    let normalizer = std::sync::Arc::new(TransitionalNormalizer::new());
    let package = Package::open_reader(
        bytes.as_slice(),
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(normalizer.clone()),
    )
    .expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    // What `strict-ooxml write --transitional` prints, and the census reads:
    // whichever of the two dropped it, the loss is named with its VML.
    let report = format!("{}\n{}", written.report, normalizer.report());
    // The normalizer names the element by its namespace URI and local name
    // (`.../main#shapeDefaults`); the VML children are named in the reason.
    for name in ["shapeDefaults", "v:fill", "o:shapelayout", "o:rules", "o:r"] {
        assert!(report.contains(name), "{name} in {report}");
    }
}
