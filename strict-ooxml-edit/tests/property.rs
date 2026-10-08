//! Property tests for the structural editor.
//!
//! Random command sequences against a document with body paragraphs, a table
//! and a block SDT, with targets and ranges that are as often wrong as right.
//! The editor's contract, checked after every step:
//!
//! - it never panics;
//! - a rejected transaction leaves the document and the revision untouched;
//! - an accepted one leaves a model that passes `validate`;
//!
//! and over the whole sequence: undoing every accepted transaction restores
//! the original document exactly, and redoing them all restores the result.

use std::fmt::Write as _;

use proptest::prelude::*;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{Address, ChangeSet, Edit, EditLimits, Editor, FormatPatch, Story};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Block, Document, TriState};
use strict_ooxml_wml::{parse_document, ParseOptions};

const BODY: &str = concat!(
    "<w:p><w:r><w:t>Alpha beta</w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Ж🙂й</w:t></w:r><w:r><w:t xml:space=\"preserve\"> tail</w:t></w:r></w:p>",
    "<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>",
    "<w:tr><w:tc><w:p><w:r><w:t>cell one</w:t></w:r></w:p></w:tc>",
    "<w:tc><w:p><w:r><w:t>cell two</w:t></w:r></w:p><w:p/></w:tc></w:tr></w:tbl>",
    "<w:sdt><w:sdtPr><w:alias w:val=\"s\"/></w:sdtPr><w:sdtContent>",
    "<w:p><w:r><w:t>in sdt</w:t></w:r></w:p></w:sdtContent></w:sdt>",
    "<w:p><w:r><w:t>last</w:t></w:r></w:p>",
);

fn document() -> Document {
    let xml = format!(
        "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\">\
         <w:body>{BODY}</w:body></w:document>"
    );
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

/// A command with its target chosen later, against the editor's current state:
/// `target` picks one of the story's paragraph addresses (modulo their count),
/// or, when `wild`, a body block index that may not exist.
#[derive(Clone, Debug)]
struct Step {
    target: usize,
    wild: bool,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Text {
        start: usize,
        len: usize,
        text: String,
    },
    Bold {
        start: usize,
        len: usize,
        on: bool,
    },
    Split {
        offset: usize,
    },
    Join,
    Delete,
    Identify,
}

fn text() -> impl Strategy<Value = String> {
    // Mostly ordinary text, sometimes a character the editor must refuse.
    prop::collection::vec(
        prop_oneof![
            8 => prop::sample::select(vec!['a', 'Z', ' ', 'ё', '🙂', '\u{301}']),
            1 => prop::sample::select(vec!['\n', '\t', '\u{0}', '\u{FFFF}']),
        ],
        0..6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![
        4 => (0usize..16, 0usize..8, text())
            .prop_map(|(start, len, text)| Kind::Text { start, len, text }),
        2 => (0usize..16, 0usize..8, any::<bool>())
            .prop_map(|(start, len, on)| Kind::Bold { start, len, on }),
        2 => (0usize..16).prop_map(|offset| Kind::Split { offset }),
        1 => Just(Kind::Join),
        1 => Just(Kind::Delete),
        1 => Just(Kind::Identify),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    (any::<usize>(), prop::bool::weighted(0.1), kind()).prop_map(|(target, wild, kind)| Step {
        target,
        wild,
        kind,
    })
}

fn resolve(editor: &Editor<'_>, step: &Step) -> Edit {
    let at = if step.wild {
        Address::body(step.target % 64)
    } else {
        let addresses = editor.paragraphs(&Story::Body).unwrap_or_default();
        if addresses.is_empty() {
            Address::body(0)
        } else {
            addresses[step.target % addresses.len()].clone()
        }
    };
    match &step.kind {
        Kind::Text { start, len, text } => Edit::Text {
            at,
            range: *start..start + len,
            text: text.clone(),
        },
        Kind::Bold { start, len, on } => Edit::Format {
            at,
            range: *start..start + len,
            patch: FormatPatch {
                bold: Some(if *on { TriState::On } else { TriState::Off }),
                ..FormatPatch::default()
            },
        },
        Kind::Split { offset } => Edit::Split {
            at,
            offset: *offset,
        },
        Kind::Join => Edit::Join { at },
        Kind::Delete => Edit::Delete { at },
        Kind::Identify => Edit::Identify { at },
    }
}

/// Whether every body block outside `change.blocks` is the block that stood
/// in its place before, shifted by the changes in front of it.
fn untouched_blocks_kept(before: &[Block], after: &[Block], change: &ChangeSet) -> bool {
    let mut ranges: Vec<_> = change
        .blocks
        .iter()
        .filter(|block| block.story == Story::Body)
        .collect();
    ranges.sort_by_key(|block| block.range.start);
    let (mut now, mut then) = (0, 0);
    for block in ranges {
        let gap = block.range.start - now;
        if after.get(now..block.range.start) != before.get(then..then + gap) {
            return false;
        }
        now = block.range.end;
        then += gap + block.replaced;
    }
    after.get(now..) == before.get(then..)
}

/// The whole model, as one comparable value.
fn snapshot(document: &Document) -> String {
    format!("{document:?}")
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn random_edits_keep_the_contract(
        steps in prop::collection::vec(step(), 1..24),
        batch in 1usize..4,
    ) {
        let mut document = document();
        let original = snapshot(&document);
        let mut editor = Editor::new(&mut document, EditLimits::default()).expect("editor");
        let mut accepted = 0_usize;
        for chunk in steps.chunks(batch) {
            let edits: Vec<Edit> = chunk.iter().map(|step| resolve(&editor, step)).collect();
            let before = snapshot(editor.document());
            let body = editor.document().body.blocks.clone();
            let revision = editor.revision();
            match editor.transact_detailed(revision, &edits) {
                Ok(change) => {
                    prop_assert!(editor.validate().is_ok(), "accepted {edits:?} left an invalid model");
                    prop_assert!(
                        untouched_blocks_kept(&body, &editor.document().body.blocks, &change),
                        "{:?} misses a changed block", change.blocks
                    );
                    if change.invalidate_layout {
                        prop_assert_eq!(editor.revision(), revision + 1);
                        accepted += 1;
                    } else {
                        prop_assert_eq!(snapshot(editor.document()), before);
                    }
                }
                Err(failure) => {
                    prop_assert_eq!(editor.revision(), revision);
                    prop_assert_eq!(snapshot(editor.document()), before, "{}", failure);
                    if let Some(index) = failure.command {
                        prop_assert!(index < edits.len());
                    }
                }
            }
        }
        let result = snapshot(editor.document());
        for _ in 0..accepted {
            let body = editor.document().body.blocks.clone();
            let revision = editor.revision();
            let change = editor.undo(revision);
            prop_assert!(change.is_ok());
            if let Ok(change) = change {
                prop_assert!(untouched_blocks_kept(&body, &editor.document().body.blocks, &change));
            }
        }
        prop_assert_eq!(snapshot(editor.document()), original.clone(), "undo all");
        for _ in 0..accepted {
            let revision = editor.revision();
            prop_assert!(editor.redo(revision).is_ok());
        }
        prop_assert_eq!(snapshot(editor.document()), result, "redo all");
    }
}

#[test]
fn typing_keeps_only_the_touched_paragraph_in_history() {
    // Roadmap stage 1: a keystroke used to keep two copies of the whole
    // document in the undo history.
    let mut body = String::new();
    for index in 0..1_000 {
        let _ = write!(
            body,
            "<w:p><w:r><w:t>Paragraph {index} of body text.</w:t></w:r></w:p>"
        );
    }
    let xml = format!(
        "<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\">\
         <w:body>{body}</w:body></w:document>"
    );
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    let mut document = parse_document(&package, &ParseOptions::default()).expect("document");
    let whole = snapshot(&document).len();
    let mut editor = Editor::new(&mut document, EditLimits::default()).expect("editor");
    for offset in 0..50 {
        let revision = editor.revision();
        editor
            .transact(
                revision,
                &[Edit::Text {
                    at: Address::body(500),
                    range: offset..offset,
                    text: "x".to_owned(),
                }],
            )
            .expect("type");
    }
    // Fifty whole-document snapshots would be a hundred times `whole`. The
    // touched paragraph itself grows: each insertion splits its run, so late
    // steps keep dozens of runs - a few times `whole` in all, not a hundred.
    assert!(
        editor.history_bytes() < whole * 4,
        "history {} bytes against a {whole}-byte document",
        editor.history_bytes()
    );
}
