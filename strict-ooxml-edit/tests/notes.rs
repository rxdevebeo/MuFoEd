//! K03: inserting and deleting footnotes and endnotes.
use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Address, Edit, EditError, EditLimits, Editor, Invariant, Story, Unsupported,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{
    Block, Document, Inline, Note, NoteKind, Paragraph, RunContent, VertAlign,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn doc(body: &str) -> Document {
    let xml = format!(
        "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\">\
         <w:body>{body}</w:body></w:document>"
    );
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

/// A paragraph holding `text`, built through the parser like the rest.
fn paragraph(text: &str) -> Block {
    doc(&format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"))
        .body
        .blocks
        .remove(0)
}

fn note(id: i32, kind: NoteKind, text: &str) -> Note {
    Note {
        id,
        kind,
        blocks: vec![paragraph(text)],
        location: SourceLocation::unknown(),
    }
}

/// A body with a reference to footnote 1, and the separators Word writes.
fn with_notes() -> Document {
    let mut d = doc(
        "<w:p><w:r><w:t>Body</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
         <w:p><w:r><w:t>Second</w:t></w:r></w:p>",
    );
    d.footnotes.insert(note(-1, NoteKind::Separator, ""));
    d.footnotes
        .insert(note(0, NoteKind::ContinuationSeparator, ""));
    d.footnotes.insert(note(1, NoteKind::Normal, "First note"));
    d
}

fn first_inlines(document: &Document, block: usize) -> &[Inline] {
    match document.body.blocks.get(block) {
        Some(Block::Paragraph(Paragraph { inlines, .. })) => inlines,
        other => panic!("paragraph expected, got {other:?}"),
    }
}

#[test]
fn a_footnote_gets_the_next_id_a_reference_and_a_number_marker() {
    let mut d = with_notes();
    let before = (d.body.clone(), d.footnotes.iter().count());
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let change = e
        .transact(
            0,
            &[Edit::InsertNote {
                at: Address::body(1),
                index: 1,
                endnote: false,
                blocks: vec![paragraph("Second note")],
            }],
        )
        .expect("insert");
    let after = e.document();
    // -1 and 0 are the separators; normal notes count from 1, so 2 is next.
    let created = after.footnotes.get(2).expect("note 2");
    assert_eq!(created.kind, NoteKind::Normal);
    let Some(Block::Paragraph(first)) = created.blocks.first() else {
        panic!("paragraph")
    };
    let Some(Inline::Run(marker)) = first.inlines.first() else {
        panic!("marker run")
    };
    assert_eq!(marker.content, [RunContent::NoteRef]);
    assert_eq!(marker.props.vert_align, Some(VertAlign::Superscript));
    let Some(Inline::Run(reference)) = first_inlines(after, 1).get(1) else {
        panic!("reference run")
    };
    assert_eq!(reference.content, [RunContent::FootnoteRef(2)]);
    assert!(e.validate().is_ok());
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().body, before.0);
    assert_eq!(e.document().footnotes.iter().count(), before.1);
    assert!(e.document().footnotes.get(2).is_none());
}

#[test]
fn a_referenced_note_stays_until_its_reference_goes_with_it() {
    let mut d = with_notes();
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let note = Address {
        story: Story::Footnote(1),
        containers: vec![],
        block: 0,
    };
    assert_eq!(
        e.transact(0, &[Edit::DeleteNote { at: note.clone() }]),
        Err(EditError::InvalidModel(Invariant::DefinitionInUse))
    );
    // The separators are not notes to delete.
    let separator = Address {
        story: Story::Footnote(-1),
        containers: vec![],
        block: 0,
    };
    assert_eq!(
        e.transact(0, &[Edit::DeleteNote { at: separator }]),
        Err(EditError::UnsupportedContent(Unsupported::Target))
    );
    let change = e
        .transact(
            0,
            &[
                Edit::DeleteInline {
                    at: Address::body(0),
                    index: 1,
                },
                Edit::DeleteNote { at: note },
            ],
        )
        .expect("delete reference and note");
    assert!(e.document().footnotes.get(1).is_none());
    assert_eq!(first_inlines(e.document(), 0).len(), 1);
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().body, original.body);
    assert!(e.document().footnotes.get(1).is_some());
}

#[test]
fn deleting_a_reference_alone_keeps_its_note() {
    let mut d = with_notes();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::DeleteInline {
            at: Address::body(0),
            index: 1,
        }],
    )
    .expect("delete reference");
    assert!(
        e.document().footnotes.get(1).is_some(),
        "no hidden collection of unreferenced notes"
    );
}

#[test]
fn an_endnote_in_a_document_without_notes_starts_at_one() {
    let mut d = doc("<w:p><w:r><w:t>Body</w:t></w:r></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::InsertNote {
            at: Address::body(0),
            index: 1,
            endnote: true,
            blocks: vec![],
        }],
    )
    .expect("insert");
    let created = e.document().endnotes.get(1).expect("endnote 1");
    assert!(
        matches!(created.blocks.last(), Some(Block::Paragraph(_))),
        "a note ends with a paragraph"
    );
    let Some(Inline::Run(reference)) = first_inlines(e.document(), 0).get(1) else {
        panic!("reference run")
    };
    assert_eq!(reference.content, [RunContent::EndnoteRef(1)]);
    assert!(e.document().footnotes.is_empty());
}

#[test]
fn a_note_cannot_hold_a_note_reference() {
    let mut d = with_notes();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let inside = Address {
        story: Story::Footnote(1),
        containers: vec![],
        block: 0,
    };
    assert_eq!(
        e.transact(
            0,
            &[Edit::InsertNote {
                at: inside,
                index: 0,
                endnote: false,
                blocks: vec![],
            }],
        ),
        Err(EditError::UnsupportedContent(Unsupported::Target))
    );
    // A reference index past the paragraph is refused, and nothing is left.
    assert_eq!(
        e.transact(
            0,
            &[Edit::InsertNote {
                at: Address::body(1),
                index: 9,
                endnote: false,
                blocks: vec![],
            }],
        ),
        Err(EditError::InvalidRange)
    );
    assert!(e.document().footnotes.get(2).is_none());
}
