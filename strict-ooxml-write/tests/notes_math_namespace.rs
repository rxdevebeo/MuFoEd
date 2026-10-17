//! A notes part that holds Office Math declares the `m` namespace.
//!
//! `EG_BlockLevelElts` reaches `m:oMath` in a footnote and an endnote, so a note
//! body can carry OMML. The notes part used to offer only `w`/`r`/`wp`/`a`/`pic`
//! to `start_root`, so a footnote with inline math wrote `m:` elements into a
//! root that never declared `xmlns:m` and the part did not parse (`unbound
//! namespace prefix 'm'`). Found by importing a LaTeX `\footnote` with `$...$`.

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::normalize::report::NormalizationReport;
use strict_ooxml_wml::model::{
    Block, Inline, MathExpression, MathNode, MathRun, MathRunProperties, Note, NoteKind, NoteTable,
    Paragraph, ParagraphProperties, Rsids, Run, RunContent, RunProperties, Space, TextNode,
};
use strict_ooxml_write::ctx::Ctx;
use strict_ooxml_write::parts::notes_part;

fn note_with_math() -> Note {
    let expression = MathExpression {
        nodes: vec![MathNode::Run(MathRun {
            field_char: None,
            properties: MathRunProperties::default(),
            run_properties: None,
            text: "x".to_owned(),
            location: SourceLocation::default(),
        })],
        location: SourceLocation::default(),
    };
    let paragraph = Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Math(expression)],
        rsids: Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::default(),
    };
    Note {
        id: 1,
        kind: NoteKind::Normal,
        blocks: vec![Block::Paragraph(paragraph)],
        location: SourceLocation::default(),
    }
}

fn note_with_text() -> Note {
    let paragraph = Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![Inline::Run(Run {
            props: RunProperties::default(),
            content: vec![RunContent::Text(TextNode {
                text: "plain".to_owned(),
                space: Space::default(),
            })],
            revision: None,
            location: SourceLocation::default(),
        })],
        rsids: Rsids::default(),
        revision: None,
        para_id: None,
        text_id: None,
        location: SourceLocation::default(),
    };
    Note {
        id: 1,
        kind: NoteKind::Normal,
        blocks: vec![Block::Paragraph(paragraph)],
        location: SourceLocation::default(),
    }
}

fn render(note: Note, is_footnote: bool) -> String {
    let mut table = NoteTable::new();
    table.insert(note);
    let mut report = NormalizationReport::default();
    let mut ctx = Ctx::new(&mut report);
    notes_part(&mut ctx, &table, is_footnote).expect("notes part")
}

#[test]
fn a_footnote_holding_math_declares_the_math_namespace() {
    let xml = render(note_with_math(), true);
    assert!(
        xml.contains(r#"xmlns:m="http://purl.oclc.org/ooxml/officeDocument/math""#),
        "{xml}"
    );
    assert!(xml.contains("<m:oMath>"), "{xml}");
}

#[test]
fn an_endnote_holding_math_declares_the_math_namespace() {
    let xml = render(note_with_math(), false);
    assert!(
        xml.contains(r#"xmlns:m="http://purl.oclc.org/ooxml/officeDocument/math""#),
        "{xml}"
    );
    assert!(xml.contains("<m:oMath>"), "{xml}");
}

#[test]
fn a_note_without_math_declares_no_unused_math_namespace() {
    // The declaration is filtered to the prefixes the part used, so the `m`
    // entry in the offered list must not leak into a note that has no math.
    let xml = render(note_with_text(), true);
    assert!(!xml.contains("xmlns:m="), "{xml}");
}
