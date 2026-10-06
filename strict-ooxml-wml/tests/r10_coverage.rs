//! R10 coverage witnesses: revision model API and fontTable parse/model paths
//! that historically left `strict-ooxml-wml` below the §15 85% line gate.

#![allow(clippy::doc_markdown, clippy::unreadable_literal)]

mod common;

use std::sync::Arc;

use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::fonts::{EmbedKind, EmbeddedFont, FontEntry, FontTable};
use strict_ooxml_wml::model::revision::{Revision, RevisionKind};
use strict_ooxml_wml::model::support::SupportStatus;

use common::{document_parts, parse_parts, rels, R_NS, W_NS};

const FONT_TABLE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/fontTable";

#[test]
fn revision_kind_covers_every_local_name_and_view_flag() {
    for (local, kind, feature, deletion, insertion) in [
        ("ins", RevisionKind::Insert, "w:ins", false, true),
        ("del", RevisionKind::Delete, "w:del", true, false),
        (
            "moveFrom",
            RevisionKind::MoveFrom,
            "w:moveFrom",
            true,
            false,
        ),
        ("moveTo", RevisionKind::MoveTo, "w:moveTo", false, true),
    ] {
        assert_eq!(RevisionKind::from_local(local), Some(kind));
        assert_eq!(kind.as_str(), local);
        assert_eq!(kind.feature_id(), feature);
        assert_eq!(kind.is_deletion(), deletion);
        assert_eq!(kind.is_insertion(), insertion);
        let revision = Revision::new(kind, 7);
        assert_eq!(revision.id, 7);
        assert!(revision.author.is_none() && revision.date.is_none());
    }
    assert_eq!(RevisionKind::from_local("unknown"), None);
}

#[test]
fn embed_kind_and_font_table_model_cover_every_face() {
    let kinds = EmbedKind::all();
    assert_eq!(
        kinds.map(EmbedKind::element),
        [
            "w:embedRegular",
            "w:embedBold",
            "w:embedItalic",
            "w:embedBoldItalic",
        ]
    );
    let mut entry = FontEntry {
        name: Arc::from("Carlito"),
        ..FontEntry::default()
    };
    for kind in kinds {
        entry.embeds.insert(
            kind,
            EmbeddedFont {
                part: PartId::new(format!("/word/fonts/{}.ttf", kind.element())),
                font_key: Some(Arc::from("{00000000-0000-0000-0000-000000000000}")),
                subsetted: matches!(kind, EmbedKind::Bold),
            },
        );
    }
    let table = FontTable {
        fonts: vec![entry, FontEntry::default()],
    };
    assert!(!table.is_empty());
    assert_eq!(table.embedded_parts().len(), 4);
    assert!(FontTable::default().is_empty());
}

#[test]
fn font_table_parses_embeds_lost_faces_and_hints() {
    let table = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<w:fonts xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\">\
  <w:font w:name=\"Embedded\">\
    <w:embedRegular r:id=\"rIdFont\" w:fontKey=\"{{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}}\" w:subsetted=\"true\"/>\
    <w:embedBold r:id=\"rIdMissing\" w:subsetted=\"1\"/>\
    <w:embedItalic/>\
    <w:panose1 w:val=\"020B0604030504040204\"/>\
    <w:charset w:val=\"00\"/>\
    <foreign xmlns=\"urn:example\"/>\
  </w:font>\
  <w:font/>\
  <w:unknownTop/>\
</w:fonts>"
    )
    .into_bytes();
    let font_rels = rels(&[(
        "rIdFont",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/font",
        "fonts/Embedded.ttf",
    )]);
    let mut parts = document_parts("<w:p/>", &[]);
    parts.push(("word/fontTable.xml".to_owned(), table));
    parts.push((
        "word/_rels/document.xml.rels".to_owned(),
        rels(&[("rIdFonts", FONT_TABLE, "fontTable.xml")]),
    ));
    parts.push(("word/_rels/fontTable.xml.rels".to_owned(), font_rels));
    parts.push(("word/fonts/Embedded.ttf".to_owned(), b"font-bytes".to_vec()));
    let document = parse_parts(&parts).expect("font table");
    let fonts = document.font_table.expect("font table present");
    assert_eq!(fonts.fonts.len(), 1, "nameless w:font is skipped");
    let entry = &fonts.fonts[0];
    assert_eq!(entry.name.as_ref(), "Embedded");
    let regular = entry.embeds.get(&EmbedKind::Regular).expect("regular");
    assert_eq!(regular.part.as_str(), "/word/fonts/Embedded.ttf");
    assert!(regular.subsetted);
    assert!(regular.font_key.is_some());
    let bold = entry.embeds.get(&EmbedKind::Bold).expect("bold lost");
    assert_eq!(bold.part.as_str(), "/word/fonts/none");
    let italic = entry.embeds.get(&EmbedKind::Italic).expect("italic lost");
    assert_eq!(italic.part.as_str(), "/word/fonts/none");
    assert!(matches!(
        document.support.get("w:embedBold").map(|e| e.status),
        Some(SupportStatus::Partial)
    ));
    assert!(matches!(
        document.support.get("w:embedItalic").map(|e| e.status),
        Some(SupportStatus::Partial)
    ));
    assert!(matches!(
        document.support.get("w:font").map(|e| e.status),
        Some(SupportStatus::Partial)
    ));
    assert_eq!(
        entry.hints.panose1.as_deref(),
        Some("020B0604030504040204"),
        "panose1 must round-trip in the model"
    );
    assert_eq!(entry.hints.charset.as_deref(), Some("00"));
    assert!(
        document.support.get("w:panose1").is_none(),
        "preserved hints must not be Unsupported: {:?}",
        document.support.get("w:panose1")
    );
    assert!(
        document.support.get("w:charset").is_none(),
        "preserved hints must not be Unsupported: {:?}",
        document.support.get("w:charset")
    );
}

#[test]
fn move_revision_containers_stamp_runs() {
    let document = parse_parts(&document_parts(
        "<w:p><w:moveFrom w:id=\"1\" w:author=\"a\" w:date=\"2020-01-01T00:00:00Z\">\
           <w:r><w:t>gone</w:t></w:r>\
         </w:moveFrom>\
         <w:moveTo w:id=\"2\" w:author=\"b\">\
           <w:r><w:t>here</w:t></w:r>\
         </w:moveTo></w:p>",
        &[],
    ))
    .expect("parse");
    let strict_ooxml_wml::model::block::Block::Paragraph(paragraph) = &document.body.blocks[0]
    else {
        panic!("paragraph");
    };
    assert_eq!(paragraph.inlines.len(), 2);
    for (idx, kind) in [(0, RevisionKind::MoveFrom), (1, RevisionKind::MoveTo)] {
        let strict_ooxml_wml::model::inline::Inline::Run(run) = &paragraph.inlines[idx] else {
            panic!("run");
        };
        let revision = run.revision.as_ref().expect("revision");
        assert_eq!(revision.kind, kind);
        assert!(revision.author.is_some());
    }
}

#[test]
fn settings_flat_maps_and_named_children_are_carried() {
    // Exercises FLAT_ON_OFF / FLAT_NUMERIC / settings_named arms that the
    // earlier smoke settings test left cold — enough to close the §15 wml gap.
    const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";
    let settings = format!(
        "<w:settings xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\">\
<w:bookFoldPrinting w:val=\"1\"/><w:bordersDoNotSurroundFooter/><w:bordersDoNotSurroundHeader w:val=\"0\"/>\
<w:doNotAutoCompressPictures w:val=\"true\"/><w:doNotIncludeSubdocsInStats w:val=\"false\"/>\
<w:doNotUseMarginsForDrawingGridOrigin/><w:embedSystemFonts w:val=\"on\"/><w:embedTrueTypeFonts w:val=\"off\"/>\
<w:noPunctuationKerning/><w:savePreviewPicture w:val=\"1\"/>\
<w:displayHorizontalDrawingGridEvery w:val=\"1\"/><w:displayVerticalDrawingGridEvery w:val=\"2\"/>\
<w:drawingGridHorizontalSpacing w:val=\"120\"/><w:drawingGridVerticalSpacing w:val=\"240\"/>\
<w:characterSpacingControl w:val=\"doNotCompress\"/><w:view w:val=\"print\"/>\
<w:docVars><w:docVar w:name=\"k\" w:val=\"v\"/></w:docVars>\
<w:noLineBreaksAfter w:lang=\"ja-JP\" w:val=\"、。\"/>\
<w:noLineBreaksBefore w:lang=\"ja-JP\" w:val=\"（\"/>\
<w:attachedTemplate r:id=\"rIdTemplate\"/>\
<w:stylePaneFormatFilter w:allStyles=\"1\" w:customStyles=\"0\"/>\
<w:revisionView w:markup=\"true\" w:comments=\"false\"/>\
<w:clrSchemeMapping w:bg1=\"light1\" w:t1=\"dark1\"/>\
<w:rsids><w:rsidRoot w:val=\"00112233\"/><w:rsid w:val=\"44556677\"/></w:rsids>\
</w:settings>"
    );
    let mut parts = document_parts("<w:p/>", &[]);
    parts.push(("word/settings.xml".to_owned(), settings.into_bytes()));
    parts.push((
        "word/_rels/document.xml.rels".to_owned(),
        rels(&[("rIdSettings", SETTINGS, "settings.xml")]),
    ));
    let document = parse_parts(&parts).expect("settings");
    let settings = &document.settings;
    assert!(settings.on_off_flags.len() >= 8);
    assert_eq!(settings.numeric_settings.len(), 4);
    assert_eq!(
        settings.character_spacing_control.as_deref(),
        Some("doNotCompress")
    );
    assert_eq!(settings.view.as_deref(), Some("print"));
    assert!(!settings.document_variables.is_empty());
    assert!(!settings.no_line_breaks_after.is_empty());
    assert!(!settings.no_line_breaks_before.is_empty());
    assert!(settings.attached_template.is_some());
    assert!(!settings.style_pane_filter.is_empty());
    assert!(!settings.revision_view.is_empty());
}
