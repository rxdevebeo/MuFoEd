#![allow(clippy::doc_markdown)]
//! `numbering.xml` in full: every child of `w:abstractNum`, `w:lvl`, `w:num`
//! and `w:lvlOverride`, picture bullets, duplicates, the elements skipped, the
//! table's editing operations and broken `numStyleLink` chains.

mod common;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::ids::{AbstractNumId, Ilvl, NumId, StyleId};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, Num, NumberingTable};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::Document;

use common::{document_parts, parse_parts, rels, A_NS, PIC_NS, R_NS, WP_NS, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";
const NUMBERING: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";
const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

/// A Strict picture bullet whose `w:drawing` is a picture.
const BULLET_DRAWING: &str = "<w:drawing>\
<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"138430\" cy=\"138430\"/>\
<wp:docPr id=\"1\" name=\"bullet\"/><a:graphic>\
<a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"bullet\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"138430\" cy=\"138430\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing>";

/// Parses a document with `numbering` (the children of `w:numbering`) and,
/// when given, `styles` (the children of `w:styles`).
fn parse_numbering(numbering: &str, styles: Option<&str>) -> Document {
    let numbering = format!(
        "<?xml version=\"1.0\"?>\n<w:numbering xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" \
xmlns:wp=\"{WP_NS}\" xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\" xmlns:w15=\"{W15_NS}\" \
xmlns:x=\"urn:x\">\n{numbering}\n</w:numbering>"
    );
    let mut entries = vec![("rIdNumbering", NUMBERING, "numbering.xml")];
    let mut extra = vec![("word/numbering.xml", numbering.into_bytes())];
    if let Some(styles) = styles {
        entries.push(("rIdStyles", STYLES, "styles.xml"));
        extra.push((
            "word/styles.xml",
            format!("<w:styles xmlns:w=\"{W_NS}\">{styles}</w:styles>").into_bytes(),
        ));
    }
    extra.push(("word/_rels/document.xml.rels", rels(&entries)));
    parse_parts(&document_parts("<w:p/>", &extra)).expect("parse")
}

fn message(document: &Document, feature: &str) -> String {
    document
        .support
        .get(feature)
        .and_then(|feature| feature.message.clone())
        .unwrap_or_default()
}

#[test]
fn every_level_child_is_read() {
    let document = parse_numbering(
        "<x:foreign/>\n<w:unknown/>\n\
<w:abstractNum w:abstractNumId=\"3\">\n\
  <x:foreign/>\n\
  <w:nsid w:val=\"1234ABCD\"/>\n\
  <w:name w:val=\"Outline\"/>\n\
  <w:multiLevelType w:val=\"multilevel\"/>\n\
  <w:styleLink w:val=\"OutlineList\"/>\n\
  <w:lvl w:ilvl=\"0\" w:tplc=\"04090001\" w15:tentative=\"0\">\n\
    <x:foreign/>\n\
    <w:start w:val=\"4\"/>\n\
    <w:numFmt w:val=\"upperRoman\"/>\n\
    <w:lvlRestart w:val=\"0\"/>\n\
    <w:pStyle w:val=\"Heading1\"/>\n\
    <w:isLgl/>\n\
    <w:suff w:val=\"space\"/>\n\
    <w:lvlText w:val=\"%1.\"/>\n\
    <w:lvlPicBulletId w:val=\"7\"/>\n\
    <w:lvlJc w:val=\"end\"/>\n\
    <w:legacy w:legacy=\"1\"/>\n\
    <w:pPr><w:ind w:start=\"720\" w:hanging=\"360\"/></w:pPr>\n\
    <w:rPr><w:b/></w:rPr>\n\
  </w:lvl>\n\
  <w:lvl w:ilvl=\"1\" w:tentative=\"1\">\n\
    <w:tentative/>\n\
    <w:lvlPicBulletId w:val=\"x\"/>\n\
  </w:lvl>\n\
</w:abstractNum>\n\
<w:abstractNum w:abstractNumId=\"3\"><w:name w:val=\"Second\"/></w:abstractNum>\n\
<w:abstractNum><w:name w:val=\"No id\"/></w:abstractNum>\n\
<w:num w:numId=\"5\" w15:durableId=\"99\">\n\
  <x:foreign/>\n\
  <w:unknown/>\n\
  <w:abstractNumId w:val=\"3\"/>\n\
  <w:lvlOverride w:ilvl=\"1\">\n\
    <x:foreign/>\n\
    <w:unknown/>\n\
    <w:startOverride w:val=\"9\"/>\n\
    <w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"bullet\"/></w:lvl>\n\
  </w:lvlOverride>\n\
</w:num>\n\
<w:num w:numId=\"5\"><w:abstractNumId w:val=\"3\"/></w:num>\n\
<w:num><w:abstractNumId w:val=\"3\"/></w:num>\n\
<w:num w:numId=\"6\"/>",
        None,
    );
    let numbering = &document.numbering;
    let abstract_num = numbering
        .abstract_num(AbstractNumId(3))
        .expect("first definition");
    assert_eq!(abstract_num.name.as_deref(), Some("Outline"));
    assert_eq!(abstract_num.multi_level_type.as_deref(), Some("multilevel"));
    assert_eq!(
        abstract_num.style_link.as_ref().map(StyleId::as_str),
        Some("OutlineList")
    );
    assert_eq!(numbering.abstracts().count(), 1);

    let first = abstract_num.level(Ilvl(0)).expect("level 0");
    assert_eq!(first.start, Some(4));
    assert_eq!(first.format.as_deref(), Some("upperRoman"));
    assert_eq!(first.restart, Some(0));
    assert_eq!(
        first.paragraph_style.as_ref().map(StyleId::as_str),
        Some("Heading1")
    );
    assert!(first.is_legal);
    assert_eq!(first.suffix.as_deref(), Some("space"));
    assert_eq!(first.text.as_deref(), Some("%1."));
    assert_eq!(first.pic_bullet_id, Some(7));
    assert!(first.justification.is_some());
    assert!(first.tentative_off && !first.tentative);
    assert!(first.paragraph.indentation.is_some());
    assert!(first.run.bold.is_on());

    let second = abstract_num.level(Ilvl(1)).expect("level 1");
    assert!(second.tentative);
    assert_eq!(second.pic_bullet_id, None);

    let num = numbering.num(NumId(5)).expect("first instance");
    assert_eq!(num.abstract_num_id, AbstractNumId(3));
    assert_eq!(num.overrides.len(), 1);
    let over = &num.overrides[0];
    assert_eq!(over.start_override, Some(9));
    assert_eq!(
        over.level
            .as_ref()
            .and_then(|level| level.format.as_deref()),
        Some("bullet")
    );
    assert_eq!(numbering.len(), 1);
    assert!(numbering.num(NumId(6)).is_none());

    assert!(message(&document, "w:abstractNumId").contains("duplicate abstractNumId 3"));
    assert!(message(&document, "w:numId").contains("duplicate numId 5"));
    assert!(message(&document, "w:num").contains("without abstractNumId"));
    assert!(message(&document, "w:num@durableId").contains("durableId"));
    assert!(message(&document, "w:lvl@tplc").contains("template code"));
    let pic_ref = document.support.get("w:lvlPicBulletId").expect("recorded");
    assert_eq!(pic_ref.status, SupportStatus::Partial);
    assert_eq!(pic_ref.count, 2);
    assert!(document.support.get("w:startOverride").is_some());
    assert!(document.support.get("w:lvlOverride").is_some());
}

#[test]
fn picture_bullets_keep_their_drawing_and_name_what_is_lost() {
    let document = parse_numbering(
        &format!(
            "<w:numPicBullet w:numPicBulletId=\"0\">\n<x:foreign/>\n<w:unknown/>\n{BULLET_DRAWING}\n</w:numPicBullet>\n\
<w:numPicBullet w:numPicBulletId=\"0\">{BULLET_DRAWING}</w:numPicBullet>\n\
<w:numPicBullet w:numPicBulletId=\"x\">{BULLET_DRAWING}</w:numPicBullet>\n\
<w:numPicBullet w:numPicBulletId=\"1\"/>\n\
<w:numPicBullet w:numPicBulletId=\"2\"><w:pict/></w:numPicBullet>\n\
<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:lvlPicBulletId w:val=\"0\"/></w:lvl></w:abstractNum>"
        ),
        None,
    );
    let numbering = &document.numbering;
    assert_eq!(numbering.pic_bullets().count(), 1);
    let bullet = numbering.pic_bullet(0).expect("bullet 0");
    assert_eq!(bullet.id, 0);
    assert!(numbering.pic_bullet(1).is_none());
    assert!(numbering.pic_bullet(2).is_none());
    assert!(!numbering.is_empty());

    assert!(message(&document, "w:numPicBulletId").contains("duplicate numPicBulletId 0"));
    let bullet_use = document.support.get("w:numPicBullet").expect("recorded");
    // Unsupported (the VML picture) outranks the skipped bullets.
    assert_eq!(bullet_use.status, SupportStatus::Unsupported);
    assert!(bullet_use.count >= 4, "{}", bullet_use.count);
    let pic_ref = document.support.get("w:lvlPicBulletId").expect("recorded");
    assert_eq!(pic_ref.status, SupportStatus::Supported);
}

/// Each way a `numStyleLink` chain can break is reported with the link left
/// unresolved; the instance falls back to the abstract it names.
#[test]
fn broken_num_style_links_are_reported() {
    let cases: [(&str, &str, &str); 5] = [
        ("Missing", "", "missing style 'Missing'"),
        (
            "Para",
            "<w:style w:type=\"paragraph\" w:styleId=\"Para\"/>",
            "not a numbering style",
        ),
        (
            "NoNum",
            "<w:style w:type=\"numbering\" w:styleId=\"NoNum\"/>",
            "has no pPr/numPr/numId",
        ),
        (
            "Dangling",
            "<w:style w:type=\"numbering\" w:styleId=\"Dangling\">\
<w:pPr><w:numPr><w:numId w:val=\"40\"/></w:numPr></w:pPr></w:style>",
            "references missing numId 40",
        ),
        (
            "ToMissing",
            "<w:style w:type=\"numbering\" w:styleId=\"ToMissing\">\
<w:pPr><w:numPr><w:numId w:val=\"9\"/></w:numPr></w:pPr></w:style>",
            "missing abstractNumId 77",
        ),
    ];
    for (link, styles, expected) in cases {
        let document = parse_numbering(
            &format!(
                "<w:abstractNum w:abstractNumId=\"0\"><w:numStyleLink w:val=\"{link}\"/></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
<w:num w:numId=\"9\"><w:abstractNumId w:val=\"77\"/></w:num>"
            ),
            Some(styles),
        );
        let reported = message(&document, "w:numStyleLink");
        assert!(reported.contains(expected), "{link}: {reported}");
        let resolved = document.numbering.resolved_abstract(NumId(1));
        assert_eq!(
            resolved.map(|abstract_num| abstract_num.id),
            Some(AbstractNumId(0))
        );
    }
}

/// After the definitions are edited, `refresh_definitions` follows the links
/// again: a link to a numbering style resolves to the abstract carrying levels.
#[test]
fn refreshed_definitions_follow_num_style_links_again() {
    let mut document = parse_numbering(
        "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
<w:abstractNum w:abstractNumId=\"1\"><w:numStyleLink w:val=\"List\"/></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
<w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>",
        Some(
            "<w:style w:type=\"numbering\" w:styleId=\"List\">\
<w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>",
        ),
    );
    let target = |document: &Document| {
        document
            .numbering
            .resolved_abstract(NumId(2))
            .map(|abstract_num| abstract_num.id)
    };
    assert_eq!(target(&document), Some(AbstractNumId(0)));
    document.numbering.clear_link_targets();
    assert_eq!(target(&document), Some(AbstractNumId(1)));
    strict_ooxml_wml::resolve::refresh_definitions(&mut document);
    assert_eq!(target(&document), Some(AbstractNumId(0)));
}

fn abstract_num(id: u32, name: &str) -> AbstractNum {
    AbstractNum {
        id: AbstractNumId(id),
        name: Some(name.into()),
        multi_level_type: None,
        num_style_link: None,
        style_link: None,
        levels: vec![Level::new(Ilvl(0))],
        location: SourceLocation::unknown(),
    }
}

fn num(id: u32, abstract_id: u32) -> Num {
    Num {
        num_id: NumId(id),
        abstract_num_id: AbstractNumId(abstract_id),
        overrides: Vec::new(),
        location: SourceLocation::unknown(),
    }
}

#[test]
fn the_table_replaces_and_removes_definitions_in_place() {
    let mut table = NumberingTable::new();
    assert!(table.is_empty());
    for id in 0..3 {
        assert!(table.insert_abstract(abstract_num(id, "first")));
        assert!(table.insert_num(num(id + 10, id)));
    }
    assert!(!table.insert_abstract(abstract_num(1, "again")));
    assert!(!table.insert_num(num(11, 0)));

    assert!(table.replace_abstract(abstract_num(1, "replaced")));
    assert!(!table.replace_abstract(abstract_num(9, "absent")));
    assert_eq!(
        table
            .abstract_num(AbstractNumId(1))
            .and_then(|found| found.name.as_deref()),
        Some("replaced")
    );
    assert!(table.replace_num(num(11, 2)));
    assert!(!table.replace_num(num(99, 0)));
    assert_eq!(
        table.num(NumId(11)).map(|found| found.abstract_num_id),
        Some(AbstractNumId(2))
    );

    // Removing the first shifts the others' indices: they stay reachable.
    let removed = table.remove_abstract(AbstractNumId(0)).expect("removed");
    assert_eq!(removed.id, AbstractNumId(0));
    assert!(table.remove_abstract(AbstractNumId(0)).is_none());
    assert!(table.abstract_num(AbstractNumId(1)).is_some());
    assert!(table.abstract_num(AbstractNumId(2)).is_some());
    assert_eq!(table.abstracts().count(), 2);

    let removed = table.remove_num(NumId(10)).expect("removed");
    assert_eq!(removed.num_id, NumId(10));
    assert!(table.remove_num(NumId(10)).is_none());
    assert_eq!(
        table.num(NumId(12)).map(|found| found.num_id),
        Some(NumId(12))
    );
    assert_eq!(table.len(), 2);

    // A link target that names a removed abstract falls back to the start.
    table.set_link_target(AbstractNumId(2), AbstractNumId(0));
    assert_eq!(
        table.resolved_abstract(NumId(12)).map(|found| found.id),
        Some(AbstractNumId(2))
    );
    table.clear_link_targets();
    assert!(table.resolved_abstract(NumId(10)).is_none());
}
