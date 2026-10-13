//! K02: styles, lists and document defaults as editor transactions.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    restarted, unused_num_id, Definition, EditError, EditLimits, Editor, Invariant,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{
    Block, DocDefaults, Document, HalfPoints, Ilvl, Inline, NumId, Style, StyleId, StyleType,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

fn document() -> Document {
    let styles = format!(
        "<w:styles xmlns:w=\"{W}\">\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Heading\"><w:name w:val=\"Heading\"/><w:basedOn w:val=\"Normal\"/>\
         <w:next w:val=\"Normal\"/><w:rPr><w:b/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Child\"><w:name w:val=\"Child\"/><w:basedOn w:val=\"Heading\"/></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Emphasis\"><w:name w:val=\"Emphasis\"/><w:rPr><w:i/></w:rPr></w:style>\
         </w:styles>"
    );
    let numbering = format!(
        "<w:numbering xmlns:w=\"{W}\">\
         <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/>\
         <w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum>\
         <w:num w:numId=\"1\"><w:abstractNumId w:val=\"1\"/></w:num>\
         <w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
         </w:numbering>"
    );
    let body = "<w:p><w:pPr><w:pStyle w:val=\"Heading\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
                <w:r><w:t>one</w:t></w:r></w:p>\
                <w:p><w:pPr><w:pStyle w:val=\"Child\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"Strong\"/></w:rPr><w:t>two</w:t></w:r></w:p>\
                <w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc>\
                <w:p><w:pPr><w:pStyle w:val=\"Heading\"/></w:pPr><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
                <w:p/>";
    let bytes = DocxBuilder::strict()
        .body(body)
        .part("word/styles.xml", styles.into_bytes())
        .rel("styles", "styles", "styles.xml")
        .part("word/numbering.xml", numbering.into_bytes())
        .rel("numbering", "numbering", "numbering.xml")
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

fn id(value: &str) -> StyleId {
    StyleId::new(value)
}

/// A copy of style `from` under a new id and name.
fn style_like(document: &Document, from: &str, to: &str) -> Style {
    let mut style = document.styles.get(&id(from)).expect("style").clone();
    style.id = id(to);
    style.name = Some(to.into());
    style.is_default = false;
    style
}

/// The `w:pStyle` of every paragraph in reading order, table cells included.
fn paragraph_styles(document: &Document) -> Vec<Option<String>> {
    fn walk(blocks: &[Block], out: &mut Vec<Option<String>>) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    out.push(p.props.style.as_ref().map(|s| s.as_str().to_owned()))
                }
                Block::Table(t) => {
                    for row in &t.rows {
                        for cell in &row.cells {
                            walk(&cell.blocks, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&document.body.blocks, &mut out);
    out
}

fn run_style(document: &Document) -> Option<String> {
    let Some(Block::Paragraph(p)) = document.body.blocks.get(1) else {
        panic!("paragraph")
    };
    let Some(Inline::Run(run)) = p.inlines.first() else {
        panic!("run")
    };
    run.props.style.as_ref().map(|s| s.as_str().to_owned())
}

fn num_id_of_first(document: &Document) -> Option<NumId> {
    let Some(Block::Paragraph(p)) = document.body.blocks.first() else {
        panic!("paragraph")
    };
    p.props.numbering.as_ref().and_then(|n| n.num_id)
}

#[test]
fn a_new_style_can_be_created_used_and_undone() {
    let mut d = document();
    let fresh = style_like(&d, "Heading", "Title");
    let before_styles = d.styles.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let change = e
        .define(0, &[Definition::CreateStyle(Box::new(fresh.clone()))])
        .expect("create");
    assert!(change.invalidate_layout);
    let created = e.document().styles.get(&id("Title")).expect("created");
    assert_eq!(
        created.based_on_chain,
        [id("Normal")],
        "the chain is derived again"
    );
    // The same id again is a conflict, and a replacement needs an existing id.
    let revision = e.revision();
    assert_eq!(
        e.define(revision, &[Definition::CreateStyle(Box::new(fresh))]),
        Err(EditError::InvalidModel(Invariant::DefinitionExists))
    );
    let ghost = style_like(e.document(), "Heading", "Ghost");
    assert_eq!(
        e.define(revision, &[Definition::ReplaceStyle(Box::new(ghost))]),
        Err(EditError::InvalidModel(Invariant::DefinitionMissing))
    );
    e.undo(revision).expect("undo");
    assert_eq!(e.document().styles, before_styles);
}

#[test]
fn a_style_in_use_moves_its_references_or_is_kept() {
    let mut d = document();
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    assert_eq!(
        e.define(
            0,
            &[Definition::DeleteStyle {
                id: id("Heading"),
                replace_with: None,
            }]
        ),
        Err(EditError::InvalidModel(Invariant::DefinitionInUse))
    );
    assert_eq!(
        e.document().body,
        original.body,
        "a refusal changes nothing"
    );
    // A replacement of another kind is refused too.
    assert_eq!(
        e.define(
            0,
            &[Definition::DeleteStyle {
                id: id("Heading"),
                replace_with: Some(id("Strong")),
            }]
        ),
        Err(EditError::UnknownStyle(id("Strong")))
    );
    let change = e
        .define(
            0,
            &[Definition::DeleteStyle {
                id: id("Heading"),
                replace_with: Some(id("Normal")),
            }],
        )
        .expect("delete with replacement");
    let after = e.document();
    assert!(after.styles.get(&id("Heading")).is_none());
    assert_eq!(
        paragraph_styles(after),
        [
            Some("Normal".into()),
            Some("Child".into()),
            Some("Normal".into()),
            None
        ],
        "the body paragraph and the one in the cell moved; Child stays"
    );
    let child = after.styles.get(&id("Child")).expect("child");
    assert_eq!(
        (child.based_on.clone(), child.based_on_chain.clone()),
        (Some(id("Normal")), vec![id("Normal")]),
        "a style based on the deleted one is based on its parent"
    );
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().body, original.body);
    assert_eq!(e.document().styles, original.styles);
}

#[test]
fn a_character_style_moves_its_runs() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.define(
        0,
        &[Definition::DeleteStyle {
            id: id("Strong"),
            replace_with: Some(id("Emphasis")),
        }],
    )
    .expect("delete");
    assert_eq!(run_style(e.document()), Some("Emphasis".into()));
    // An unused style goes without a replacement.
    let revision = e.revision();
    e.define(
        revision,
        &[Definition::DeleteStyle {
            id: id("Emphasis"),
            replace_with: None,
        }],
    )
    .expect_err("Emphasis is used now");
}

#[test]
fn the_default_style_and_a_based_on_cycle_are_refused() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    assert_eq!(
        e.define(
            0,
            &[Definition::DeleteStyle {
                id: id("Normal"),
                replace_with: Some(id("Heading")),
            }]
        ),
        Err(EditError::InvalidModel(Invariant::DefinitionInUse))
    );
    let mut looped = e
        .document()
        .styles
        .get(&id("Heading"))
        .expect("heading")
        .clone();
    looped.based_on = Some(id("Child"));
    assert_eq!(
        e.define(0, &[Definition::ReplaceStyle(Box::new(looped))]),
        Err(EditError::InvalidModel(Invariant::StyleChain))
    );
    assert_eq!(e.revision(), 0);
}

#[test]
fn a_batch_creates_a_style_and_bases_another_on_it() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let base = style_like(e.document(), "Normal", "Base");
    let mut derived = style_like(e.document(), "Normal", "Derived");
    derived.based_on = Some(id("Base"));
    e.define(
        0,
        &[
            Definition::CreateStyle(Box::new(base)),
            Definition::CreateStyle(Box::new(derived)),
        ],
    )
    .expect("batch");
    let derived = e.document().styles.get(&id("Derived")).expect("derived");
    assert_eq!(derived.based_on_chain, [id("Base")]);
    assert_eq!(
        e.document().styles.get(&id("Base")).map(|s| s.style_type),
        Some(StyleType::Paragraph)
    );
}

#[test]
fn a_list_in_use_moves_its_paragraphs_or_is_kept() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    assert_eq!(
        e.define(
            0,
            &[Definition::DeleteNum {
                id: NumId(1),
                replace_with: None,
            }]
        ),
        Err(EditError::InvalidModel(Invariant::DefinitionInUse))
    );
    // An abstract definition with instances cannot go.
    assert_eq!(
        e.define(
            0,
            &[Definition::DeleteAbstractNum {
                id: strict_ooxml_wml::model::AbstractNumId(1),
            }]
        ),
        Err(EditError::InvalidModel(Invariant::DefinitionInUse))
    );
    let change = e
        .define(
            0,
            &[Definition::DeleteNum {
                id: NumId(1),
                replace_with: Some(NumId(2)),
            }],
        )
        .expect("delete with replacement");
    assert_eq!(num_id_of_first(e.document()), Some(NumId(2)));
    assert!(e.document().numbering.num(NumId(1)).is_none());
    e.undo(change.revision).expect("undo");
    assert_eq!(num_id_of_first(e.document()), Some(NumId(1)));
    assert!(e.document().numbering.num(NumId(1)).is_some());
}

#[test]
fn restarting_a_list_makes_a_new_instance_with_a_start_override() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let fresh = unused_num_id(e.document());
    assert_eq!(fresh, NumId(3));
    let num = e.document().numbering.num(NumId(1)).expect("num").clone();
    let restart = restarted(&num, fresh, Ilvl(0), 5);
    e.define(0, &[Definition::CreateNum(Box::new(restart))])
        .expect("create");
    let created = e.document().numbering.num(fresh).expect("created");
    assert_eq!(created.abstract_num_id, num.abstract_num_id);
    assert_eq!(created.overrides[0].start_override, Some(5));
    // Id 0 means "no list" in w:numPr and is never an instance.
    let mut zero = num.clone();
    zero.num_id = NumId(0);
    let revision = e.revision();
    assert_eq!(
        e.define(revision, &[Definition::CreateNum(Box::new(zero))]),
        Err(EditError::InvalidModel(Invariant::Numbering))
    );
}

#[test]
fn document_defaults_are_set_and_undone() {
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let mut defaults = DocDefaults::default();
    defaults.run.size = Some(HalfPoints(28));
    let change = e
        .define(0, &[Definition::SetDocDefaults(Box::new(defaults.clone()))])
        .expect("defaults");
    assert_eq!(e.document().styles.defaults(), Some(&defaults));
    e.undo(change.revision).expect("undo");
    assert_eq!(e.document().styles.defaults(), None);
}

#[test]
fn lists_are_applied_and_removed_as_one_transaction() {
    use strict_ooxml_edit::{Address, Edit, OperationLimits, Operations};
    use strict_ooxml_wml::model::NumPr;
    let mut d = document();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    let numbering = |document: &Document, block: usize| -> Option<NumPr> {
        match document.body.blocks.get(block) {
            Some(Block::Paragraph(p)) => p.props.numbering,
            _ => None,
        }
    };
    // The second paragraph joins list 2 at level 1.
    Operations::new(&mut e, OperationLimits::default())
        .apply_list(0, &[Address::body(1)], NumId(2), Ilvl(1))
        .expect("apply");
    assert_eq!(
        numbering(e.document(), 1),
        Some(NumPr {
            num_id: Some(NumId(2)),
            ilvl: Some(Ilvl(1)),
        })
    );
    // A list that does not exist is refused.
    assert!(Operations::new(&mut e, OperationLimits::default())
        .apply_list(1, &[Address::body(1)], NumId(9), Ilvl(0))
        .is_err());
    // A paragraph listed directly loses its numPr...
    Operations::new(&mut e, OperationLimits::default())
        .remove_list(1, &[Address::body(0), Address::body(1)])
        .expect("remove");
    assert_eq!(numbering(e.document(), 0), None);
    assert_eq!(numbering(e.document(), 1), None);
    // ...one whose style lists it gets numId 0, "no list".
    let mut listed = style_like(e.document(), "Normal", "Listed");
    listed.paragraph.numbering = Some(NumPr {
        num_id: Some(NumId(1)),
        ilvl: Some(Ilvl(0)),
    });
    e.define(2, &[Definition::CreateStyle(Box::new(listed))])
        .expect("listed style");
    let mut properties = match e.document().body.blocks.first() {
        Some(Block::Paragraph(p)) => p.props.clone(),
        _ => panic!("paragraph"),
    };
    properties.style = Some(id("Listed"));
    e.transact(
        3,
        &[Edit::ParagraphProperties {
            at: Address::body(0),
            properties: Box::new(properties),
        }],
    )
    .expect("use the listed style");
    Operations::new(&mut e, OperationLimits::default())
        .remove_list(4, &[Address::body(0)])
        .expect("remove");
    assert_eq!(
        numbering(e.document(), 0),
        Some(NumPr {
            num_id: Some(NumId(0)),
            ilvl: None,
        })
    );
}
