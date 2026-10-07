//! Structural editing regressions use independently generated documents.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{Address, Container, Edit, EditError, EditLimits, Editor, Story};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::{
    model::{Block, Document},
    parse_document, ParseOptions,
};
fn doc(body: &str) -> Document {
    let xml = format!("<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body>{body}</w:body></w:document>");
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    parse_document(&package, &ParseOptions::default()).unwrap()
}
fn paragraph() -> &'static str {
    "<w:p><w:r><w:t>А🙂Б</w:t></w:r></w:p>"
}
#[test]
fn split_join_exact_history_and_distinct_identity() {
    let mut d = doc(paragraph());
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Split {
            at: Address::body(0),
            offset: 2,
        }],
    )
    .unwrap();
    assert_eq!(e.document().body.blocks.len(), 2);
    assert_ne!(
        e.document().body.blocks[0].as_paragraph().unwrap().para_id,
        e.document().body.blocks[1].as_paragraph().unwrap().para_id
    );
    e.transact(
        1,
        &[Edit::Join {
            at: Address::body(0),
        }],
    )
    .unwrap();
    assert_eq!(e.document().body.blocks.len(), 1);
    e.undo(2).unwrap();
    assert_eq!(e.document().body.blocks.len(), 2);
    e.undo(3).unwrap();
    assert_eq!(e.document().body, original);
    e.redo(4).unwrap();
    assert_eq!(e.document().body.blocks.len(), 2);
}
#[test]
fn nested_cell_split_and_delete_keeps_final_paragraph() {
    let mut d = doc(&format!("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", paragraph()));
    let at = Address {
        story: Story::Body,
        containers: vec![Container::Cell {
            table: 0,
            row: 0,
            cell: 0,
        }],
        block: 0,
    };
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Split {
            at: at.clone(),
            offset: 1,
        }],
    )
    .unwrap();
    let Block::Table(t) = &e.document().body.blocks[0] else {
        panic!()
    };
    assert_eq!(t.rows[0].cells[0].blocks.len(), 2);
    e.transact(1, &[Edit::Delete { at: at.clone() }]).unwrap();
    assert_eq!(
        e.transact(2, &[Edit::Delete { at }]),
        Err(EditError::InvalidModel)
    );
}
#[test]
fn structural_batch_rolls_back_and_stale_revision_rejects() {
    let mut d = doc(paragraph());
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    assert_eq!(
        e.transact(
            0,
            &[
                Edit::Split {
                    at: Address::body(0),
                    offset: 1
                },
                Edit::Delete {
                    at: Address::body(99)
                }
            ]
        ),
        Err(EditError::InvalidParagraph)
    );
    assert_eq!(e.document().body, original);
    assert_eq!(e.undo(0), Err(EditError::EmptyHistory));
    e.transact(
        0,
        &[Edit::Split {
            at: Address::body(0),
            offset: 1,
        }],
    )
    .unwrap();
    assert_eq!(e.transact(0, &[]), Err(EditError::StaleRevision));
}
#[test]
fn insert_delete_and_new_branch_invalidate_redo() {
    let mut d = doc(paragraph());
    let block = d.body.blocks[0].clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Insert {
            at: Address::body(1),
            block: Box::new(block),
        }],
    )
    .unwrap();
    assert_eq!(e.document().body.blocks.len(), 2);
    e.undo(1).unwrap();
    e.transact(
        2,
        &[Edit::Delete {
            at: Address::body(0),
        }],
    )
    .unwrap();
    assert_eq!(e.redo(3), Err(EditError::EmptyHistory));
    assert!(!e.document().body.blocks.is_empty());
}

#[test]
fn section_boundary_moves_right_on_split_and_join_cannot_cross_it() {
    let mut d=doc("<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"10000\"/></w:sectPr></w:pPr><w:r><w:t>abc</w:t></w:r></w:p><w:p/><w:sectPr><w:pgSz w:w=\"12000\"/></w:sectPr>");
    let sections = d.sections.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[Edit::Split {
            at: Address::body(0),
            offset: 1,
        }],
    )
    .unwrap();
    assert!(e.document().body.blocks[0]
        .as_paragraph()
        .unwrap()
        .props
        .section
        .is_none());
    assert!(e.document().body.blocks[1]
        .as_paragraph()
        .unwrap()
        .props
        .section
        .is_some());
    assert_eq!(e.document().sections, sections);
    assert_eq!(
        e.transact(
            1,
            &[Edit::Join {
                at: Address::body(1)
            }]
        ),
        Err(EditError::InvalidModel)
    );
    e.transact(
        1,
        &[Edit::Join {
            at: Address::body(0),
        }],
    )
    .unwrap();
    assert_eq!(e.document().sections, sections);
}
#[test]
fn stable_identity_survives_insertions_and_sdt_addresses_work() {
    let mut d = doc(&format!(
        "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>{}",
        paragraph(),
        paragraph()
    ));
    let block = d.body.blocks[1].clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let at = Address {
        story: Story::Body,
        containers: vec![Container::Sdt(0)],
        block: 0,
    };
    e.transact(
        0,
        &[
            Edit::Identify { at: at.clone() },
            Edit::Text {
                at: at.clone(),
                range: 0..1,
                text: "Z".into(),
            },
        ],
    )
    .unwrap();
    let id = e.paragraph(&at).unwrap().para_id.clone().unwrap();
    e.transact(
        1,
        &[Edit::Insert {
            at: Address::body(0),
            block: Box::new(block),
        }],
    )
    .unwrap();
    let found = e.find(&Story::Body, &id).unwrap();
    assert_eq!(found.containers, vec![Container::Sdt(1)]);
    assert_eq!(e.paragraphs(&Story::Body).unwrap().len(), 3);
}
#[test]
fn header_and_note_edits_undo_together_without_touching_body() {
    use strict_ooxml_core::{error::SourceLocation, part::PartId};
    use strict_ooxml_wml::model::{HeaderFooter, Note, NoteKind};
    let mut d = doc(paragraph());
    let body = d.body.clone();
    let part = PartId::new("/word/header1.xml");
    d.headers_footers.push(HeaderFooter {
        part: part.clone(),
        is_header: true,
        blocks: body.blocks.clone(),
        location: SourceLocation::unknown(),
    });
    d.footnotes.insert(Note {
        id: 1,
        kind: NoteKind::Normal,
        blocks: body.blocks.clone(),
        location: SourceLocation::unknown(),
    });
    let h = Address {
        story: Story::HeaderFooter(part),
        containers: vec![],
        block: 0,
    };
    let n = Address {
        story: Story::Footnote(1),
        containers: vec![],
        block: 0,
    };
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[
            Edit::Split {
                at: h.clone(),
                offset: 1,
            },
            Edit::Text {
                at: n.clone(),
                range: 0..1,
                text: "X".into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(e.document().body, body);
    assert_eq!(e.document().headers_footers[0].blocks.len(), 2);
    assert_ne!(e.document().footnotes.get(1).unwrap().blocks, body.blocks);
    e.undo(1).unwrap();
    assert_eq!(e.document().headers_footers[0].blocks, body.blocks);
    assert_eq!(e.document().footnotes.get(1).unwrap().blocks, body.blocks);
}
#[test]
fn rich_text_node_keeps_hyperlink_and_tab_and_full_run_properties() {
    use strict_ooxml_wml::model::{HalfPoints, Inline, RunContent, RunProperties, TriState};
    let mut d=doc("<w:p><w:hyperlink w:anchor=\"target\"><w:r><w:t>abc</w:t><w:tab/><w:t>tail</w:t></w:r></w:hyperlink></w:p>");
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let properties = RunProperties {
        size: Some(HalfPoints(32)),
        bold: TriState::On,
        ..RunProperties::default()
    };
    e.transact(
        0,
        &[
            Edit::TextNode {
                at: Address::body(0),
                inline: vec![0, 0],
                content: 0,
                range: 1..2,
                text: "🙂".into(),
            },
            Edit::RunProperties {
                at: Address::body(0),
                inline: vec![0, 0],
                properties: Box::new(properties.clone()),
            },
        ],
    )
    .unwrap();
    let edited_paragraph = e.paragraph(&Address::body(0)).unwrap();
    let Inline::Hyperlink(hyperlink) = &edited_paragraph.inlines[0] else {
        panic!()
    };
    let Inline::Run(run) = &hyperlink.inlines[0] else {
        panic!()
    };
    assert_eq!(run.props, properties);
    assert_eq!(run.content[1], RunContent::Tab);
    assert_eq!(hyperlink.anchor.as_deref(), Some("target"));
    e.undo(1).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn dangling_boundaries_and_unknown_nested_style_are_atomic() {
    use strict_ooxml_wml::model::{Bookmark, Inline, RunProperties, StyleId};
    let mut d = doc(paragraph());
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    assert_eq!(
        e.transact(
            0,
            &[Edit::InsertInline {
                at: Address::body(0),
                index: 0,
                inline: Box::new(Inline::BookmarkStart(Bookmark::new("1", "x")))
            }]
        ),
        Err(EditError::InvalidModel)
    );
    assert_eq!(e.document().body, original);
    assert_eq!(
        e.transact(
            0,
            &[Edit::RunProperties {
                at: Address::body(0),
                inline: vec![0],
                properties: Box::new(RunProperties {
                    style: Some(StyleId::new("missing")),
                    ..RunProperties::default()
                })
            }]
        ),
        Err(EditError::UnknownStyle)
    );
    e.transact(
        0,
        &[
            Edit::InsertInline {
                at: Address::body(0),
                index: 0,
                inline: Box::new(Inline::BookmarkStart(Bookmark::new("1", "x"))),
            },
            Edit::InsertInline {
                at: Address::body(0),
                index: 2,
                inline: Box::new(Inline::BookmarkEnd(
                    strict_ooxml_wml::model::BookmarkId::new("1"),
                )),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        e.transact(
            1,
            &[Edit::DeleteInline {
                at: Address::body(0),
                index: 0
            }]
        ),
        Err(EditError::InvalidModel)
    );
}
#[test]
fn table_properties_grid_rows_and_merge_topology_validate_atomically() {
    use strict_ooxml_wml::model::{
        CellProperties, GridCol, TableProperties, Twips, VerticalMerge, Width,
    };
    let mut d=doc(&format!("<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",paragraph()));
    let original = d.body.clone();
    let Block::Table(t) = &d.body.blocks[0] else {
        panic!()
    };
    let row = t.rows[0].clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    let properties = TableProperties {
        width: Some(Width {
            value: Some(4000),
            kind: strict_ooxml_wml::model::WidthKind::Dxa,
        }),
        ..TableProperties::default()
    };
    e.transact(
        0,
        &[
            Edit::TableProperties {
                at: Address::body(0),
                properties: Box::new(properties),
            },
            Edit::Grid {
                at: Address::body(0),
                columns: vec![GridCol {
                    width: Some(Twips(4000)),
                }],
            },
            Edit::InsertRow {
                at: Address::body(0),
                index: 1,
                row: Box::new(row),
            },
        ],
    )
    .unwrap();
    let Block::Table(t) = &e.document().body.blocks[0] else {
        panic!()
    };
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.grid[0].width, Some(Twips(4000)));
    assert_eq!(
        e.transact(
            1,
            &[Edit::CellProperties {
                at: Address::body(0),
                row: 1,
                cell: 0,
                properties: Box::new(CellProperties {
                    vertical_merge: Some(VerticalMerge::Continue),
                    ..CellProperties::default()
                })
            }]
        ),
        Err(EditError::InvalidModel)
    );
    e.transact(
        1,
        &[
            Edit::CellProperties {
                at: Address::body(0),
                row: 0,
                cell: 0,
                properties: Box::new(CellProperties {
                    vertical_merge: Some(VerticalMerge::Restart),
                    ..CellProperties::default()
                }),
            },
            Edit::CellProperties {
                at: Address::body(0),
                row: 1,
                cell: 0,
                properties: Box::new(CellProperties {
                    vertical_merge: Some(VerticalMerge::Continue),
                    ..CellProperties::default()
                }),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        e.transact(
            2,
            &[Edit::DeleteRow {
                at: Address::body(0),
                index: 0
            }]
        ),
        Err(EditError::InvalidModel)
    );
    e.undo(2).unwrap();
    e.undo(3).unwrap();
    assert_eq!(e.document().body, original);
}
#[test]
fn frame_and_shape_textbox_edits_have_exact_history() {
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_wml::model::*;
    let mut d = doc(paragraph());
    let nested = d.body.blocks.clone();
    let before = d.body.clone();
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
    let frame = FrameProperties {
        width: Some(Twips(3000)),
        x: Some(400),
        ..FrameProperties::default()
    };
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    e.transact(
        0,
        &[
            Edit::InsertInline {
                at: Address::body(0),
                index: 1,
                inline: Box::new(Inline::Drawing(drawing)),
            },
            Edit::Frame {
                at: Address::body(0),
                frame: Some(frame.clone()),
            },
        ],
    )
    .unwrap();
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
    assert_eq!(e.paragraphs(&Story::Body).unwrap().len(), 2);
    e.transact(
        1,
        &[Edit::Split {
            at: at.clone(),
            offset: 1,
        }],
    )
    .unwrap();
    let id = e.paragraph(&at).unwrap().para_id.clone().unwrap();
    assert_eq!(e.find(&Story::Body, &id).unwrap(), at);
    assert_eq!(
        e.paragraph(&Address::body(0)).unwrap().props.frame,
        Some(frame)
    );
    e.undo(2).unwrap();
    e.undo(3).unwrap();
    assert_eq!(e.document().body, before);
}
#[test]
fn structured_noop_limits_and_numbering_location_identity() {
    let mut d = doc(paragraph());
    let original = d.body.clone();
    let mut e = Editor::new(&mut d, EditLimits::default())
        .unwrap()
        .with_history_byte_limit(1);
    assert_eq!(
        e.transact(
            0,
            &[Edit::Split {
                at: Address::body(0),
                offset: 1
            }]
        ),
        Err(EditError::LimitExceeded)
    );
    assert_eq!(e.document().body, original);
    let mut e = e.with_history_byte_limit(64 * 1024 * 1024);
    e.transact(
        0,
        &[Edit::Split {
            at: Address::body(0),
            offset: 1,
        }],
    )
    .unwrap();
    assert_ne!(
        e.paragraph(&Address::body(0)).unwrap().location,
        e.paragraph(&Address::body(1)).unwrap().location
    );
    e.undo(1).unwrap();
    let changed = e
        .transact(
            2,
            &[Edit::TextNode {
                at: Address::body(0),
                inline: vec![0],
                content: 0,
                range: 0..3,
                text: "А🙂Б".into(),
            }],
        )
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert!(!changed.invalidate_layout);
    e.redo(2).unwrap();
}
#[test]
fn cached_field_text_and_paragraph_total_limit_are_protected() {
    let mut d=doc("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).unwrap();
    assert_eq!(
        e.transact(
            0,
            &[Edit::TextNode {
                at: Address::body(0),
                inline: vec![3],
                content: 0,
                range: 0..1,
                text: "2".into()
            }]
        ),
        Err(EditError::UnsupportedContent)
    );
    let mut d = doc("<w:p><w:r><w:t>ab</w:t><w:tab/><w:t>cd</w:t></w:r></w:p>");
    let mut e = Editor::new(
        &mut d,
        EditLimits {
            paragraph_scalars: 4,
            history_transactions: 2,
        },
    )
    .unwrap();
    assert_eq!(
        e.transact(
            0,
            &[Edit::TextNode {
                at: Address::body(0),
                inline: vec![0],
                content: 0,
                range: 0..0,
                text: "x".into()
            }]
        ),
        Err(EditError::LimitExceeded)
    );
}
