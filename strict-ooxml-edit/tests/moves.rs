//! K06 (first part): `Edit::Move` between containers of one story.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Address, Container, Edit, EditError, EditLimits, Editor, Invariant, Story, Unsupported,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Block, Document, Inline, ParaId, RunContent};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// `one`, a table of two cells (`left`, `right`), `two`, a block SDT holding
/// `inner`, and `last` ending the only section.
fn document() -> Document {
    let xml = "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body>\
               <w:p><w:r><w:t>one</w:t></w:r></w:p>\
               <w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
               <w:tr><w:tc><w:p><w:r><w:t>left</w:t></w:r></w:p></w:tc>\
               <w:tc><w:p><w:r><w:t>right</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
               <w:p><w:r><w:t>two</w:t></w:r></w:p>\
               <w:sdt><w:sdtContent><w:p><w:r><w:t>inner</w:t></w:r></w:p></w:sdtContent></w:sdt>\
               <w:p><w:r><w:t>last</w:t></w:r></w:p>\
               <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>\
               </w:body></w:document>";
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.as_bytes().to_vec())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

fn cell(table: usize, cell: usize, block: usize) -> Address {
    Address {
        story: Story::Body,
        containers: vec![Container::Cell {
            table,
            row: 0,
            cell,
        }],
        block,
    }
}

fn sdt(index: usize, block: usize) -> Address {
    Address {
        story: Story::Body,
        containers: vec![Container::Sdt(index)],
        block,
    }
}

fn texts(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &p.inlines {
                    if let Inline::Run(r) = inline {
                        for content in &r.content {
                            if let RunContent::Text(t) = content {
                                out.push_str(&t.text);
                            }
                        }
                    }
                }
                out.push(' ');
            }
            Block::Table(t) => {
                out.push('[');
                for c in t.rows.iter().flat_map(|r| &r.cells) {
                    out.push('|');
                    out.push_str(texts(&c.blocks).trim_end());
                }
                out.push_str("] ");
            }
            Block::SdtBlock(s) => {
                out.push('{');
                out.push_str(texts(&s.blocks).trim_end());
                out.push_str("} ");
            }
            _ => {}
        }
    }
    out
}

fn body(editor: &Editor<'_>) -> String {
    texts(&editor.document().body.blocks).trim_end().to_owned()
}

/// Gives the paragraph at `at` an identity and returns it.
fn identify(editor: &mut Editor<'_>, at: &Address) -> ParaId {
    let revision = editor.revision();
    editor
        .transact(revision, &[Edit::Identify { at: at.clone() }])
        .expect("identify");
    editor
        .paragraph(at)
        .expect("paragraph")
        .para_id
        .clone()
        .expect("id")
}

fn moved(editor: &mut Editor<'_>, at: Address, to: Address) -> Result<(), EditError> {
    let revision = editor.revision();
    editor
        .transact(revision, &[Edit::Move { at, to }])
        .map(drop)
}

#[test]
fn a_paragraph_moves_into_a_cell_and_out_keeping_its_identity() {
    let mut d = document();
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let id = identify(&mut e, &Address::body(0));
    // Into the right cell, before its paragraph; the table moves up by one.
    moved(&mut e, Address::body(0), cell(1, 1, 0)).expect("into the cell");
    assert_eq!(body(&e), "[|left|one right] two {inner} last");
    assert_eq!(e.find(&Story::Body, &id).expect("found"), cell(0, 1, 0));
    // Out of the cell, after `two`.
    moved(&mut e, cell(0, 1, 0), Address::body(2)).expect("out of the cell");
    assert_eq!(body(&e), "[|left|right] two one {inner} last");
    assert_eq!(e.find(&Story::Body, &id).expect("found"), Address::body(2));
    assert!(e.validate().is_ok());
    while e.undo(e.revision()).is_ok() {}
    assert_eq!(e.document().body, original.body);
}

#[test]
fn moves_within_a_container_and_between_siblings() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    // Down and up in the body; to its own place is no change.
    moved(&mut e, Address::body(0), Address::body(3)).expect("down");
    assert_eq!(body(&e), "[|left|right] two one {inner} last");
    moved(&mut e, Address::body(2), Address::body(0)).expect("up");
    assert_eq!(body(&e), "one [|left|right] two {inner} last");
    let revision = e.revision();
    for to in [0, 1] {
        moved(&mut e, Address::body(0), Address::body(to)).expect("in place");
    }
    assert_eq!(
        e.revision(),
        revision,
        "a move to where it is records nothing"
    );
    // From the SDT into the left cell of the table before it.
    moved(&mut e, sdt(3, 0), cell(1, 0, 1)).expect("sdt to cell");
    assert_eq!(body(&e), "one [|left inner|right] two {} last");
    // A whole table into the SDT that follows it.
    moved(&mut e, Address::body(1), sdt(3, 0)).expect("table to sdt");
    assert_eq!(body(&e), "one two {[|left inner|right]} last");
    assert!(e.validate().is_ok());
}

#[test]
fn moves_that_would_break_the_document_are_refused() {
    let mut d = document();
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    // A cell keeps its last paragraph.
    assert_eq!(
        moved(&mut e, cell(1, 0, 0), Address::body(0)),
        Err(EditError::InvalidModel(Invariant::TableTopology))
    );
    // A table does not go into itself.
    assert_eq!(
        moved(&mut e, Address::body(1), cell(1, 0, 0)),
        Err(EditError::UnsupportedContent(Unsupported::Target))
    );
    // Nor a block into another story.
    let note = Address {
        story: Story::Footnote(1),
        containers: vec![],
        block: 0,
    };
    assert_eq!(
        moved(&mut e, Address::body(0), note),
        Err(EditError::UnsupportedContent(Unsupported::Target))
    );
    // Past the end of the target.
    assert_eq!(
        moved(&mut e, Address::body(0), cell(1, 1, 5)),
        Err(EditError::InvalidParagraph)
    );
    assert_eq!(e.document().body, original.body);
    assert_eq!(e.revision(), 0);
}

#[test]
fn a_section_boundary_paragraph_stays_where_it_is() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::InsertSectionBreak {
            at: Address::body(0),
        }],
    )
    .expect("break");
    assert_eq!(
        moved(&mut e, Address::body(0), Address::body(3)),
        Err(EditError::InvalidModel(Invariant::SectionBoundary))
    );
    // Other paragraphs move across the boundary freely.
    moved(&mut e, Address::body(2), Address::body(0)).expect("across");
    assert_eq!(body(&e), "two one [|left|right] {inner} last");
    assert!(e.validate().is_ok());
}
