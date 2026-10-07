//! Content-control properties survive parse -> write -> parse in `CT_SdtPr`
//! order, and whatever Strict cannot carry is in the writer's loss report.

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::normalize::report::NormalizationReport;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::{Block, SdtContainer, SdtControl, SdtDateMapping, SdtLock};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::Document;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

fn open(bytes: &[u8]) -> Result<Package, StrictError> {
    Package::open_reader(bytes, &OpenOptions::default())
}

fn parse(package: &Package) -> Result<Document, StrictError> {
    parse_document(package, &ParseOptions::default())
}

struct RoundTrip {
    before: Document,
    after: Document,
    xml: String,
    report: NormalizationReport,
}

fn round_trip(sdt_pr: &str) -> RoundTrip {
    let body = format!(
        "<w:sdt><w:sdtPr>{sdt_pr}</w:sdtPr>\
         <w:sdtContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:sdtContent></w:sdt>"
    );
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(&body)
        .build();
    let package = open(&bytes).expect("open");
    let before = parse(&package).expect("parse");
    let written = write_package(&before, Some(&package), &WriteOptions::default()).expect("write");
    let reopened = open(&written.bytes).expect("reopen");
    let after = parse(&reopened).expect("reparse");
    let xml = String::from_utf8(
        reopened
            .read_part(&PartId::new("/word/document.xml"))
            .expect("document"),
    )
    .expect("utf-8");
    RoundTrip {
        before,
        after,
        xml,
        report: written.report,
    }
}

fn first_sdt(document: &Document) -> &SdtContainer {
    match &document.body.blocks[0] {
        Block::SdtBlock(sdt) => sdt,
        other => panic!("expected sdt block, got {other:?}"),
    }
}

/// The written `w:sdtPr`, without its surrounding tags.
fn sdt_pr(xml: &str) -> &str {
    let start = xml.find("<w:sdtPr>").expect("sdtPr");
    let rest = &xml[start..];
    let end = rest.find("</w:sdtPr>").expect("sdtPr end");
    &rest[..end]
}

/// Asserts `needles` appear in `haystack` in this order.
fn assert_in_order(haystack: &str, needles: &[&str]) {
    let mut from = 0;
    for needle in needles {
        let at = haystack[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} missing or out of order in {haystack}"));
        from += at + needle.len();
    }
}

fn sdt_losses(report: &NormalizationReport) -> Vec<String> {
    report
        .losses()
        .into_iter()
        .map(|loss| loss.feature_id)
        .filter(|feature| feature.starts_with("w:sdt") || feature.starts_with("w14:"))
        .collect()
}

#[test]
fn drop_down_list_round_trips() {
    let trip = round_trip(
        "<w:tag w:val=\"choice\"/>\
         <w:dropDownList w:lastValue=\"B\">\
         <w:listItem w:displayText=\"Alpha\" w:value=\"A\"/>\
         <w:listItem w:displayText=\"Beta\" w:value=\"B\"/>\
         </w:dropDownList>",
    );
    let control = first_sdt(&trip.before).control.clone();
    assert!(
        matches!(&control, Some(SdtControl::DropDownList { items, .. }) if items.len() == 2),
        "{control:?}"
    );
    assert_eq!(first_sdt(&trip.after).control, control);
    assert!(
        trip.xml.contains(
            "<w:dropDownList w:lastValue=\"B\">\
             <w:listItem w:displayText=\"Alpha\" w:value=\"A\"/>\
             <w:listItem w:displayText=\"Beta\" w:value=\"B\"/>\
             </w:dropDownList>"
        ),
        "{}",
        trip.xml
    );
    assert!(sdt_losses(&trip.report).is_empty(), "{:?}", trip.report);
}

#[test]
fn date_round_trips() {
    let trip = round_trip(
        "<w:date w:fullDate=\"2026-10-07T00:00:00Z\">\
         <w:dateFormat w:val=\"dd.MM.yyyy\"/><w:lid w:val=\"ru-RU\"/>\
         <w:storeMappedDataAs w:val=\"date\"/><w:calendar w:val=\"gregorian\"/>\
         </w:date>",
    );
    let Some(SdtControl::Date(date)) = &first_sdt(&trip.before).control else {
        panic!("expected a date control");
    };
    assert_eq!(date.full_date.as_deref(), Some("2026-10-07T00:00:00Z"));
    assert_eq!(date.format.as_deref(), Some("dd.MM.yyyy"));
    assert_eq!(date.lid.as_deref(), Some("ru-RU"));
    assert_eq!(date.store_mapped_as, Some(SdtDateMapping::Date));
    assert_eq!(
        first_sdt(&trip.after).control,
        first_sdt(&trip.before).control
    );
    assert_in_order(
        sdt_pr(&trip.xml),
        &[
            "<w:date w:fullDate=\"2026-10-07T00:00:00Z\">",
            "<w:dateFormat w:val=\"dd.MM.yyyy\"/>",
            "<w:lid w:val=\"ru-RU\"/>",
            "<w:storeMappedDataAs w:val=\"date\"/>",
            "<w:calendar w:val=\"gregorian\"/>",
        ],
    );
    assert!(sdt_losses(&trip.report).is_empty(), "{:?}", trip.report);
}

/// Lock, temporary, data binding and a multi-line text control round-trip, and
/// the written children follow the `CT_SdtPr` sequence even when the source
/// listed them out of order.
#[test]
fn lock_temporary_binding_and_text_round_trip_in_schema_order() {
    let trip = round_trip(
        "<w:text w:multiLine=\"1\"/><w:tabIndex w:val=\"2\"/><w:label w:val=\"4\"/>\
         <w:dataBinding w:xpath=\"/root[1]/name[1]\" w:storeItemID=\"{0F1E2D3C-0000-0000-0000-000000000001}\"/>\
         <w:temporary/><w:lock w:val=\"contentLocked\"/><w:id w:val=\"9\"/>\
         <w:tag w:val=\"name\"/><w:alias w:val=\"Name\"/>",
    );
    let before = first_sdt(&trip.before);
    assert_eq!(before.lock, Some(SdtLock::ContentLocked));
    assert!(before.temporary);
    assert_eq!(before.control, Some(SdtControl::Text { multi_line: true }));
    let binding = before.data_binding.as_ref().expect("binding");
    assert_eq!(&*binding.xpath, "/root[1]/name[1]");

    let after = first_sdt(&trip.after);
    assert_eq!(after.lock, before.lock);
    assert_eq!(after.temporary, before.temporary);
    assert_eq!(after.data_binding, before.data_binding);
    assert_eq!(after.label, Some(4));
    assert_eq!(after.tab_index, Some(2));
    assert_eq!(after.control, before.control);

    assert_in_order(
        sdt_pr(&trip.xml),
        &[
            "<w:alias w:val=\"Name\"/>",
            "<w:tag w:val=\"name\"/>",
            "<w:id w:val=\"9\"/>",
            "<w:lock w:val=\"contentLocked\"/>",
            "<w:temporary/>",
            "<w:dataBinding ",
            "<w:label w:val=\"4\"/>",
            "<w:tabIndex w:val=\"2\"/>",
            "<w:text w:multiLine=\"true\"/>",
        ],
    );
    assert!(sdt_losses(&trip.report).is_empty(), "{:?}", trip.report);
}

/// Children the model does not keep are in the parser's support report AND the
/// writer's loss report; the control itself and its modelled type survive.
#[test]
fn unmodelled_children_are_reported_by_parser_and_writer() {
    let trip = round_trip(&format!(
        "<w:tag w:val=\"t\"/><w15:color xmlns:w15=\"{W15_NS}\" w15:val=\"FF0000\"/>\
         <w:richText/><w:citation/>"
    ));
    let color = format!("w:sdtPr/ext:{W15_NS}:color");
    for feature in [color.as_str(), "w:sdtPr/w:citation"] {
        let entry = trip
            .before
            .support
            .get(feature)
            .unwrap_or_else(|| panic!("{feature} not in the support report"));
        assert_eq!(entry.status, SupportStatus::Partial, "{feature}");
    }
    let losses = sdt_losses(&trip.report);
    assert!(losses.contains(&color), "{losses:?}");
    assert!(
        losses.iter().any(|feature| feature == "w:sdtPr/w:citation"),
        "{losses:?}"
    );
    assert!(!trip.xml.contains("color"), "{}", trip.xml);
    assert!(!trip.xml.contains("w:citation"), "{}", trip.xml);
    assert!(trip.xml.contains("<w:richText/>"), "{}", trip.xml);
    let after = first_sdt(&trip.after);
    assert_eq!(after.tag.as_deref(), Some("t"));
    assert_eq!(after.control, Some(SdtControl::RichText));
}
