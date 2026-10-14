//! K12 (first part): typed structural comparison of two documents.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    diff, Address, Definition, DiffChange, DiffItem, DiffMode, Edit, EditLimits, Editor, Story,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{DocDefaults, Document, HalfPoints};
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

const THREE: &str = "<w:p><w:r><w:t>one</w:t></w:r></w:p>\
                     <w:p><w:r><w:t>two</w:t></w:r></w:p>\
                     <w:p><w:r><w:t>three</w:t></w:r></w:p>";

/// An editor's document with every top-level paragraph given an identity,
/// so that a changed paragraph pairs with its old self.
fn identified(body: &str) -> Document {
    let mut d = doc(body);
    let count = d.body.blocks.len();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let identify: Vec<Edit> = (0..count)
        .map(|block| Edit::Identify {
            at: Address::body(block),
        })
        .collect();
    e.transact(0, &identify).expect("identify");
    e.document().clone()
}

fn body(before: Option<usize>, after: Option<usize>, change: DiffChange) -> DiffItem {
    DiffItem::Block {
        story: Story::Body,
        before,
        after,
        change,
    }
}

#[test]
fn equal_documents_and_an_undone_edit_have_no_differences() {
    let original = doc(THREE);
    for mode in [DiffMode::Exact, DiffMode::Semantic] {
        assert!(diff(&original, &original, mode).expect("diff").is_empty());
    }
    let mut d = original.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let change = e
        .transact(
            0,
            &[Edit::Text {
                at: Address::body(1),
                range: 0..3,
                text: "TWO".into(),
            }],
        )
        .expect("edit");
    e.undo(change.revision).expect("undo");
    assert!(diff(&original, e.document(), DiffMode::Exact)
        .expect("diff")
        .is_empty());
}

#[test]
fn edits_show_as_modified_added_and_removed_blocks() {
    let original = identified(THREE);
    let mut d = original.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let fresh = doc("<w:p><w:r><w:t>new</w:t></w:r></w:p>")
        .body
        .blocks
        .remove(0);
    e.transact(
        0,
        &[
            Edit::Text {
                at: Address::body(1),
                range: 0..3,
                text: "TWO".into(),
            },
            Edit::Delete {
                at: Address::body(2),
            },
            Edit::Insert {
                at: Address::body(0),
                block: Box::new(fresh),
            },
        ],
    )
    .expect("edits");
    let changes = diff(&original, e.document(), DiffMode::Semantic).expect("diff");
    assert_eq!(
        changes.items,
        [
            body(Some(1), Some(2), DiffChange::Modified),
            body(Some(2), None, DiffChange::Removed),
            body(None, Some(0), DiffChange::Added),
        ]
    );
}

#[test]
fn a_paragraph_that_kept_its_identity_is_reported_moved() {
    let before = identified(THREE);
    let mut after = before.clone();
    after.body.blocks.swap(0, 1);
    let changes = diff(&before, &after, DiffMode::Exact).expect("diff");
    assert_eq!(
        changes.items,
        [body(Some(0), Some(1), DiffChange::Moved)],
        "one paragraph moved; nothing was added or removed"
    );
}

#[test]
fn semantic_comparison_ignores_run_boundaries_exact_does_not() {
    let whole = doc("<w:p><w:r><w:t>ab</w:t></w:r></w:p>");
    let split = doc("<w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r></w:p>");
    assert!(diff(&whole, &split, DiffMode::Semantic)
        .expect("diff")
        .is_empty());
    // Without an identity the two paragraphs are not paired by guessing.
    assert_eq!(
        diff(&whole, &split, DiffMode::Exact).expect("diff").items,
        [
            body(Some(0), None, DiffChange::Removed),
            body(None, Some(0), DiffChange::Added)
        ]
    );
    // Different formatting is a difference in either mode.
    let bold = doc("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>ab</w:t></w:r></w:p>");
    assert!(!diff(&whole, &bold, DiffMode::Semantic)
        .expect("diff")
        .is_empty());
}

#[test]
fn definitions_sections_and_notes_are_compared_too() {
    let original = identified(&format!(
        "{THREE}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>"
    ));
    let mut d = original.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let mut defaults = DocDefaults::default();
    defaults.run.size = Some(HalfPoints(30));
    e.define(0, &[Definition::SetDocDefaults(Box::new(defaults))])
        .expect("defaults");
    e.transact(
        1,
        &[
            Edit::InsertSectionBreak {
                at: Address::body(0),
            },
            Edit::InsertNote {
                at: Address::body(2),
                index: 1,
                endnote: false,
                blocks: vec![],
            },
        ],
    )
    .expect("break and note");
    let changes = diff(&original, e.document(), DiffMode::Semantic)
        .expect("diff")
        .items;
    for expected in [
        DiffItem::Defaults,
        DiffItem::Story {
            story: Story::Footnote(1),
            change: DiffChange::Added,
        },
        DiffItem::Section {
            before: None,
            after: Some(1),
            change: DiffChange::Added,
        },
        body(Some(0), Some(0), DiffChange::Modified),
        body(Some(2), Some(2), DiffChange::Modified),
    ] {
        assert!(
            changes.contains(&expected),
            "{expected:?} not in {changes:?}"
        );
    }
    assert!(
        !changes.contains(&body(Some(1), Some(1), DiffChange::Modified)),
        "the untouched paragraph is not reported"
    );
    // A block-less story is no block.
    assert!(!changes.iter().any(|item| matches!(
        item,
        DiffItem::Block {
            story: Story::Footnote(_),
            ..
        }
    )));
}
