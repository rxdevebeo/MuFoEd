//! `w:sdtPr` keeps the control type, lock, binding and flags of a content
//! control, and records every child it does not model (`CT_SdtPr`).

#![allow(clippy::doc_markdown)]

mod common;

use strict_ooxml_wml::model::block::{
    Block, SdtCheckboxState, SdtContainer, SdtControl, SdtDataBinding, SdtDate, SdtDateMapping,
    SdtListItem, SdtLock,
};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::Document;

use common::{document_parts, parse_parts};

const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

fn parse_body(body: &str) -> Document {
    parse_parts(&document_parts(body, &[])).expect("parse document")
}

fn sdt_with(sdt_pr: &str) -> String {
    format!(
        "<w:sdt><w:sdtPr>{sdt_pr}</w:sdtPr>\
         <w:sdtContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:sdtContent></w:sdt>"
    )
}

fn first_sdt(document: &Document) -> &SdtContainer {
    match &document.body.blocks[0] {
        Block::SdtBlock(sdt) => sdt,
        other => panic!("expected sdt block, got {other:?}"),
    }
}

fn item(display_text: &str, value: &str) -> SdtListItem {
    SdtListItem {
        display_text: Some(display_text.into()),
        value: Some(value.into()),
    }
}

#[test]
fn drop_down_list_keeps_items_and_last_value() {
    let document = parse_body(&sdt_with(
        "<w:alias w:val=\"Choice\"/><w:tag w:val=\"choice\"/><w:id w:val=\"7\"/>\
         <w:dropDownList w:lastValue=\"B\">\
         <w:listItem w:displayText=\"Alpha\" w:value=\"A\"/>\
         <w:listItem w:displayText=\"Beta\" w:value=\"B\"/>\
         </w:dropDownList>",
    ));
    let sdt = first_sdt(&document);
    assert_eq!(
        sdt.control,
        Some(SdtControl::DropDownList {
            items: vec![item("Alpha", "A"), item("Beta", "B")],
            last_value: Some("B".into()),
        })
    );
    assert!(sdt.unmodelled.is_empty(), "{:?}", sdt.unmodelled);
}

#[test]
fn combo_box_is_distinct_from_drop_down_list() {
    let document = parse_body(&sdt_with(
        "<w:comboBox><w:listItem w:displayText=\"One\" w:value=\"1\"/></w:comboBox>",
    ));
    assert_eq!(
        first_sdt(&document).control,
        Some(SdtControl::ComboBox {
            items: vec![item("One", "1")],
            last_value: None,
        })
    );
}

#[test]
fn date_keeps_full_date_format_lid_mapping_and_calendar() {
    let document = parse_body(&sdt_with(
        "<w:date w:fullDate=\"2026-10-07T00:00:00Z\">\
         <w:dateFormat w:val=\"dd.MM.yyyy\"/><w:lid w:val=\"ru-RU\"/>\
         <w:storeMappedDataAs w:val=\"dateTime\"/><w:calendar w:val=\"gregorian\"/>\
         </w:date>",
    ));
    assert_eq!(
        first_sdt(&document).control,
        Some(SdtControl::Date(SdtDate {
            full_date: Some("2026-10-07T00:00:00Z".into()),
            format: Some("dd.MM.yyyy".into()),
            lid: Some("ru-RU".into()),
            store_mapped_as: Some(SdtDateMapping::DateTime),
            calendar: Some("gregorian".into()),
        }))
    );
}

#[test]
fn lock_temporary_binding_label_and_tab_index_are_kept() {
    let document = parse_body(&sdt_with(
        "<w:id w:val=\"1\"/><w:lock w:val=\"sdtContentLocked\"/><w:temporary/>\
         <w:dataBinding w:prefixMappings=\"xmlns:ns0='urn:x'\" w:xpath=\"/ns0:root[1]/ns0:name[1]\" \
         w:storeItemID=\"{11111111-2222-3333-4444-555555555555}\"/>\
         <w:label w:val=\"3\"/><w:tabIndex w:val=\"5\"/><w:text w:multiLine=\"true\"/>",
    ));
    let sdt = first_sdt(&document);
    assert_eq!(sdt.lock, Some(SdtLock::SdtContentLocked));
    assert!(sdt.temporary);
    assert_eq!(
        sdt.data_binding,
        Some(SdtDataBinding {
            prefix_mappings: Some("xmlns:ns0='urn:x'".into()),
            xpath: "/ns0:root[1]/ns0:name[1]".into(),
            store_item_id: "{11111111-2222-3333-4444-555555555555}".into(),
        })
    );
    assert_eq!(sdt.label, Some(3));
    assert_eq!(sdt.tab_index, Some(5));
    assert_eq!(sdt.control, Some(SdtControl::Text { multi_line: true }));
    assert!(sdt.unmodelled.is_empty(), "{:?}", sdt.unmodelled);
}

#[test]
fn temporary_false_is_off() {
    let document = parse_body(&sdt_with("<w:temporary w:val=\"false\"/><w:richText/>"));
    let sdt = first_sdt(&document);
    assert!(!sdt.temporary);
    assert_eq!(sdt.control, Some(SdtControl::RichText));
}

#[test]
fn invalid_lock_is_recorded_and_ignored() {
    let document = parse_body(&sdt_with("<w:lock w:val=\"bogus\"/>"));
    assert_eq!(first_sdt(&document).lock, None);
    let entry = document
        .support
        .get("w:sdtPr/w:lock")
        .expect("invalid lock recorded");
    assert_eq!(entry.status, SupportStatus::Partial);
}

#[test]
fn doc_part_obj_keeps_its_category() {
    let document = parse_body(&sdt_with(
        "<w:docPartObj><w:docPartGallery w:val=\"Cover Pages\"/>\
         <w:docPartCategory w:val=\"Custom\"/><w:docPartUnique/></w:docPartObj>",
    ));
    let sdt = first_sdt(&document);
    assert_eq!(sdt.doc_part_gallery.as_deref(), Some("Cover Pages"));
    assert_eq!(sdt.doc_part_category.as_deref(), Some("Custom"));
    assert!(sdt.doc_part_unique);
    assert_eq!(sdt.control, None);
}

#[test]
fn w14_checkbox_is_modelled() {
    let document = parse_body(&sdt_with(&format!(
        "<w14:checkbox xmlns:w14=\"{W14_NS}\"><w14:checked w14:val=\"1\"/>\
         <w14:checkedState w14:val=\"2612\" w14:font=\"MS Gothic\"/>\
         <w14:uncheckedState w14:val=\"2610\" w14:font=\"MS Gothic\"/></w14:checkbox>"
    )));
    let state = |value: &str| SdtCheckboxState {
        value: Some(value.into()),
        font: Some("MS Gothic".into()),
    };
    assert_eq!(
        first_sdt(&document).control,
        Some(SdtControl::Checkbox {
            checked: true,
            checked_state: Some(state("2612")),
            unchecked_state: Some(state("2610")),
        })
    );
}

/// Unmodelled children - an unknown WML child, a `w15` extension and a second
/// control type, which `CT_SdtPr` forbids - are each recorded, never dropped
/// silently, and listed on the control for the writer.
#[test]
fn unmodelled_children_are_recorded() {
    let document = parse_body(&sdt_with(&format!(
        "<w:tag w:val=\"t\"/><w15:color xmlns:w15=\"{W15_NS}\" w15:val=\"FF0000\"/>\
         <w:zzzUnknown/><w:dropDownList/><w:citation/>"
    )));
    let sdt = first_sdt(&document);
    let color = format!("w:sdtPr/ext:{W15_NS}:color");
    for feature in [color.as_str(), "w:sdtPr/w:zzzUnknown", "w:sdtPr/w:citation"] {
        let entry = document
            .support
            .get(feature)
            .unwrap_or_else(|| panic!("{feature} must be recorded"));
        assert_eq!(entry.status, SupportStatus::Partial, "{feature}");
        assert!(
            sdt.unmodelled.iter().any(|id| &**id == feature),
            "{feature} not listed: {:?}",
            sdt.unmodelled
        );
    }
    assert_eq!(sdt.tag.as_deref(), Some("t"));
    assert_eq!(
        sdt.control,
        Some(SdtControl::DropDownList {
            items: Vec::new(),
            last_value: None,
        })
    );
}

/// AUD-41 row-level unwrapping keeps the new fields too.
#[test]
fn row_level_sdt_keeps_lock_and_control() {
    let document = parse_body(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"1000\"/></w:tblGrid>\
         <w:sdt><w:sdtPr><w:lock w:val=\"sdtLocked\"/><w:group/></w:sdtPr><w:sdtContent>\
         <w:tr><w:tc><w:p/></w:tc></w:tr></w:sdtContent></w:sdt></w:tbl>",
    );
    let Block::Table(table) = &document.body.blocks[0] else {
        panic!("expected table");
    };
    let sdt = table.rows[0].sdt.as_ref().expect("row keeps sdtPr");
    assert_eq!(sdt.lock, Some(SdtLock::SdtLocked));
    assert_eq!(sdt.control, Some(SdtControl::Group));
}
