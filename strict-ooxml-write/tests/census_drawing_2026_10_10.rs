//! Census rows of 2026-10-10 in the drawing writer.
//!
//! Each test is one row the census gate listed as a silent change between an
//! input part and the part this writer produced:
//!
//! - `a:gs@pos was=100000`: a normalized stop position (`100%`) was read as an
//!   integer, failed, and was written as `0%`.
//! - `cNvPr` under `wpg:grpSp`: a nested group's non-visual properties were read
//!   and never written.
//! - `a:blip@link`: a linked picture lost `r:link` and its external relationship.
//! - `a:ext` under `a:extLst` in a footer: an Office extension inside a skipped
//!   `wp:docPr` was dropped without the citation ADR-0014 requires.
//! - `c:chart@id` / `c:userShapes@id`: a carried chart is rewritten to Strict by
//!   the reader; the write now says so (`c:chartSpace`, `c:userShapes`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::sync::Arc;

use strict_ooxml_core::opc::rels::TargetMode;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::docx::{rels_xml, Family, Rel};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Document, SupportStatus};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage";
const LINK_TARGET: &str = "http://example.com/linked.png";
const A16_NS: &str = "http://schemas.microsoft.com/office/drawing/2014/main";

/// What one write produced.
struct Written {
    document: Document,
    package: Package,
    bytes: Vec<u8>,
    report: String,
}

fn open(bytes: &[u8], normalize: bool) -> Package {
    let options = if normalize {
        OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            ))
    } else {
        OpenOptions::default()
    };
    Package::open_reader(bytes, &options).expect("open")
}

fn write(bytes: &[u8], normalize: bool) -> Written {
    let source = open(bytes, normalize);
    let document = parse_document(&source, &ParseOptions::default()).expect("parse");
    let written = write_package(&document, Some(&source), &WriteOptions::default()).expect("write");
    let package = open(&written.bytes, false);
    Written {
        document,
        package,
        bytes: written.bytes,
        report: written.report.to_string(),
    }
}

fn part_text(package: &Package, part: &str) -> String {
    String::from_utf8_lossy(&package.read_part(&PartId::new(part)).expect(part)).into_owned()
}

/// The written package's second write is byte-identical to its first.
fn assert_fixed_point(first: &Written) {
    let again = write(&first.bytes, false);
    assert!(again.bytes == first.bytes, "not a fixed point");
}

/// An inline `wps:wsp` holding `fill` in its shape properties.
fn shape_with_fill(shape_uri: &str, fill: &str) -> String {
    format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/>\
<wp:docPr id=\"1\" name=\"s\"/><a:graphic><a:graphicData uri=\"{shape_uri}\">\
<wps:wsp><wps:cNvPr id=\"3\" name=\"s\"/><wps:cNvSpPr/><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
<a:ext cx=\"100\" cy=\"100\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{fill}\
</wps:spPr><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

#[test]
fn a_normalized_gradient_keeps_its_stop_positions() {
    let fill = "<a:gradFill rotWithShape=\"0\"><a:gsLst>\
<a:gs pos=\"0\"><a:srgbClr val=\"FFFFFF\"/></a:gs>\
<a:gs pos=\"35500\"><a:srgbClr val=\"000000\"/></a:gs>\
<a:gs pos=\"100000\"><a:schemeClr val=\"dk1\"><a:lumMod val=\"40000\"/></a:schemeClr></a:gs>\
</a:gsLst><a:lin ang=\"5400000\" scaled=\"1\"/></a:gradFill>";
    let body = shape_with_fill(
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        fill,
    );
    let bytes = DocxBuilder::transitional().body(&body).build();
    let written = write(&bytes, true);
    let xml = part_text(&written.package, "/word/document.xml");
    assert!(xml.contains(r#"<a:gs pos="0%">"#), "{xml}");
    assert!(xml.contains(r#"<a:gs pos="35.5%">"#), "{xml}");
    assert!(xml.contains(r#"<a:gs pos="100%">"#), "{xml}");
    assert!(!xml.contains("100.000%"), "two decimals at most: {xml}");
    assert_fixed_point(&written);
}

#[test]
fn a_strict_gradient_stop_percentage_is_read_as_written() {
    let fill = "<a:gradFill><a:gsLst>\
<a:gs pos=\"0%\"><a:srgbClr val=\"FFFFFF\"/></a:gs>\
<a:gs pos=\"100%\"><a:srgbClr val=\"000000\"/></a:gs>\
</a:gsLst></a:gradFill>";
    let body = shape_with_fill(
        "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        fill,
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let written = write(&bytes, false);
    let xml = part_text(&written.package, "/word/document.xml");
    assert!(xml.contains(r#"<a:gs pos="100%">"#), "{xml}");
    assert_eq!(xml.matches(r#"<a:gs pos="0%">"#).count(), 1, "{xml}");
    assert_fixed_point(&written);
}

#[test]
fn a_nested_group_keeps_its_non_visual_properties() {
    let xfrm = "<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/>\
<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"100\" cy=\"100\"/></a:xfrm>";
    let shape = "<wps:wsp><wps:cNvPr id=\"9\" name=\"s\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"10\" cy=\"10\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></wps:spPr><wps:bodyPr/></wps:wsp>";
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/>\
<wp:docPr id=\"1\" name=\"g\"/><a:graphic>\
<a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\">\
<wpg:wgp><wpg:cNvGrpSpPr/><wpg:grpSpPr>{xfrm}</wpg:grpSpPr>\
<wpg:grpSp><wpg:cNvPr id=\"7\" name=\"Group 7\" descr=\"inner\"/><wpg:cNvGrpSpPr/>\
<wpg:grpSpPr>{xfrm}</wpg:grpSpPr>{shape}</wpg:grpSp>{shape}</wpg:wgp>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let bytes = DocxBuilder::strict().body(&body).build();
    let written = write(&bytes, false);
    let xml = part_text(&written.package, "/word/document.xml");
    assert!(
        xml.contains(
            r#"<wpg:grpSp><wpg:cNvPr id="7" name="Group 7" descr="inner"/><wpg:cNvGrpSpPr"#
        ),
        "{xml}"
    );
    // The top-level group had no `wpg:cNvPr`, and none is invented for it.
    assert_eq!(xml.matches("<wpg:cNvPr").count(), 1, "{xml}");
    assert!(
        !written.report.contains("wpg:cNvPr@id"),
        "{}",
        written.report
    );
    assert_fixed_point(&written);
}

/// A picture whose blip embeds `rId5` and links `rId6`, inside an anchor whose
/// `wp:docPr` carries an Office 2014 `a16:creationId` extension.
fn linked_picture() -> String {
    format!(
        "<w:p><w:r><w:drawing>\
<wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"1\" \
behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"column\"><wp:posOffset>0</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV>\
<wp:extent cx=\"100\" cy=\"100\"/><wp:wrapNone/>\
<wp:docPr id=\"1\" name=\"p\"><a:extLst><a:ext uri=\"{{FF2B5EF4-FFF2-40B4-BE49-F238E27FC236}}\">\
<a16:creationId xmlns:a16=\"{A16_NS}\" id=\"{{00000000-0000-0000-0000-000000000001}}\"/>\
</a:ext></a:extLst></wp:docPr>\
<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"2\" name=\"p\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rId5\" r:link=\"rId6\" cstate=\"print\"/>\
<a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"
    )
}

fn image_rels() -> Vec<Rel> {
    vec![
        Rel::new("rId5", Family::Strict.rel_type("image"), "media/image1.png"),
        Rel {
            id: "rId6".to_owned(),
            type_uri: Family::Strict.rel_type("image"),
            target: LINK_TARGET.to_owned(),
            external: true,
        },
    ]
}

/// The `r:link` value of the one linked blip in `xml`, and the relationship it
/// names in `part`'s `.rels`, which must be the external link.
fn assert_link_kept(package: &Package, part: &str) {
    let xml = part_text(package, part);
    let at = xml
        .find("r:link=\"")
        .unwrap_or_else(|| panic!("no r:link in {part}: {xml}"));
    let rest = &xml[at + "r:link=\"".len()..];
    let id = &rest[..rest.find('"').expect("closing quote")];
    let rels = package.relationships(&PartId::new(part));
    let rel = rels
        .iter()
        .find(|rel| rel.id == id)
        .unwrap_or_else(|| panic!("{id} is not declared by {part}"));
    assert_eq!(rel.target, LINK_TARGET);
    assert_eq!(rel.target_mode, TargetMode::External);
    assert!(rel.raw_type.ends_with("/image"), "{}", rel.raw_type);
    assert!(xml.contains("r:embed=\""), "the embedded copy stays: {xml}");
}

#[test]
fn a_linked_picture_keeps_its_external_relationship() {
    let mut builder = DocxBuilder::strict()
        .body(&linked_picture())
        .content_type("/word/media/image1.png", "image/png")
        .part("word/media/image1.png", PNG.to_vec());
    for rel in image_rels() {
        builder = builder.rel_uri(rel);
    }
    let written = write(&builder.build(), false);
    assert_link_kept(&written.package, "/word/document.xml");
    assert!(!written.report.contains("r:link"), "{}", written.report);
    assert_fixed_point(&written);
}

#[test]
fn a_footer_keeps_a_linked_picture_and_cites_the_docpr_extension() {
    let footer = strict_ooxml_testkit::docx::part_xml(Family::Strict, "w:ftr", &linked_picture());
    let builder = DocxBuilder::strict()
        .body("<w:p/><w:sectPr><w:footerReference w:type=\"default\" r:id=\"rId3\"/></w:sectPr>")
        .rel("rId3", "footer", "footer1.xml")
        .content_type(
            "/word/footer1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
        )
        .content_type("/word/media/image1.png", "image/png")
        .part("word/footer1.xml", footer)
        .part("word/_rels/footer1.xml.rels", rels_xml(&image_rels()))
        .part("word/media/image1.png", PNG.to_vec());
    let written = write(&builder.build(), false);
    let footer = written
        .package
        .parts()
        .map(|part| part.id.as_str().to_owned())
        .find(|name| name.starts_with("/word/footer") && name.ends_with(".xml"))
        .expect("a footer is written");
    assert_link_kept(&written.package, &footer);
    let xml = part_text(&written.package, &footer);
    // ADR-0014: the extension is not written, and the reader names it.
    assert!(!xml.contains("creationId"), "{xml}");
    assert_eq!(
        written
            .document
            .support
            .get("a:ext")
            .map(|use_| use_.status),
        Some(SupportStatus::Unsupported)
    );
    assert_fixed_point(&written);
}

#[test]
fn a_docpr_extension_in_the_body_is_cited() {
    let mut builder = DocxBuilder::strict()
        .body(&linked_picture())
        .content_type("/word/media/image1.png", "image/png")
        .part("word/media/image1.png", PNG.to_vec());
    for rel in image_rels() {
        builder = builder.rel_uri(rel);
    }
    let written = write(&builder.build(), false);
    let xml = part_text(&written.package, "/word/document.xml");
    assert!(!xml.contains("creationId"), "{xml}");
    assert!(xml.contains(r#"<wp:docPr id="1" name="p"/>"#), "{xml}");
    assert_eq!(
        written
            .document
            .support
            .get("a:ext")
            .map(|use_| use_.status),
        Some(SupportStatus::Unsupported)
    );
}

const TRANSITIONAL_CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const TRANSITIONAL_R_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const TRANSITIONAL_REL_BASE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

#[test]
fn a_carried_chart_rewritten_to_strict_is_cited_and_keeps_its_user_shapes() {
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/>\
<wp:docPr id=\"1\" name=\"c\"/><a:graphic><a:graphicData uri=\"{TRANSITIONAL_CHART_NS}\">\
<c:chart xmlns:c=\"{TRANSITIONAL_CHART_NS}\" r:id=\"rId7\"/></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>"
    );
    let chart = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<c:chartSpace xmlns:c=\"{TRANSITIONAL_CHART_NS}\" xmlns:r=\"{TRANSITIONAL_R_NS}\">\
<c:chart><c:plotArea><c:layout/></c:plotArea></c:chart><c:userShapes r:id=\"rId2\"/>\
</c:chartSpace>"
    );
    let shapes = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<c:userShapes xmlns:c=\"{TRANSITIONAL_CHART_NS}\">\
<cdr:relSizeAnchor xmlns:cdr=\"http://schemas.openxmlformats.org/drawingml/2006/chartDrawing\">\
<cdr:from><cdr:x>0.1</cdr:x><cdr:y>0.1</cdr:y></cdr:from>\
<cdr:to><cdr:x>0.2</cdr:x><cdr:y>0.2</cdr:y></cdr:to></cdr:relSizeAnchor></c:userShapes>"
    );
    let builder = DocxBuilder::transitional()
        .body(&body)
        .rel("rId7", "chart", "charts/chart1.xml")
        .content_type(
            "/word/charts/chart1.xml",
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
        )
        .content_type(
            "/word/drawings/drawing1.xml",
            "application/vnd.openxmlformats-officedocument.drawingml.chartshapes+xml",
        )
        .part("word/charts/chart1.xml", chart.into_bytes())
        .part(
            "word/charts/_rels/chart1.xml.rels",
            rels_xml(&[Rel::new(
                "rId2",
                format!("{TRANSITIONAL_REL_BASE}chartUserShapes"),
                "../drawings/drawing1.xml",
            )]),
        )
        .part("word/drawings/drawing1.xml", shapes.into_bytes());
    let written = write(&builder.build(), true);
    assert!(
        written.report.contains("c:chartSpace"),
        "the rewritten chart is cited:\n{}",
        written.report
    );
    assert!(
        written.report.contains("c:userShapes"),
        "the rewritten chart shapes are cited:\n{}",
        written.report
    );
    let chart = part_text(&written.package, "/word/charts/chart1.xml");
    assert!(chart.contains(r#"<c:userShapes r:id="rId2"/>"#), "{chart}");
    let rels = written
        .package
        .relationships(&PartId::new("/word/charts/chart1.xml"));
    let user_shapes = rels
        .iter()
        .find(|rel| rel.id == "rId2")
        .expect("the user-shapes relationship is carried");
    assert!(
        user_shapes.raw_type.ends_with("/chartUserShapes"),
        "{}",
        user_shapes.raw_type
    );
    assert!(written
        .package
        .read_part(&PartId::new("/word/drawings/drawing1.xml"))
        .is_ok());
    let document = part_text(&written.package, "/word/document.xml");
    assert!(document.contains("<c:chart r:id=\""), "{document}");
}

#[test]
fn a_chart_from_a_strict_source_is_not_cited() {
    let strict_chart = "http://purl.oclc.org/ooxml/drawingml/chart";
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"100\" cy=\"100\"/>\
<wp:docPr id=\"1\" name=\"c\"/><a:graphic><a:graphicData uri=\"{strict_chart}\">\
<c:chart xmlns:c=\"{strict_chart}\" r:id=\"rId7\"/></a:graphicData></a:graphic>\
</wp:inline></w:drawing></w:r></w:p>"
    );
    let chart = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<c:chartSpace xmlns:c=\"{strict_chart}\"><c:chart><c:plotArea><c:layout/></c:plotArea>\
</c:chart></c:chartSpace>"
    );
    let builder = DocxBuilder::strict()
        .body(&body)
        .rel("rId7", "chart", "charts/chart1.xml")
        .content_type(
            "/word/charts/chart1.xml",
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
        )
        .part("word/charts/chart1.xml", chart.clone().into_bytes());
    let written = write(&builder.build(), false);
    assert!(
        !written.report.contains("c:chartSpace"),
        "{}",
        written.report
    );
    assert_eq!(
        part_text(&written.package, "/word/charts/chart1.xml"),
        chart
    );
}
