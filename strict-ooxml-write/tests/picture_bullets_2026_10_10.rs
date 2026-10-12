//! Picture bullets (`w:numPicBullet` + `w:lvlPicBulletId`) round-trip.
//!
//! Census 2026-10-10 found 009_stratigrafi's picture bullet skipped silently and
//! then named as a loss (`TZ-63`/`TZ-64`). The bullet is now modelled: its
//! `w:drawing` is parsed with the body's drawing parser, the image resolves
//! against `numbering.xml`'s own relationships, and the writer gives
//! `numbering.xml` a `.rels` part with the image relationship.
//!
//! * a Strict bullet (`w:drawing`) keeps its picture, its image bytes and the
//!   level's reference, and the written package is a fixed point;
//! * a Transitional VML bullet (`w:pict/v:shape/v:imagedata`) is converted by the
//!   normalizer (T7) and written as a Strict `w:drawing`, with no VML left.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::case_sensitive_file_extension_comparisons,
    missing_docs
)]

use std::sync::Arc;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::docx::{rels_xml, Family, Rel};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Document, SupportStatus};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbullet";
const NUMBERING_PART: &str = "/word/numbering.xml";
const NUMBERING_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";

/// One abstract definition whose level 0 uses picture bullet 0, and one
/// instance of it.
const PICTURE_BULLET_LEVEL: &str = "<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/>\
<w:lvlText w:val=\"\u{f0b7}\"/><w:lvlPicBulletId w:val=\"0\"/><w:lvlJc w:val=\"start\"/>\
</w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";

/// A Strict picture bullet: `w:drawing` with `a:blip r:embed="rId1"`.
const STRICT_BULLET: &str = "<w:numPicBullet w:numPicBulletId=\"0\"><w:drawing>\
<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"138430\" cy=\"138430\"/>\
<wp:docPr id=\"1\" name=\"bullet\"/><a:graphic>\
<a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"bullet\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"138430\" cy=\"138430\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:numPicBullet>";

/// 009_stratigrafi as Word wrote it: a VML `w:pict` inside `w:numPicBullet`.
const VML_BULLET: &str = "<w:numPicBullet w:numPicBulletId=\"0\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\">\
<w:pict><v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" \
o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\">\
<v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/>\
<v:f eqn=\"sum @0 1 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" \
o:connecttype=\"rect\"/><o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>\
<v:shape id=\"_x0000_i1039\" type=\"#_x0000_t75\" style=\"width:10.9pt;height:10.9pt\" \
o:bullet=\"t\"><v:imagedata r:id=\"rId1\" o:title=\"bullet\"/></v:shape></w:pict>\
</w:numPicBullet>";

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

fn position(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("missing {needle} in {text}"))
}

/// A package whose `numbering.xml` is `numbering` and whose
/// `numbering.xml.rels` names `media/image1.png` as `rId1`.
fn numbering_package(family: Family, numbering: &str) -> Vec<u8> {
    let builder = match family {
        Family::Strict => DocxBuilder::strict(),
        Family::Transitional => DocxBuilder::transitional(),
    };
    builder
        .body(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
<w:r><w:t>item</w:t></w:r></w:p>",
        )
        .rel("rIdNumbering", "numbering", "numbering.xml")
        .content_type(NUMBERING_PART, NUMBERING_TYPE)
        .part_xml("word/numbering.xml", "w:numbering", numbering)
        .part(
            "word/_rels/numbering.xml.rels",
            rels_xml(&[Rel::new(
                "rId1",
                family.rel_type("image"),
                "media/image1.png",
            )]),
        )
        .part("word/media/image1.png", PNG.to_vec())
        .build()
}

/// The written numbering part carries the bullet as a `w:drawing` whose
/// `r:embed` resolves through `numbering.xml.rels` to the source's bytes, and
/// the level still refers to it, in `CT_Numbering` / `CT_Lvl` order.
fn assert_bullet_carried(written: &Written) -> String {
    let numbering = part_text(&written.package, NUMBERING_PART);
    assert!(
        numbering.contains("<w:numPicBullet w:numPicBulletId=\"0\"><w:drawing>"),
        "{numbering}"
    );
    assert!(
        position(&numbering, "<w:numPicBullet") < position(&numbering, "<w:abstractNum"),
        "numPicBullet comes first in CT_Numbering: {numbering}"
    );
    assert!(
        numbering.contains("<w:lvlPicBulletId w:val=\"0\"/>"),
        "{numbering}"
    );
    assert!(
        position(&numbering, "<w:lvlText") < position(&numbering, "<w:lvlPicBulletId"),
        "{numbering}"
    );
    assert!(
        position(&numbering, "<w:lvlPicBulletId") < position(&numbering, "<w:lvlJc"),
        "{numbering}"
    );

    let start = position(&numbering, "r:embed=\"") + "r:embed=\"".len();
    let rest = numbering.get(start..).expect("embed value");
    let id = rest
        .get(..rest.find('"').expect("closing quote"))
        .expect("id");
    let rel = written
        .package
        .resolve_relationship(&PartId::new(NUMBERING_PART), id)
        .unwrap_or_else(|_| panic!("{id} is declared by numbering.xml.rels"));
    assert!(rel.raw_type.ends_with("/image"), "{}", rel.raw_type);
    assert!(
        rel.raw_type.starts_with("http://purl.oclc.org/ooxml/"),
        "a Strict relationship type: {}",
        rel.raw_type
    );
    let image = rel.resolved.clone().expect("resolved image part");
    assert_eq!(
        written.package.read_part(&image).expect("image bytes"),
        PNG,
        "the bullet's image is carried byte for byte"
    );

    let bullet = written
        .document
        .support
        .get("w:numPicBullet")
        .expect("w:numPicBullet recorded");
    assert_eq!(bullet.status, SupportStatus::Supported, "{bullet:?}");
    assert!(
        !written.report.contains("w:numPicBullet") && !written.report.contains("lvlPicBulletId"),
        "{}",
        written.report
    );
    numbering
}

/// The written package's second write is byte-identical to its first.
fn assert_fixed_point(first: &Written) {
    let again = write(&first.bytes, false);
    assert_eq!(
        part_text(&again.package, NUMBERING_PART),
        part_text(&first.package, NUMBERING_PART),
        "numbering.xml is not a fixed point"
    );
    assert_eq!(
        part_text(&again.package, "/word/_rels/numbering.xml.rels"),
        part_text(&first.package, "/word/_rels/numbering.xml.rels"),
        "numbering.xml.rels is not a fixed point"
    );
    assert!(again.bytes == first.bytes, "not a fixed point");
}

/// A Strict `w:numPicBullet` with `w:drawing` survives a write and a re-write.
#[test]
fn strict_picture_bullet_round_trips_with_its_image() {
    let numbering = format!("{STRICT_BULLET}{PICTURE_BULLET_LEVEL}");
    let bytes = numbering_package(Family::Strict, &numbering);
    let written = write(&bytes, false);
    let model_bullet = written
        .document
        .numbering
        .pic_bullet(0)
        .expect("picture bullet 0 is modelled");
    assert_eq!(model_bullet.id, 0);
    let level = written
        .document
        .numbering
        .abstracts()
        .next()
        .and_then(|abstract_num| abstract_num.levels.first())
        .expect("level 0");
    assert_eq!(level.pic_bullet_id, Some(0));
    assert!(
        written
            .document
            .media
            .iter()
            .any(|item| item.part.as_str() == "/word/media/image1.png"),
        "the bullet's image is in the document's media index"
    );
    let numbering = assert_bullet_carried(&written);
    assert!(numbering.contains("name=\"bullet\""), "{numbering}");
    assert_fixed_point(&written);
}

/// 009_stratigrafi: the VML bullet is converted (T7) and written as DrawingML.
#[test]
fn transitional_vml_picture_bullet_is_written_as_a_drawing() {
    let numbering = format!("{VML_BULLET}{PICTURE_BULLET_LEVEL}");
    let bytes = numbering_package(Family::Transitional, &numbering);
    let written = write(&bytes, true);
    let numbering = assert_bullet_carried(&written);
    for vml in [
        "w:pict",
        "v:shape",
        "v:imagedata",
        "o:lock",
        "urn:schemas-microsoft-com:vml",
        "urn:schemas-microsoft-com:office:office",
    ] {
        assert!(!numbering.contains(vml), "{vml} left in {numbering}");
    }
    assert_fixed_point(&written);
}

/// A level that names a picture bullet the part does not define writes no
/// dangling `w:lvlPicBulletId`, and says so.
#[test]
fn a_reference_to_a_missing_picture_bullet_is_not_written() {
    let bytes = numbering_package(Family::Strict, PICTURE_BULLET_LEVEL);
    let written = write(&bytes, false);
    let numbering = part_text(&written.package, NUMBERING_PART);
    assert!(!numbering.contains("lvlPicBulletId"), "{numbering}");
    assert!(numbering.contains("<w:lvlText "), "{numbering}");
    assert!(
        written.report.contains("w:lvlPicBulletId"),
        "{}",
        written.report
    );
}
