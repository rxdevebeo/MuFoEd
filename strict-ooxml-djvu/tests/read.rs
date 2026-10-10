//! `DjVu` regression tests using deterministic native fixtures.
#![allow(clippy::float_cmp)]
mod common;
use common::*;
use strict_ooxml_djvu::{DjvuDocument, DjvuError, DjvuLimits, TextLayer, ZoneKind};
fn open(bytes: &[u8]) -> DjvuDocument {
    DjvuDocument::open(bytes, DjvuLimits::default()).expect("readable fixture")
}
#[test]
fn compressed_and_plain_text_are_identical_and_utf8_is_preserved() {
    let a = open(&text_file("Привет 世界 café", false));
    let b = open(&text_file("Привет 世界 café", true));
    assert_eq!(a.pages()[0].text, b.pages()[0].text);
    let TextLayer::Readable(root) = &a.pages()[0].text else {
        panic!("zones")
    };
    assert_eq!(root.text, "Привет 世界 café");
    let line = &root.children[0].children[0];
    assert_eq!(line.rect.x, 10.0);
    assert_eq!(line.rect.y, 10.0);
    assert_eq!(line.rect.height, 10.0);
}
#[test]
fn sibling_line_deltas_use_the_previous_lower_left_corner() {
    let a = zone(ZoneKind::Line, [10, 10, 80, 10, 0], 1, &[]);
    let b = zone(ZoneKind::Line, [0, 10, 80, 10, 0], 1, &[]);
    let tree = zone(ZoneKind::Page, [0, 0, 100, 100, 0], 2, &[a, b]);
    let mut data = vec![0, 0, 2, b'a', b'b', 1];
    data.extend(tree);
    let doc = open(&single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &data),
    ]));
    let TextLayer::Readable(root) = &doc.pages()[0].text else {
        panic!("readable")
    };
    assert_eq!(root.children[0].text, "a");
    assert_eq!(root.children[1].text, "b");
    assert_eq!(root.children[0].rect.y, 10.0);
    assert_eq!(root.children[1].rect.y, 30.0);
}
#[test]
fn missing_text_and_text_without_zones_are_distinct() {
    assert_eq!(
        open(&single(&[chunk(b"INFO", &info(100, 100, 300))])).pages()[0].text,
        TextLayer::Empty
    );
    let d = open(&single(&[
        chunk(b"INFO", &info(100, 100, 300)),
        chunk(b"TXTa", &[0, 0, 1, b'a']),
    ]));
    assert!(matches!(&d.pages()[0].text,TextLayer::Unusable {text,..} if text=="a"));
}
#[test]
fn info_dpi_is_little_endian_and_rotation_zero_is_flag_one() {
    let d = open(&single(&[chunk(b"INFO", &info(100, 200, 300))]));
    assert_eq!(d.pages()[0].info.dpi, 300);
    assert_eq!(d.pages()[0].info.rotation, 0);
    for (flag, rotation) in [(1, 0), (6, 90), (2, 180), (5, 270)] {
        let mut data = info(100, 200, 300);
        data[9] = flag;
        let d = open(&single(&[chunk(b"INFO", &data)]));
        assert_eq!(d.pages()[0].info.rotation, rotation);
    }
}
#[test]
fn absolute_padding_and_unknown_chunks_do_not_desynchronize_text() {
    let bytes = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"Xxxx", &[1]),
        chunk(b"TXTa", &text("ok")),
    ]);
    let d = open(&bytes);
    assert!(matches!(d.pages()[0].text, TextLayer::Readable(_)));
    assert!(d.pages()[0]
        .warnings
        .iter()
        .any(|s| s.contains("1 unknown")));
}
#[test]
fn fragmented_text_chunks_are_accumulated_before_parsing() {
    let data = text("fragmented");
    let d = open(&single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &data[..7]),
        chunk(b"TXTz", &djvu_bzz::bzz_encode(&data[7..])),
    ]));
    assert!(matches!(&d.pages()[0].text,TextLayer::Readable(z) if z.text=="fragmented"));
}
#[test]
fn bundled_pages_follow_dirm_order() {
    let a = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("first")),
        ],
    );
    let b = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(200, 100, 72)),
            chunk(b"TXTz", &djvu_bzz::bzz_encode(&text("second"))),
        ],
    );
    let d = open(&bundled(&[a, b]));
    assert_eq!(d.pages().len(), 2);
    assert_eq!(d.pages()[1].info.width, 200);
    assert_eq!(d.pages()[0].id, "p0.djvu");
}

#[test]
fn annotations_are_read_per_page() {
    let first = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("first")),
            ant("(maparea \"\" (rect 0 0 100 100) (url \"#page=2\"))"),
        ],
    );
    let second = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("second")),
        ],
    );
    let d = open(&bundled(&[first, second]));
    assert_eq!(d.pages()[0].annotations.len(), 1);
    assert_eq!(
        d.pages()[0].annotations[0].target,
        strict_ooxml_djvu::AnnotationTarget::Page("page=2".to_owned())
    );
    assert!(d.pages()[1].annotations.is_empty());
}
#[test]
fn indirect_resolver_and_shared_includes_are_explicit() {
    let page = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"INCL", b"shared.djvu"),
        chunk(b"TXTa", &text("hello")),
    ]);
    let shared = file(&form(b"DJVI", &[chunk(b"ANTa", b"(background #ffffff)")]));
    let index = file(&form(
        b"DJVM",
        &[directory(
            &[
                ("page.djvu", 1, 0, page.len()),
                ("shared.djvu", 0, 0, shared.len()),
            ],
            false,
        )],
    ));
    assert!(matches!(
        DjvuDocument::open(&index, DjvuLimits::default()),
        Err(DjvuError::Component(_))
    ));
    let d = DjvuDocument::open_with_resolver(&index, DjvuLimits::default(), |id| match id {
        "page.djvu" => Ok(page.clone()),
        "shared.djvu" => Ok(shared.clone()),
        _ => Err(DjvuError::Component(id.into())),
    })
    .expect("resolved");
    assert_eq!(d.pages()[0].chunks[1].id, *b"ANTa");
    assert!(matches!(d.pages()[0].text, TextLayer::Readable(_)));
}
#[test]
fn unsafe_indirect_id_is_rejected_before_the_resolver_runs() {
    let index = file(&form(
        b"DJVM",
        &[directory(&[("../escape", 1, 0, 0)], false)],
    ));
    let result = DjvuDocument::open_with_resolver(&index, DjvuLimits::default(), |_| {
        panic!("must not call resolver")
    });
    assert!(result.is_err());
}
#[test]
fn cyclic_includes_are_rejected() {
    let page = single(&[chunk(b"INFO", &info(100, 100, 72)), chunk(b"INCL", b"a")]);
    let shared = file(&form(b"DJVI", &[chunk(b"INCL", b"a")]));
    let index = file(&form(
        b"DJVM",
        &[directory(
            &[("p", 1, 0, page.len()), ("a", 0, 0, shared.len())],
            false,
        )],
    ));
    let result = DjvuDocument::open_with_resolver(&index, DjvuLimits::default(), |id| {
        Ok(if id == "p" {
            page.clone()
        } else {
            shared.clone()
        })
    });
    assert!(result.expect_err("cycle").to_string().contains("cyclic"));
}
#[test]
fn budgets_refuse_document_chunk_text_tree_and_bzz_block_work() {
    let bytes = text_file("limits", true);
    let default = DjvuLimits::default();
    for limits in [
        DjvuLimits {
            max_file_bytes: 8,
            ..default
        },
        DjvuLimits {
            max_chunks: 1,
            ..default
        },
        DjvuLimits {
            max_chunk_bytes: 5,
            ..default
        },
        DjvuLimits {
            max_text_bytes: 5,
            ..default
        },
        DjvuLimits {
            max_zones: 1,
            ..default
        },
        DjvuLimits {
            max_zone_depth: 0,
            ..default
        },
        DjvuLimits {
            max_bzz_block: 5,
            ..default
        },
        DjvuLimits {
            max_pages: 0,
            ..default
        },
    ] {
        assert!(DjvuDocument::open(&bytes, limits).is_err());
    }
}
#[test]
fn invalid_utf8_boundary_is_unusable_and_retains_text() {
    let mut txt = text("é");
    let last = txt.len() - 17;
    txt[last + 9..last + 11].copy_from_slice(&0x8001u16.to_be_bytes());
    txt[last + 11..last + 14].copy_from_slice(&u24(1));
    let d = open(&single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &txt),
    ]));
    assert!(matches!(&d.pages()[0].text,TextLayer::Unusable {text,..} if text=="é"));
}
#[test]
fn short_info_compatibility_and_forward_version_refusal() {
    for n in [5, 6, 8, 9, 10] {
        let data = info(100, 100, 300);
        assert!(DjvuDocument::open(
            &single(&[chunk(b"INFO", &data[..n])]),
            DjvuLimits::default()
        )
        .is_ok());
    }
    let mut data = info(100, 100, 300);
    data[4] = 50;
    assert!(DjvuDocument::open(&single(&[chunk(b"INFO", &data)]), DjvuLimits::default()).is_err());
}
proptest::proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..4096)) {
        let _=DjvuDocument::open(&bytes,DjvuLimits {max_file_bytes:4096,max_chunks:128,max_text_bytes:4096,max_bzz_block:4096,..DjvuLimits::default()});
    }
    #[test]
    fn mutated_native_text_does_not_panic(index in 0usize..150,value in proptest::prelude::any::<u8>()) {
        let mut bytes=text_file("Привет hostile",true);let n=index%bytes.len();bytes[n]=value;let _=DjvuDocument::open(&bytes,DjvuLimits {max_chunks:128,max_text_bytes:4096,max_bzz_block:4096,..DjvuLimits::default()});
    }
}

#[test]
fn cumulative_zone_text_is_budgeted_before_copying() {
    assert!(DjvuDocument::open(
        &text_file("repeated", true),
        DjvuLimits {
            max_zone_text_bytes: 8,
            ..DjvuLimits::default()
        }
    )
    .is_err());
}

#[test]
fn unicode_component_ids_are_safe_identifiers() {
    let page = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &text("Olá")),
    ]);
    let index = file(&form(
        b"DJVM",
        &[directory(&[("civilisação.djvu", 1, 0, page.len())], false)],
    ));
    let doc = DjvuDocument::open_with_resolver(&index, DjvuLimits::default(), |id| {
        assert_eq!(id, "civilisação.djvu");
        Ok(page.clone())
    })
    .expect("safe Unicode id");
    assert_eq!(doc.pages()[0].id, "civilisação.djvu");
}

#[test]
fn the_navm_outline_is_read_as_a_forest() {
    let page = |label: &str| {
        form(
            b"DJVU",
            &[
                chunk(b"INFO", &info(100, 100, 72)),
                chunk(b"TXTa", &text(label)),
            ],
        )
    };
    // DjVuLibre layout: "Chapter" (one child) then "Section", pre-order.
    let outline = navm(&[(1, "Chapter", "#page=1"), (0, "Section", "#page=2")]);
    let doc = open(&bundled_with(&[page("one"), page("two")], &[outline]));
    assert_eq!(doc.outline.len(), 1, "one root entry");
    assert_eq!(doc.outline[0].title, "Chapter");
    assert_eq!(doc.outline[0].url, "#page=1");
    assert_eq!(doc.outline[0].children.len(), 1);
    assert_eq!(doc.outline[0].children[0].title, "Section");
    assert_eq!(doc.outline[0].children[0].url, "#page=2");
}

#[test]
fn a_broken_navm_is_a_warning_not_an_error() {
    let page = form(
        b"DJVU",
        &[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"TXTa", &text("one")),
        ],
    );
    // A `NAVM` whose payload is not BZZ.
    let broken = chunk(b"NAVM", b"not bzz at all");
    let doc = DjvuDocument::open(&bundled_with(&[page], &[broken]), DjvuLimits::default())
        .expect("the outline never fails the read");
    assert!(doc.outline.is_empty());
    assert!(
        doc.warnings.iter().any(|w| w.contains("NAVM")),
        "{:?}",
        doc.warnings
    );
}

#[test]
fn xmp_fills_properties_the_metadata_block_omits() {
    let page = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &text("body")),
        ant("(metadata (Title \"Native\")) \
             (xmp \"<rdf:RDF xmlns:rdf='x'><dc:title>From XMP</dc:title>\
             <dc:creator>Ada Lovelace</dc:creator></rdf:RDF>\")"),
    ]);
    let doc = open(&page);
    // The native `(metadata …)` key wins; XMP is the fallback.
    assert_eq!(doc.metadata.get("Title"), Some("Native"));
    assert_eq!(doc.metadata.get("Author"), Some("Ada Lovelace"));
}
