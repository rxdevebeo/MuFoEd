//! Compound operations must demonstrate behavior beyond an API scaffold.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Address, EditLimits, Editor, OperationLimits, Operations, ReplacePolicy, SearchQuery,
    SearchScope, Story, TextQuery,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{model::Document, parse_document, ParseOptions};
fn doc(body: &str) -> Document {
    let bytes = DocxBuilder::strict().body(body).build();
    let p = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    parse_document(&p, &ParseOptions::default()).unwrap()
}
fn text(d: &Document) -> Vec<String> {
    d.body
        .blocks
        .iter()
        .filter_map(|b| b.as_paragraph())
        .map(|p| {
            p.inlines
                .iter()
                .filter_map(|i| {
                    if let strict_ooxml_wml::model::Inline::Run(r) = i {
                        Some(r)
                    } else {
                        None
                    }
                })
                .flat_map(|r| &r.content)
                .filter_map(|c| {
                    if let strict_ooxml_wml::model::RunContent::Text(t) = c {
                        Some(t.text.clone())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}
#[test]
fn search_across_runs_and_unicode_boundaries() {
    let mut d =
        doc("<w:p><w:r><w:t>Пр</w:t></w:r><w:r><w:t>ивет приветик ПРИВЕТ</w:t></w:r></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let ops = Operations::new(&mut e, OperationLimits::default());
    let q = TextQuery {
        text: "привет".into(),
        case_sensitive: false,
        whole_word: true,
    };
    let result = ops
        .search(0, &SearchScope::All, &SearchQuery::Text(q))
        .unwrap();
    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.hits[0].range, Some(0..6));
    assert_eq!(result.hits[0].slices.len(), 2);
    assert_eq!(e.document().body, original);
}
#[test]
fn replacement_is_one_atomic_transaction_and_exact_undo() {
    let mut d=doc("<w:p><w:r><w:t>ab</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>cd abcd</w:t></w:r></w:p><w:p><w:r><w:t>abcd</w:t></w:r></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("abcd"),
            "X",
            ReplacePolicy::Strict,
        )
        .unwrap();
    assert_eq!(report.replaced, 3);
    assert_eq!(text(e.document()), vec!["X X", "X"]);
    assert_eq!(report.change.revision, 1);
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn moved_block_changes_order_reports_identity_and_undo() {
    let mut d=doc("<w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_block(0, &Address::body(0), &Address::body(3))
        .unwrap();
    assert_eq!(text(e.document()), vec!["b", "c", "a"]);
    assert_eq!(report.destination, Address::body(2));
    // The move keeps identities; this paragraph has none to keep.
    assert_eq!(
        report.identities,
        [IdentityChange {
            before: None,
            after: None
        }]
    );
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn moved_row_changes_order_and_undo() {
    let mut d=doc("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_row(0, &Address::body(0), 0, 2)
        .unwrap();
    assert_eq!(report.identities.len(), 1);
    let addresses = e.paragraphs(&Story::Body).unwrap();
    let p = e.paragraph(&addresses[0]).unwrap();
    let strict_ooxml_wml::model::Inline::Run(run) = &p.inlines[0] else {
        panic!()
    };
    assert!(
        matches!(&run.content[0], strict_ooxml_wml::model::RunContent::Text(t) if t.text == "b")
    );
    assert_eq!(report.row, Some(1));
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
use strict_ooxml_edit::{Container, Edit, EditError, IdentityChange, Invariant, OperationError};
#[test]
fn lowercase_expansion_never_splits_an_original_scalar() {
    let mut d = doc("<w:p><w:r><w:t>İ i X🙂</w:t></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let ops = Operations::new(&mut e, OperationLimits::default());
    let q = TextQuery {
        text: "i".into(),
        case_sensitive: false,
        whole_word: false,
    };
    let hits = ops
        .search(0, &SearchScope::All, &SearchQuery::Text(q))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, Some(2..3));
    let q = TextQuery {
        text: "i\u{307}".into(),
        case_sensitive: false,
        whole_word: false,
    };
    let hits = ops
        .search(0, &SearchScope::All, &SearchQuery::Text(q))
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, Some(0..1));
}
#[test]
fn wrapper_replacement_preserves_hyperlink_properties_and_tab() {
    let mut d=doc("<w:p><w:r><w:t>a</w:t></w:r><w:hyperlink w:anchor=\"target\"><w:r><w:rPr><w:i/></w:rPr><w:t>bc</w:t><w:tab/><w:t>tail</w:t></w:r></w:hyperlink></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("abc"),
            "X",
            ReplacePolicy::Strict,
        )
        .unwrap();
    assert_eq!(report.replaced, 1);
    let strict_ooxml_wml::model::Inline::Hyperlink(link) =
        &e.paragraph(&Address::body(0)).unwrap().inlines[1]
    else {
        panic!()
    };
    assert_eq!(link.anchor.as_deref(), Some("target"));
    let strict_ooxml_wml::model::Inline::Run(run) = &link.inlines[0] else {
        panic!()
    };
    assert_eq!(run.props.italic, strict_ooxml_wml::model::TriState::On);
    assert!(matches!(
        run.content[1],
        strict_ooxml_wml::model::RunContent::Tab
    ));
    let hits = Operations::new(&mut e, OperationLimits::default())
        .search(
            1,
            &SearchScope::All,
            &SearchQuery::Text(TextQuery::new("X\ttail")),
        )
        .unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert!(!hits.hits[0].editable);
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn protected_field_refuses_all_or_is_explicitly_skipped() {
    let mut d=doc("<w:p><w:r><w:t>hit</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>hit</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let mut ops = Operations::new(&mut e, OperationLimits::default());
    assert!(matches!(
        ops.replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("hit"),
            "new",
            ReplacePolicy::Strict
        ),
        Err(OperationError::ProtectedContent)
    ));
    let field = ops
        .search(0, &SearchScope::All, &SearchQuery::HasComplexField)
        .unwrap();
    assert_eq!(field.hits.len(), 1);
    let report = ops
        .replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("hit"),
            "new",
            ReplacePolicy::EditableOnly,
        )
        .unwrap();
    assert_eq!(report.matched, 2);
    assert_eq!(report.replaced, 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].paragraph, Address::body(1));
    assert_eq!(text(e.document()), vec!["new", "hit"]);
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn all_stories_and_nested_containers_replace_and_undo_together() {
    use strict_ooxml_core::{error::SourceLocation, part::PartId};
    use strict_ooxml_wml::model::{HeaderFooter, Note, NoteKind};
    let mut d=doc("<w:p><w:r><w:t>hit</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>hit</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sdt><w:sdtContent><w:p><w:r><w:t>hit</w:t></w:r></w:p></w:sdtContent></w:sdt>");
    let blocks = doc("<w:p><w:r><w:t>hit</w:t></w:r></w:p>").body.blocks;
    for (name, header) in [("header1.xml", true), ("footer1.xml", false)] {
        d.headers_footers.push(HeaderFooter {
            part: PartId::new(format!("/word/{name}")),
            is_header: header,
            blocks: blocks.clone(),
            location: SourceLocation::unknown(),
        });
    }
    let note = Note {
        id: 1,
        kind: NoteKind::Normal,
        blocks: blocks.clone(),
        location: SourceLocation::unknown(),
    };
    d.footnotes.insert(note.clone());
    d.endnotes.insert(note);
    let original = format!("{d:?}");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("hit"),
            "done",
            ReplacePolicy::Strict,
        )
        .unwrap();
    assert_eq!(report.replaced, 7);
    let hits = Operations::new(&mut e, OperationLimits::default())
        .search(
            1,
            &SearchScope::All,
            &SearchQuery::Text(TextQuery::new("done")),
        )
        .unwrap();
    assert_eq!(hits.hits.len(), 7);
    e.undo(1).unwrap();
    assert_eq!(format!("{:?}", e.document()), original);
}
#[test]
fn metadata_queries_use_direct_style_and_exact_location() {
    use strict_ooxml_wml::model::StyleId;
    let bytes=DocxBuilder::strict().body("<w:p><w:pPr><w:pStyle w:val=\"P\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"C\"/></w:rPr><w:t>value</w:t></w:r></w:p>").part("word/styles.xml",br#"<w:styles xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:style w:type="paragraph" w:styleId="P"/><w:style w:type="character" w:styleId="C"/></w:styles>"#.to_vec()).rel("styles","styles","styles.xml").build();
    let p = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&p, &ParseOptions::default()).unwrap();
    let location = d.body.blocks[0].as_paragraph().unwrap().location.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let ops = Operations::new(&mut e, OperationLimits::default());
    for q in [
        SearchQuery::ParagraphStyle(StyleId::new("P")),
        SearchQuery::CharacterStyle(StyleId::new("C")),
        SearchQuery::Location(location),
    ] {
        let hits = ops
            .search(0, &SearchScope::Paragraph(Address::body(0)), &q)
            .unwrap();
        assert_eq!(hits.hits.len(), 1);
        assert!(hits.hits[0].range.is_none());
    }
    assert!(ops
        .search(
            0,
            &SearchScope::All,
            &SearchQuery::CharacterStyle(StyleId::new("absent"))
        )
        .unwrap()
        .hits
        .is_empty());
}
#[test]
fn stale_limits_noop_and_redo_are_preserved() {
    let mut d = doc("<w:p><w:r><w:t>a a</w:t></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Text {
            at: Address::body(0),
            range: 0..1,
            text: "b".into(),
        }],
    )
    .unwrap();
    e.undo(1).unwrap();
    let mut ops = Operations::new(&mut e, OperationLimits::default());
    assert!(matches!(
        ops.search(
            0,
            &SearchScope::All,
            &SearchQuery::Text(TextQuery::new("a"))
        ),
        Err(OperationError::Edit(EditError::StaleRevision))
    ));
    let report = ops
        .replace_all(
            2,
            &SearchScope::All,
            &TextQuery::new("a"),
            "a",
            ReplacePolicy::Strict,
        )
        .unwrap();
    assert_eq!(report.replaced, 0);
    assert_eq!(report.change.revision, 2);
    let report = ops
        .move_block(2, &Address::body(0), &Address::body(1))
        .unwrap();
    assert_eq!(report.change.revision, 2);
    assert_eq!(report.identities[0].before, report.identities[0].after);
    e.redo(2).unwrap();
    assert_eq!(text(e.document()), vec!["b a"]);
    let original = e.document().body.clone();
    for limits in [
        OperationLimits {
            matches: 0,
            ..OperationLimits::default()
        },
        OperationLimits {
            scalars: 0,
            ..OperationLimits::default()
        },
        OperationLimits {
            work: 0,
            ..OperationLimits::default()
        },
        OperationLimits {
            commands: 0,
            ..OperationLimits::default()
        },
    ] {
        let result = Operations::new(&mut e, limits).replace_all(
            3,
            &SearchScope::All,
            &TextQuery::new("a"),
            "new",
            ReplacePolicy::Strict,
        );
        assert!(matches!(result, Err(OperationError::LimitExceeded)));
        assert_eq!(e.document().body, original);
    }
    assert!(matches!(
        Operations::new(&mut e, OperationLimits::default()).search(
            3,
            &SearchScope::All,
            &SearchQuery::Text(TextQuery::new(""))
        ),
        Err(OperationError::InvalidQuery)
    ));
}
#[test]
fn nested_block_move_retains_contents_and_reports_fresh_ids() {
    let mut d=doc("<w:sdt><w:sdtContent><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:sdtContent></w:sdt>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let source = Address {
        story: Story::Body,
        containers: vec![Container::Sdt(0)],
        block: 1,
    };
    e.transact(0, &[Edit::Identify { at: source.clone() }])
        .unwrap();
    let id = e.paragraph(&source).unwrap().para_id.clone();
    let mut dest = source.clone();
    dest.block = 0;
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_block(1, &source, &dest)
        .unwrap();
    assert_eq!(report.identities[0].before, id);
    assert_eq!(report.identities[0].after, id, "a move keeps the identity");
    assert_eq!(
        e.find(&Story::Body, id.as_ref().unwrap()).unwrap(),
        dest,
        "and the paragraph is found where it went"
    );
    let p = e.paragraph(&dest).unwrap();
    let strict_ooxml_wml::model::Inline::Run(run) = &p.inlines[0] else {
        panic!()
    };
    assert!(
        matches!(&run.content[0], strict_ooxml_wml::model::RunContent::Text(t) if t.text == "b")
    );
    e.undo(2).unwrap();
    e.undo(3).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn a_block_moves_out_of_a_container_keeping_its_identity() {
    let mut d=doc("<w:p><w:r><w:t>a</w:t></w:r></w:p><w:sdt><w:sdtContent><w:p><w:r><w:t>b</w:t></w:r></w:p><w:p><w:r><w:t>c</w:t></w:r></w:p></w:sdtContent></w:sdt>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let source = Address {
        story: Story::Body,
        containers: vec![Container::Sdt(1)],
        block: 1,
    };
    e.transact(0, &[Edit::Identify { at: source.clone() }])
        .unwrap();
    let id = e.paragraph(&source).unwrap().para_id.clone();
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_block(1, &source, &Address::body(0))
        .unwrap();
    assert_eq!(report.destination, Address::body(0));
    assert_eq!(text(e.document()), vec!["c", "a"]);
    assert_eq!(
        report.identities,
        [IdentityChange {
            before: id.clone(),
            after: id
        }]
    );
    e.undo(2).unwrap();
    e.undo(3).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn illegal_moves_are_atomic_and_preserve_redo() {
    let mut d=doc("<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"10000\"/></w:sectPr></w:pPr><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p><w:sectPr><w:pgSz w:w=\"12000\"/></w:sectPr>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Text {
            at: Address::body(1),
            range: 0..1,
            text: "c".into(),
        }],
    )
    .unwrap();
    e.undo(1).unwrap();
    let result = Operations::new(&mut e, OperationLimits::default()).move_block(
        2,
        &Address::body(0),
        &Address::body(2),
    );
    assert!(matches!(
        result,
        Err(OperationError::Edit(EditError::InvalidModel(
            Invariant::SectionBoundary
        )))
    ));
    assert_eq!(e.document().body, original);
    let dest = Address {
        story: Story::Footnote(7),
        containers: vec![],
        block: 0,
    };
    assert!(matches!(
        Operations::new(&mut e, OperationLimits::default()).move_block(2, &Address::body(0), &dest),
        Err(OperationError::UnsupportedMove)
    ));
    assert!(matches!(
        Operations::new(&mut e, OperationLimits::default()).move_block(
            2,
            &Address::body(1),
            &Address::body(9)
        ),
        Err(OperationError::Edit(EditError::InvalidParagraph))
    ));
    e.redo(2).unwrap();
    assert_eq!(text(e.document()), vec!["a", "c"]);
}
#[test]
fn replacement_survives_write_and_reopen() {
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>a🙂a</w:t></w:r></w:p>")
        .build();
    let source = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let mut d = parse_document(&source, &ParseOptions::default()).unwrap();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let report = Operations::new(&mut e, OperationLimits::default())
        .replace_all(
            0,
            &SearchScope::All,
            &TextQuery::new("a"),
            "Б ",
            ReplacePolicy::Strict,
        )
        .unwrap();
    assert_eq!(report.replaced, 2);
    let out = strict_ooxml_write::write_package(
        e.document(),
        Some(&source),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .unwrap();
    let p = Package::open_reader(&out.bytes[..], &OpenOptions::default()).unwrap();
    let reopened = parse_document(&p, &ParseOptions::default()).unwrap();
    assert_eq!(text(&reopened), vec!["Б 🙂Б "]);
}
#[test]
fn cell_move_respects_final_paragraph_and_row_merge_rolls_back() {
    let mut d=doc("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let a = Address {
        story: Story::Body,
        containers: vec![Container::Cell {
            table: 0,
            row: 0,
            cell: 0,
        }],
        block: 0,
    };
    let b = Address {
        block: 2,
        ..a.clone()
    };
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_block(0, &a, &b)
        .unwrap();
    let found = Operations::new(&mut e, OperationLimits::default())
        .search(
            1,
            &SearchScope::Paragraph(report.destination.clone()),
            &SearchQuery::Text(TextQuery::new("a")),
        )
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    assert_eq!(report.destination.block, 1);
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
    let mut d=doc("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p/></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    assert!(matches!(
        Operations::new(&mut e, OperationLimits::default()).move_row(0, &Address::body(0), 0, 2),
        Err(OperationError::Edit(EditError::InvalidModel(
            Invariant::TableTopology
        )))
    ));
    assert_eq!(e.document().body, original);
    assert_eq!(e.revision(), 0);
}
#[test]
#[allow(clippy::too_many_lines)]
fn shape_textbox_search_replace_and_move_stay_in_scope() {
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_wml::model::*;
    let mut d = doc("<w:p><w:r><w:t>outside</w:t></w:r></w:p>");
    let nested =
        doc("<w:p><w:r><w:t>inside</w:t></w:r></w:p><w:p><w:r><w:t>tail</w:t></w:r></w:p>")
            .body
            .blocks;
    let location = SourceLocation::unknown();
    let drawing = Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(Extent {
                cx: Emu(914_400),
                cy: Emu(914_400),
            }),
            effect_extent: None,
            doc_pr: None,
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            graphic_uri: None,
            graphic: Box::new(Graphic::Shape(Shape {
                name: None,
                descr: None,
                nv_id: None,
                bw_mode: None,
                tx_box: None,
                sp_locks: None,
                connector: false,
                effects: None,
                geometry: ShapeGeometry::Preset("rect".into()),
                xfrm: None,
                offset: None,
                extent: None,
                fill: None,
                stroke: None,
                text: Some(TextBox {
                    body: None,
                    blocks: nested,
                    location: location.clone(),
                }),
                style: None,
                location: location.clone(),
            })),
            location: location.clone(),
        }),
        location,
    };
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::InsertInline {
            at: Address::body(0),
            index: 1,
            inline: Box::new(Inline::Drawing(drawing)),
        }],
    )
    .unwrap();
    let original = e.document().body.clone();
    let at = Address {
        story: Story::Body,
        containers: vec![Container::TextBox {
            paragraph: 0,
            inline: vec![1],
            content: None,
            graphics: vec![],
        }],
        block: 0,
    };
    let found = Operations::new(&mut e, OperationLimits::default())
        .search(
            1,
            &SearchScope::Story(Story::Body),
            &SearchQuery::Text(TextQuery::new("inside")),
        )
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].paragraph, at);
    Operations::new(&mut e, OperationLimits::default())
        .replace_all(
            1,
            &SearchScope::Paragraph(at.clone()),
            &TextQuery::new("inside"),
            "changed",
            ReplacePolicy::Strict,
        )
        .unwrap();
    let dest = Address {
        block: 2,
        ..at.clone()
    };
    let report = Operations::new(&mut e, OperationLimits::default())
        .move_block(2, &at, &dest)
        .unwrap();
    assert_eq!(report.destination.block, 1);
    assert_eq!(text(e.document()), vec!["outside"]);
    let found = Operations::new(&mut e, OperationLimits::default())
        .search(
            3,
            &SearchScope::Paragraph(report.destination),
            &SearchQuery::Text(TextQuery::new("changed")),
        )
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    e.undo(3).unwrap();
    e.undo(4).unwrap();
    assert_eq!(e.document().body, original);
}

/// A paragraph with an insertion and a deletion, then a paragraph whose mark
/// was deleted, then the centred paragraph its text would join.
const TRACKED: &str = concat!(
    "<w:p><w:r><w:t xml:space=\"preserve\">keep </w:t></w:r>",
    "<w:ins w:id=\"1\" w:author=\"A\"><w:r><w:t>new</w:t></w:r></w:ins>",
    "<w:del w:id=\"2\" w:author=\"A\"><w:r><w:delText>old</w:delText></w:r></w:del></w:p>",
    "<w:p><w:pPr><w:rPr><w:del w:id=\"3\" w:author=\"A\"/></w:rPr></w:pPr>",
    "<w:r><w:t>joined</w:t></w:r></w:p>",
    "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>",
    "<w:r><w:t xml:space=\"preserve\"> next</w:t></w:r></w:p>",
);

fn no_revisions(d: &Document) -> bool {
    d.body
        .blocks
        .iter()
        .filter_map(|b| b.as_paragraph())
        .all(|p| {
            p.revision.is_none()
                && p.inlines.iter().all(|i| match i {
                    strict_ooxml_wml::model::Inline::Run(r) => r.revision.is_none(),
                    _ => true,
                })
        })
}

#[test]
fn accepting_every_change_keeps_insertions_and_joins_a_deleted_mark() {
    let mut d = doc(TRACKED);
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.accept_all(0).unwrap();
    assert_eq!(text(e.document()), ["keep new", "joined next"]);
    assert!(no_revisions(e.document()));
    let joined = e.document().body.blocks[1].as_paragraph().unwrap();
    assert!(
        joined.props.alignment.is_some(),
        "the surviving mark is the next paragraph's"
    );
    e.undo(1).unwrap();
    assert_eq!(e.document().body, before);
}

#[test]
fn rejecting_every_change_keeps_deletions_and_the_marks() {
    let mut d = doc(TRACKED);
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.reject_all(0).unwrap();
    assert_eq!(text(e.document()), ["keep old", "joined", " next"]);
    assert!(no_revisions(e.document()));
}

#[test]
fn accepting_one_paragraph_leaves_the_others_tracked() {
    let mut d = doc(TRACKED);
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[strict_ooxml_edit::Edit::AcceptRevisions {
            at: Address::body(0),
        }],
    )
    .unwrap();
    assert_eq!(text(e.document()), ["keep new", "joined", " next"]);
    let second = e.document().body.blocks[1].as_paragraph().unwrap();
    assert!(
        second.revision.is_some(),
        "only the addressed paragraph changes"
    );
}

fn tracked(at: usize, range: std::ops::Range<usize>, text: &str) -> strict_ooxml_edit::Edit {
    strict_ooxml_edit::Edit::TrackedText {
        at: Address::body(at),
        range,
        text: text.into(),
        author: "Reviewer".into(),
        date: Some("2026-10-08T12:00:00Z".into()),
    }
}

/// Every run's text with its tracked kind: `+` inserted, `-` deleted.
fn marked(d: &Document) -> Vec<String> {
    let p = d.body.blocks[0].as_paragraph().unwrap();
    p.inlines
        .iter()
        .filter_map(|i| match i {
            strict_ooxml_wml::model::Inline::Run(r) => Some(r),
            _ => None,
        })
        .map(|r| {
            let text: String = r
                .content
                .iter()
                .filter_map(|c| match c {
                    strict_ooxml_wml::model::RunContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect();
            let mark = match r.revision.as_ref().map(|v| v.kind) {
                Some(strict_ooxml_wml::model::RevisionKind::Insert) => "+",
                Some(strict_ooxml_wml::model::RevisionKind::Delete) => "-",
                _ => "",
            };
            format!("{mark}{text}")
        })
        .collect()
}

#[test]
fn tracked_text_keeps_the_old_text_deleted_and_extends_its_own_insertion() {
    let mut d = doc("<w:p><w:r><w:t>hello world</w:t></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(0, &[tracked(0, 6..11, "there")]).unwrap();
    assert_eq!(marked(e.document()), ["hello ", "-world", "+there"]);
    // Typing at the end of the insertion extends it; deleting inside it takes
    // the inserted text back instead of marking it deleted.
    e.transact(1, &[tracked(0, 16..16, "!")]).unwrap();
    e.transact(2, &[tracked(0, 11..12, "")]).unwrap();
    assert_eq!(marked(e.document()), ["hello ", "-world", "+here!"]);
    let p = e.document().body.blocks[0].as_paragraph().unwrap();
    let ids: Vec<u32> = p
        .inlines
        .iter()
        .filter_map(|i| match i {
            strict_ooxml_wml::model::Inline::Run(r) => r.revision.as_ref(),
            _ => None,
        })
        .map(|v| {
            assert_eq!(v.author.as_deref(), Some("Reviewer"));
            v.id
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(
        ids[0], ids[1],
        "the deletion and the insertion are two changes"
    );

    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.accept_all(3).unwrap();
    assert_eq!(text(e.document()), ["hello here!"]);
    e.undo(4).unwrap();
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.reject_all(5).unwrap();
    assert_eq!(text(e.document()), ["hello world"]);
}

/// The `w:id`s of the tracked changes left in the body, marks included.
fn change_ids(d: &Document) -> Vec<u32> {
    let mut ids: Vec<u32> = d
        .body
        .blocks
        .iter()
        .filter_map(|b| b.as_paragraph())
        .flat_map(|p| {
            p.revision
                .iter()
                .map(|r| r.id)
                .chain(p.inlines.iter().filter_map(|i| match i {
                    strict_ooxml_wml::model::Inline::Run(r) => r.revision.as_ref().map(|r| r.id),
                    _ => None,
                }))
        })
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn one_change_is_accepted_or_rejected_by_its_id_and_the_others_stay() {
    // The insertion alone: its text stays untracked, the deletion and the
    // deleted mark are still there.
    let mut d = doc(TRACKED);
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.accept_change(0, 1).unwrap();
    assert_eq!(text(e.document()), ["keep newold", "joined", " next"]);
    assert_eq!(change_ids(e.document()), [2, 3]);

    // The deleted mark alone joins its paragraph with the next one.
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.accept_change(1, 3).unwrap();
    assert_eq!(text(e.document()), ["keep newold", "joined next"]);
    assert_eq!(change_ids(e.document()), [2]);

    // Rejecting the deletion keeps its text, untracked.
    let mut o = Operations::new(&mut e, OperationLimits::default());
    o.reject_change(2, 2).unwrap();
    assert_eq!(text(e.document()), ["keep newold", "joined next"]);
    assert!(no_revisions(e.document()));

    // An id no change has changes nothing.
    let mut o = Operations::new(&mut e, OperationLimits::default());
    let change = o.accept_change(3, 99).unwrap();
    assert!(!change.invalidate_layout);
}

#[test]
fn a_single_change_command_leaves_the_other_runs_of_its_paragraph() {
    let mut d = doc(TRACKED);
    let before = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let change = e
        .transact(
            0,
            &[strict_ooxml_edit::Edit::RejectRevision {
                at: Address::body(0),
                id: 1,
            }],
        )
        .unwrap();
    assert_eq!(text(e.document()), ["keep old", "joined", " next"]);
    assert_eq!(change_ids(e.document()), [2, 3]);
    e.undo(change.revision).unwrap();
    assert_eq!(e.document().body, before);
}

#[test]
fn a_tracked_split_inserts_a_mark_that_accepting_keeps_and_rejecting_joins() {
    for (accept, expected) in [(true, vec!["abc", "def"]), (false, vec!["abcdef"])] {
        let mut d = doc("<w:p><w:r><w:t>abcdef</w:t></w:r></w:p>");
        let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
        e.transact(
            0,
            &[strict_ooxml_edit::Edit::TrackedSplit {
                at: Address::body(0),
                offset: 3,
                author: "Reviewer".into(),
                date: None,
            }],
        )
        .unwrap();
        assert_eq!(text(e.document()), ["abc", "def"]);
        let left = e.document().body.blocks[0].as_paragraph().unwrap();
        let mark = left.revision.as_ref().expect("tracked mark");
        assert_eq!(mark.kind, strict_ooxml_wml::model::RevisionKind::Insert);
        assert_eq!(mark.author.as_deref(), Some("Reviewer"));
        assert!(e.document().body.blocks[1]
            .as_paragraph()
            .unwrap()
            .revision
            .is_none());
        let mut o = Operations::new(&mut e, OperationLimits::default());
        if accept {
            o.accept_all(1).unwrap();
        } else {
            o.reject_all(1).unwrap();
        }
        assert_eq!(text(e.document()), expected);
        assert!(no_revisions(e.document()));
    }
}

#[test]
fn a_tracked_join_deletes_the_mark_and_accepting_joins() {
    for (accept, expected) in [(true, vec!["onetwo"]), (false, vec!["one", "two"])] {
        let mut d = doc("<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>");
        let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
        e.transact(
            0,
            &[strict_ooxml_edit::Edit::TrackedJoin {
                at: Address::body(0),
                author: "Reviewer".into(),
                date: None,
            }],
        )
        .unwrap();
        assert_eq!(text(e.document()), ["one", "two"], "nothing joins yet");
        let mark = e.document().body.blocks[0]
            .as_paragraph()
            .unwrap()
            .revision
            .clone()
            .expect("tracked mark");
        assert_eq!(mark.kind, strict_ooxml_wml::model::RevisionKind::Delete);
        let mut o = Operations::new(&mut e, OperationLimits::default());
        if accept {
            o.accept_all(1).unwrap();
        } else {
            o.reject_all(1).unwrap();
        }
        assert_eq!(text(e.document()), expected);
    }
}

#[test]
fn joining_over_an_inserted_mark_takes_it_back_at_once() {
    let mut d = doc("<w:p><w:r><w:t>abcdef</w:t></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let tracked = |edit| [edit];
    e.transact(
        0,
        &tracked(strict_ooxml_edit::Edit::TrackedSplit {
            at: Address::body(0),
            offset: 3,
            author: "Reviewer".into(),
            date: None,
        }),
    )
    .unwrap();
    e.transact(
        1,
        &tracked(strict_ooxml_edit::Edit::TrackedJoin {
            at: Address::body(0),
            author: "Reviewer".into(),
            date: None,
        }),
    )
    .unwrap();
    assert_eq!(text(e.document()), ["abcdef"]);
    assert!(no_revisions(e.document()));
}
