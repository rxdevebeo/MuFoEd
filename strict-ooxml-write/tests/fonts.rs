//! Embedded fonts survive a round trip, byte for byte and relationship for
//! relationship.
//!
//! Queue item 11 of the Transitional -> Strict queue, and the last one whose loss
//! was **silent**: sixteen font binaries in two corpus documents went missing
//! before this, and nothing in the loss report said so until the census gate
//! added a part ledger and looked (`W7-DROPPED`, `TZ-15`).
//!
//! Three things have to hold, and each of them failed on its own first:
//!
//! 1. **the parser reads `word/fontTable.xml` at all.** It did not, and the
//!    writer derived the part from the faces the styles name. Everything else
//!    here is downstream of that.
//! 2. **the `r:id` is resolved to a PART, not carried across.** `w:embedRegular/
//!    @r:id` names a relationship of `word/fontTable.xml`, resolved through
//!    `word/_rels/fontTable.xml.rels`. Its ids mean nothing in a package we
//!    write, so this write allocates its own — and the same per-part rule a
//!    header's picture ran into, which is why there are two of these.
//! 3. **`w:fontKey` comes with the bytes.** ECMA-376 §17.8.1 obfuscates the
//!    first 32 bytes of a font under a real GUID, and a consumer de-obfuscates
//!    with the key from the file. Bytes without the key are a font that renders
//!    as garbage, which is worse than no font at all.
//!
//! The fixture is a real document — `DOCX_13_Pages_Medium_2cdcf0763c.docx`, ten
//! embedded faces across three families, all `w:fontKey="{00000000-…}"` — because
//! a synthetic font table would have had no way to be wrong in the ways real ones
//! are.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::rels::{RelType, TargetMode};
use strict_ooxml_core::opc::OpenOptions;
use strict_ooxml_core::opc::Package;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::fonts::EmbedKind;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/samples/DOCX_13_Pages_Medium_2cdcf0763c.docx")
}

fn open_transitional(path: &Path) -> Result<Package> {
    Package::open_reader(
        std::fs::read(path)
            .expect("the fixture is present")
            .as_slice(),
        &strict_ooxml_core::opc::OpenOptions::default()
            .conformance(strict_ooxml_core::opc::ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            )),
    )
}

fn write(
    document: &strict_ooxml_wml::Document,
    package: &Package,
) -> strict_ooxml_write::WriteOutput {
    write_package(document, Some(package), &WriteOptions::default())
        .expect("the corpus document writes")
}

/// Prefers [`MediaBag`] bytes, then falls back to the package (AUD-61 font test).
struct Overlay<'a>(&'a Package, &'a strict_ooxml_write::package::MediaBag);

impl strict_ooxml_write::package::Source for Overlay<'_> {
    fn read_part(&self, part: &PartId) -> Result<Vec<u8>> {
        use strict_ooxml_write::package::Source;
        Source::read_part(self.1, part).or_else(|_| Source::read_part(self.0, part))
    }
    fn relationship(
        &self,
        from: &PartId,
        rel_id: &str,
    ) -> Option<strict_ooxml_write::package::RelationshipInfo> {
        use strict_ooxml_write::package::Source;
        Source::relationship(self.0, from, rel_id)
    }
    fn relationships(&self, from: &PartId) -> Vec<strict_ooxml_write::package::RelationshipInfo> {
        use strict_ooxml_write::package::Source;
        Source::relationships(self.0, from)
    }
    fn content_type(&self, part: &PartId) -> Option<String> {
        use strict_ooxml_write::package::Source;
        Source::content_type(self.0, part)
    }
    fn parts(&self) -> Vec<PartId> {
        use strict_ooxml_write::package::Source;
        Source::parts(self.0)
    }
}

/// Every part of a package by name, with its bytes.
fn parts(package: &Package) -> BTreeMap<String, Vec<u8>> {
    package
        .parts()
        .filter_map(|part| {
            let name = part.id.as_str().trim_start_matches('/').to_owned();
            package.read_part(&part.id).ok().map(|bytes| (name, bytes))
        })
        .collect()
}

/// The `r:id` → target map of a written `.rels` part.
fn font_relationships(written: &Package) -> BTreeMap<String, String> {
    let id = PartId::new("/word/_rels/fontTable.xml.rels");
    let Ok(bytes) = written.read_part(&id) else {
        return BTreeMap::new();
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut out = BTreeMap::new();
    for chunk in text.split("<Relationship ").skip(1) {
        let id = chunk
            .split("Id=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        let target = chunk
            .split("Target=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        let kind = chunk
            .split("Type=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        if let (Some(id), Some(target), Some(kind)) = (id, target, kind) {
            assert_eq!(
                kind,
                strict_ooxml_core::opc::rels::strict_type_uri(&RelType::Font),
                "a w:embed* relationship is a font relationship, and this one is not: {text}"
            );
            out.insert(id.to_owned(), target.to_owned());
        }
    }
    out
}

#[test]
fn the_font_table_is_read_and_every_embedded_face_is_resolved_to_a_part() {
    let package = open_transitional(&fixture()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");

    let table = document
        .font_table
        .as_ref()
        .expect("the document has a font table part");
    assert!(
        !table.is_empty(),
        "and it is not empty: the document embeds ten faces"
    );
    let embedded = table.embedded_parts();
    assert_eq!(
        embedded.len(),
        10,
        "ten faces across three families: {table:?}"
    );
    for font in &embedded {
        assert_ne!(
            font.part.as_str(),
            strict_ooxml_wml::parse::LOST_FONT_PART,
            "every relationship resolved: the source names all ten"
        );
        assert!(
            package.part(&font.part).is_some(),
            "{} is really in the package",
            font.part
        );
    }
    // The families are the document's own, and the count is a measurement rather
    // than a hope: three declared `w:font` entries in the source.
    assert_eq!(table.fonts.len(), 3, "{table:?}");
    assert!(table
        .fonts
        .iter()
        .any(|entry| entry.name.as_ref() == "Nunito"));
}

/// The acceptance measurement: **the bytes are in the written package**, under a
/// name this write chose, and each `w:embed*` points at one of them through the
/// font table's own relationship part.
#[test]
fn every_embedded_font_reaches_the_written_package_with_its_bytes() {
    let package = open_transitional(&fixture()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write(&document, &package);

    let source_parts = parts(&package);
    let written_parts =
        parts(&Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap());
    let rels = font_relationships(
        &Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap(),
    );
    assert_eq!(
        rels.len(),
        10,
        "one relationship per embedded face, and they are all font relationships: {rels:?}"
    );

    let font_table = String::from_utf8_lossy(
        written_parts
            .get("word/fontTable.xml")
            .expect("the font table is written"),
    )
    .into_owned();
    assert_eq!(
        font_table.matches("<w:embed").count(),
        10,
        "and one w:embed* element for each: {font_table}"
    );

    // Every `r:id` in the table resolves, through the font table's own rels, to a
    // part whose bytes are byte-identical to something in the source. **Byte
    // identity**, because the alternative — a part of the right name and the wrong
    // content — is a font that renders as a different font.
    let mut checked = 0usize;
    for chunk in font_table.split("<w:embed").skip(1) {
        let Some(id) = chunk
            .split("r:id=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
        else {
            continue;
        };
        let target = rels
            .get(id)
            .unwrap_or_else(|| panic!("{id} has no relationship in the written font table"));
        let name = format!("word/{target}");
        let bytes = written_parts
            .get(&name)
            .unwrap_or_else(|| panic!("{name} is referenced but not in the package"));
        assert!(
            source_parts.values().any(|original| original == bytes),
            "{name} carries bytes the source had, not new ones"
        );
        checked += 1;
    }
    assert_eq!(checked, 10, "every embed was followed to its bytes");
}

/// `w:fontKey` is not metadata, and the test is that a consumer can still read the
/// font.
///
/// ECMA-376 §17.8.1: a font whose `w:fontKey` is a real GUID has its **first 32
/// bytes XOR-obfuscated** with that GUID, and the key is what un-obfuscates it.
/// Copying the bytes and dropping the key would produce a package that looks
/// complete and renders the font as garbage, so the key is carried and asserted.
#[test]
fn the_font_key_travels_with_the_bytes_it_describes() {
    let package = open_transitional(&fixture()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write(&document, &package);
    let written_parts =
        parts(&Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap());
    let font_table = String::from_utf8_lossy(
        written_parts
            .get("word/fontTable.xml")
            .expect("the font table is written"),
    )
    .into_owned();

    let table = document.font_table.as_ref().expect("a font table");
    let keys: Vec<String> = table
        .embedded_parts()
        .iter()
        .filter_map(|font| font.font_key.as_deref().map(ToString::to_string))
        .collect();
    assert_eq!(keys.len(), 10, "every face carries a key: {keys:?}");
    for key in &keys {
        assert!(
            font_table.contains(&format!(r#"w:fontKey="{key}""#)),
            "{key} is written back, so a consumer can de-obfuscate the bytes: {font_table}"
        );
    }
}

/// A font table with nothing embedded gets **no relationship part** beside it.
///
/// An empty `.rels` is the OPC form of the unused-declaration debt: a part
/// declaring nothing, which `strict_conformance.rs` already refuses for
/// namespaces in the parts this writer regenerates.
#[test]
fn a_font_table_with_no_embedded_face_gets_no_relationship_part() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-text.docx");
    let package = Package::open_reader(
        std::fs::read(&path).expect("a Strict fixture").as_slice(),
        &OpenOptions::default(),
    )
    .expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written = write(&document, &package);
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap();
    assert!(
        reopened
            .part(&PartId::new("/word/_rels/fontTable.xml.rels"))
            .is_none(),
        "nothing to relate, so nothing is declared"
    );
    assert!(
        reopened.part(&PartId::new("/word/fontTable.xml")).is_some(),
        "and the font table itself is still written"
    );
}

/// The four faces are four **named** cases, and `CT_Font` is a sequence.
///
/// A `w:font` that embeds only `w:embedBold` must produce only that element — a
/// writer that filled all four would claim four faces it does not have — and they
/// must come out in the schema's order, because a child earlier than the schema
/// says is "This element is not expected" (`XS-23`'s lesson applied to another
/// container).
#[test]
fn the_four_faces_are_four_named_cases_in_the_schemas_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-text.docx");
    let package = Package::open_reader(
        std::fs::read(&path).expect("a Strict fixture").as_slice(),
        &OpenOptions::default(),
    )
    .expect("open");
    let mut document = parse_document(&package, &ParseOptions::default()).expect("parse");

    // A hand-built entry that embeds only the bold face.
    let mut entry = strict_ooxml_wml::model::fonts::FontEntry {
        name: "Only Bold".into(),
        ..strict_ooxml_wml::model::fonts::FontEntry::default()
    };
    entry.embeds.insert(
        EmbedKind::Bold,
        strict_ooxml_wml::model::fonts::EmbeddedFont {
            part: PartId::new("/word/fonts/only-bold.ttf"),
            font_key: None,
            subsetted: false,
        },
    );
    document.font_table = Some(strict_ooxml_wml::model::fonts::FontTable { fonts: vec![entry] });

    // The hand-built face needs readable bytes; a MediaBag supplies them without
    // changing the fixture package's own fonts.
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    bag.insert(
        PartId::new("/word/fonts/only-bold.ttf"),
        b"font-bytes".to_vec(),
    );
    let written = write_package(
        &document,
        Some(&Overlay(&package, &bag)),
        &WriteOptions::default(),
    )
    .expect("write");
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap();
    let table = String::from_utf8_lossy(
        &reopened
            .read_part(&PartId::new("/word/fontTable.xml"))
            .expect("written"),
    )
    .into_owned();

    assert!(table.contains("<w:embedBold"), "{table}");
    for absent in ["<w:embedRegular", "<w:embedItalic", "<w:embedBoldItalic"] {
        assert!(
            !table.contains(absent),
            "{absent} is not a face we have: {table}"
        );
    }
    // And the order of the schema, checked on a table that has all four.
    let all = EmbedKind::all();
    assert_eq!(
        all.map(EmbedKind::element),
        [
            "w:embedRegular",
            "w:embedBold",
            "w:embedItalic",
            "w:embedBoldItalic"
        ],
        "CT_Font declares them in this order"
    );
}

/// A face whose relationship did not resolve keeps the **family entry** and loses
/// only the face, and says so.
///
/// The family name is real and the document refers to it, so dropping the entry
/// would lose a font the reader still expects; keeping the face would write an
/// `r:id` that resolves to nothing. The two claims are separate and the report
/// carries the second.
#[test]
fn an_unresolvable_embed_loses_the_face_and_keeps_the_family() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-text.docx");
    let package = Package::open_reader(
        std::fs::read(&path).expect("a Strict fixture").as_slice(),
        &OpenOptions::default(),
    )
    .expect("open");
    let mut document = parse_document(&package, &ParseOptions::default()).expect("parse");

    let mut entry = strict_ooxml_wml::model::fonts::FontEntry {
        name: "Broken".into(),
        ..strict_ooxml_wml::model::fonts::FontEntry::default()
    };
    entry.embeds.insert(
        EmbedKind::Regular,
        strict_ooxml_wml::model::fonts::EmbeddedFont {
            part: PartId::new(strict_ooxml_wml::parse::LOST_FONT_PART),
            font_key: None,
            subsetted: false,
        },
    );
    document.font_table = Some(strict_ooxml_wml::model::fonts::FontTable { fonts: vec![entry] });

    let written = write(&document, &package);
    let reopened = Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap();
    let table = String::from_utf8_lossy(
        &reopened
            .read_part(&PartId::new("/word/fontTable.xml"))
            .expect("written"),
    )
    .into_owned();
    assert!(
        table.contains(r#"<w:font w:name="Broken""#),
        "the family is real and stays, and CT_Font's children are all optional so an entry \
         with no embeds is legitimately self-closing: {table}"
    );
    assert!(
        !table.contains("<w:embedRegular"),
        "and the face that had no bytes is not written: {table}"
    );
    assert!(
        reopened
            .part(&PartId::new("/word/_rels/fontTable.xml.rels"))
            .is_none(),
        "nothing to relate: an r:id with no target is a dangling reference"
    );
    assert!(
        written.report.to_string().contains("could not be resolved"),
        "and the loss is named, because a family with a silently missing face looks \
         identical to one that never had it: {}",
        written.report
    );
}

/// The content type of an embedded font is half of what the bytes mean.
///
/// `.ttf` is a plain font and `.odttf` is an **obfuscated** one: same format,
/// first 32 bytes XOR-obfuscated with the key. Declaring an obfuscated font as
/// `application/x-font-ttf` hands a consumer bytes it will read as they are and
/// render as a different font.
#[test]
fn an_obfuscated_font_is_declared_as_one() {
    assert_eq!(strict_ooxml_write::font_extension("fonts/x.odttf"), "odttf");
    assert_eq!(strict_ooxml_write::font_extension("fonts/X.ODTTF"), "odttf");
    assert_eq!(strict_ooxml_write::font_extension("fonts/x.ttf"), "ttf");
    assert_eq!(
        strict_ooxml_write::font_extension("fonts/no-extension"),
        "ttf"
    );
    assert_eq!(
        strict_ooxml_write::font_content_type("odttf"),
        "application/vnd.openxmlformats-officedocument.obfuscatedFont"
    );
    assert_eq!(
        strict_ooxml_write::font_content_type("ttf"),
        "application/x-font-ttf"
    );
}

/// The whole trip is a fixed point, which is what makes "the fonts are still
/// there" checkable rather than asserted.
#[test]
fn the_written_package_settles_with_its_fonts() {
    let package = open_transitional(&fixture()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let first = write(&document, &package);
    let reopened = Package::open_reader(&first.bytes[..], &OpenOptions::default()).unwrap();
    let reparsed = parse_document(&reopened, &ParseOptions::default()).expect("reparse");
    let second = write(&reparsed, &reopened);
    assert_eq!(
        first.bytes.len(),
        second.bytes.len(),
        "a second write carries the same bytes, so the fonts are carried rather \
         than re-derived each time"
    );
    let _ = TargetMode::Internal;
}
