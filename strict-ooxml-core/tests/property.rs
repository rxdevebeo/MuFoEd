//! Property-based tests (`proptest`) for the Stage-1 core.
//!
//! Covers the properties listed in `STAGE-1-TASK.md` §9.3: path-canonicalization
//! idempotence, ZIP round-trip, and panic-freedom over arbitrary bytes.

use std::io::Cursor;

use proptest::prelude::*;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::path::{canonicalize_part_name, resolve_target};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_core::xml::{XmlEvent, XmlReader};

mod common;

use common::build_zip;

fn segment() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,6}".prop_map(String::from),
        Just(".".to_owned()),
        Just("..".to_owned()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn path_canonicalization_is_idempotent(segments in prop::collection::vec(segment(), 1..6)) {
        let path = segments.join("/");
        if let Ok(id) = canonicalize_part_name(&path) {
            let again = canonicalize_part_name(id.as_str().trim_start_matches('/'));
            let same = matches!(&again, Ok(other) if *other == id);
            prop_assert!(same, "not idempotent: {path} -> {again:?}");
        }
    }

    #[test]
    fn resolved_targets_are_always_canonical(base in "[a-z]{1,4}/[a-z]{1,4}\\.xml", target in "[a-z]{1,4}(/[a-z]{1,4}){0,3}\\.xml") {
        let base = PartId::new(format!("/{base}"));
        if let Ok(Some(resolved)) = resolve_target(&base, &target, false) {
            prop_assert!(resolved.as_str().starts_with('/'));
            prop_assert!(!resolved.as_str().contains(".."));
        }
    }

    #[test]
    fn zip_round_trip(contents in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..128), 0..6)) {
        let owned: Vec<(String, Vec<u8>)> = contents
            .iter()
            .enumerate()
            .map(|(i, data)| (format!("word/part{i}.bin"), data.clone()))
            .collect();
        let mut entries: Vec<(&str, &[u8], bool)> = vec![
            ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.as_slice(), false),
            ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.as_slice(), false),
            ("word/document.xml", br#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"/>"#.as_slice(), false),
        ];
        for (name, data) in &owned {
            entries.push((name.as_str(), data.as_slice(), false));
        }
        let archive = build_zip(&entries);
        let options = OpenOptions::default();
        let package = Package::open_reader(Cursor::new(archive), &options).unwrap();
        for (name, data) in &owned {
            let id = PartId::new(format!("/{name}"));
            let bytes = package.read_part(&id).unwrap();
            prop_assert_eq!(&bytes, data);
        }
    }

    #[test]
    fn package_open_never_panics(data in prop::collection::vec(any::<u8>(), 0..2048)) {
        let options = OpenOptions::default().limits(ResourceLimits {
            max_single_uncompressed: 64 * 1024,
            max_total_uncompressed: 128 * 1024,
            ..ResourceLimits::default()
        });
        let _ = Package::open_reader(Cursor::new(data), &options);
    }

    #[test]
    fn xml_reader_never_panics(data in prop::collection::vec(any::<u8>(), 0..1024)) {
        let limits = ResourceLimits {
            max_xml_depth: 16,
            max_xml_attributes_per_elem: 16,
            max_text_len: 1024,
            ..ResourceLimits::default()
        };
        if let Ok(mut reader) = XmlReader::new(&data, PartId::new("/t.xml"), &limits) {
            for _ in 0..256 {
                match reader.next_event() {
                    Ok(XmlEvent::Eof) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        }
    }
}
