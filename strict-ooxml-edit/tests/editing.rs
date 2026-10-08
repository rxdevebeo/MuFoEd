//! Independent behavioral regressions for the E01 editing contract.
//!
//! `EditSession` is deprecated in favour of `Editor`; it is a wrapper now, and
//! these tests hold the wrapper to the contract it always had.
#![allow(deprecated)]
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Command, EditError, EditLimits, EditSession, FormatPatch, Invariant, Unsupported,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Block, Document, Inline, ParaId, RunContent, StyleId, TriState};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn doc(body: &str) -> Document {
    let xml = format!("<w:document xmlns:w=\"http://purl.oclc.org/ooxml/wordprocessingml/main\"><w:body>{body}</w:body></w:document>");
    let bytes = DocxBuilder::strict()
        .part("word/document.xml", xml.into_bytes())
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    parse_document(&package, &ParseOptions::default()).unwrap()
}
fn simple() -> Document {
    doc("<w:p><w:r><w:t>abc</w:t></w:r></w:p><w:p><w:r><w:t>safe</w:t></w:r></w:p>")
}
fn text(d: &Document, index: usize) -> String {
    let Block::Paragraph(p) = &d.body.blocks[index] else {
        panic!("paragraph")
    };
    p.inlines
        .iter()
        .flat_map(|i| match i {
            Inline::Run(r) => r.content.iter(),
            _ => panic!("run"),
        })
        .map(|c| match c {
            RunContent::Text(t) => t.text.as_str(),
            _ => panic!("text"),
        })
        .collect()
}
fn replace(paragraph: usize, range: std::ops::Range<usize>, value: &str) -> Command {
    Command::ReplaceText {
        paragraph,
        range,
        text: value.into(),
    }
}
#[test]
fn unicode_replacement_across_runs_preserves_neighbors() {
    let mut d = doc("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>А🙂</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>Бв</w:t></w:r></w:p>");
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    let change = s.transact(0, &[replace(0, 1..3, "XY")]).unwrap();
    assert_eq!(text(s.document(), 0), "АXYв");
    let Block::Paragraph(p) = &s.document().body.blocks[0] else {
        unreachable!()
    };
    let Inline::Run(left) = &p.inlines[0] else {
        unreachable!()
    };
    let Inline::Run(right) = p.inlines.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(left.props.bold, TriState::On);
    assert_eq!(right.props.italic, TriState::On);
    assert_eq!(change.paragraphs, [0]);
    assert!(change.invalidate_support && s.support_is_stale());
}
#[test]
fn format_only_requested_range_and_undo_exactly() {
    let mut d = simple();
    let before = d.body.clone();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    s.transact(
        0,
        &[Command::Format {
            paragraph: 0,
            range: 1..2,
            patch: FormatPatch {
                bold: Some(TriState::On),
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let Block::Paragraph(p) = &s.document().body.blocks[0] else {
        unreachable!()
    };
    assert_eq!(p.inlines.len(), 3);
    for (i, inline) in p.inlines.iter().enumerate() {
        let Inline::Run(r) = inline else {
            unreachable!()
        };
        assert_eq!(
            r.props.bold,
            if i == 1 {
                TriState::On
            } else {
                TriState::Absent
            }
        );
    }
    s.undo(1).unwrap();
    assert_eq!(s.document().body, before);
    s.redo(2).unwrap();
    assert_eq!(text(s.document(), 0), "abc");
}
#[test]
fn transaction_failure_leaves_document_and_history_intact() {
    let mut d = simple();
    let before = d.body.clone();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    let result = s.transact(0, &[replace(0, 0..1, "X"), replace(1, 0..99, "Y")]);
    assert_eq!(result, Err(EditError::InvalidRange));
    assert_eq!(s.document().body, before);
    assert_eq!(s.revision(), 0);
    assert_eq!(s.undo(0), Err(EditError::EmptyHistory));
}
#[test]
fn undo_redo_branch_and_noop() {
    let mut d = simple();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    s.transact(0, &[replace(0, 0..1, "X"), replace(1, 0..4, "other")])
        .unwrap();
    assert_eq!(text(s.document(), 0), "Xbc");
    s.undo(1).unwrap();
    assert_eq!(text(s.document(), 1), "safe");
    let noop = s.transact(2, &[replace(0, 0..3, "abc")]).unwrap();
    assert!(noop.paragraphs.is_empty());
    s.redo(2).unwrap();
    assert_eq!(text(s.document(), 1), "other");
    s.undo(3).unwrap();
    s.transact(4, &[replace(0, 0..0, "new")]).unwrap();
    assert_eq!(s.redo(5), Err(EditError::EmptyHistory));
    assert_eq!(
        s.transact(0, &[replace(0, 0..0, "bad")]),
        Err(EditError::StaleRevision)
    );
}
#[test]
fn rejects_controls_and_complex_content_without_mutation() {
    let mut d = doc("<w:p><w:r><w:t>abc</w:t><w:tab/></w:r></w:p>");
    let before = d.body.clone();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    assert_eq!(
        s.transact(0, &[replace(0, 0..1, "x")]),
        Err(EditError::UnsupportedContent(Unsupported::RunContent))
    );
    assert_eq!(s.document().body, before);
    drop(s);
    let mut d = simple();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    for invalid in ["\0", "\n", "\r", "\t"] {
        assert_eq!(
            s.transact(0, &[replace(0, 0..0, invalid)]),
            Err(EditError::InvalidText)
        );
    }
}
#[test]
fn enforces_history_and_text_limits() {
    let mut d = simple();
    let mut s = EditSession::new(
        &mut d,
        EditLimits {
            history_transactions: 1,
            paragraph_scalars: 5,
            ..EditLimits::default()
        },
    )
    .unwrap();
    s.transact(0, &[replace(0, 0..1, "X")]).unwrap();
    s.transact(1, &[replace(0, 0..1, "Y")]).unwrap();
    assert_eq!(
        s.transact(2, &[replace(0, 0..0, "long")]),
        Err(EditError::LimitExceeded)
    );
    s.undo(2).unwrap();
    assert_eq!(text(s.document(), 0), "Xbc");
    assert_eq!(s.undo(3), Err(EditError::EmptyHistory));
}

#[test]
fn sequential_commands_use_current_offsets_and_empty_paragraph_can_be_edited() {
    let mut d = doc("<w:p/>");
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    s.transact(0, &[replace(0, 0..0, "🙂xy"), replace(0, 1..2, "Ж")])
        .unwrap();
    assert_eq!(text(s.document(), 0), "🙂Жy");
    s.transact(1, &[replace(0, 0..3, "")]).unwrap();
    assert_eq!(text(s.document(), 0), "");
    s.undo(2).unwrap();
    assert_eq!(text(s.document(), 0), "🙂Жy");
}

#[test]
fn insertion_at_start_inherits_right_and_untouched_metadata_is_preserved() {
    let mut d = doc("<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>stay</w:t><w:tab/></w:r></w:p>");
    let neighbor = d.body.blocks[1].clone();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    s.transact(0, &[replace(0, 0..0, " x ")]).unwrap();
    assert_eq!(text(s.document(), 0), " x a");
    let Block::Paragraph(p) = &s.document().body.blocks[0] else {
        unreachable!()
    };
    let Inline::Run(r) = &p.inlines[0] else {
        unreachable!()
    };
    assert_eq!(r.props.italic, TriState::On);
    assert_eq!(s.document().body.blocks[1], neighbor);
}

#[test]
fn failed_edit_preserves_redo_and_repeated_format_is_noop() {
    let mut d = simple();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    let command = Command::Format {
        paragraph: 0,
        range: 0..2,
        patch: FormatPatch {
            italic: Some(TriState::On),
            ..Default::default()
        },
    };
    s.transact(0, std::slice::from_ref(&command)).unwrap();
    let after = s.document().body.clone();
    let result = s.transact(1, std::slice::from_ref(&command)).unwrap();
    assert_eq!(result.revision, 1);
    assert!(result.paragraphs.is_empty());
    assert_eq!(s.document().body, after);
    s.undo(1).unwrap();
    assert_eq!(
        s.transact(2, &[replace(0, std::ops::Range { start: 5, end: 1 }, "x")]),
        Err(EditError::InvalidRange)
    );
    s.redo(2).unwrap();
    assert_eq!(s.document().body, after);
}

#[test]
fn model_validation_rejects_duplicate_ids_missing_style_and_wrong_sections() {
    let mut d = simple();
    for b in &mut d.body.blocks {
        let Block::Paragraph(p) = b else {
            unreachable!()
        };
        p.para_id = Some(ParaId::new("0000000A"));
    }
    assert!(matches!(
        EditSession::new(&mut d, EditLimits::default()),
        Err(EditError::InvalidModel(Invariant::DuplicateId))
    ));
    let mut d = simple();
    let Block::Paragraph(p) = &mut d.body.blocks[0] else {
        unreachable!()
    };
    p.props.style = Some(StyleId::new("missing"));
    assert!(matches!(
        EditSession::new(&mut d, EditLimits::default()),
        Err(EditError::UnknownStyle(id)) if id.as_str() == "missing"
    ));
    let mut d = simple();
    let Block::Paragraph(p) = &mut d.body.blocks[0] else {
        unreachable!()
    };
    p.props.section = Some(strict_ooxml_wml::model::SectionProperties::default());
    d.sections.clear();
    assert!(matches!(
        EditSession::new(&mut d, EditLimits::default()),
        Err(EditError::InvalidModel(Invariant::Sections))
    ));
}

#[test]
fn missing_character_style_and_missing_paragraph_are_atomic_errors() {
    let mut d = simple();
    let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
    let before = s.document().body.clone();
    assert_eq!(
        s.transact(
            0,
            &[Command::Format {
                paragraph: 0,
                range: 0..1,
                patch: FormatPatch {
                    character_style: Some(Some(StyleId::new("absent"))),
                    ..Default::default()
                }
            }]
        ),
        Err(EditError::UnknownStyle(StyleId::new("absent")))
    );
    assert_eq!(
        s.transact(0, &[replace(100, 0..0, "x")]),
        Err(EditError::InvalidParagraph)
    );
    assert_eq!(s.document().body, before);
}

#[test]
fn edited_model_writes_and_reopens_with_exact_text() {
    let mut d = simple();
    {
        let mut s = EditSession::new(&mut d, EditLimits::default()).unwrap();
        s.transact(0, &[replace(0, 1..2, " 🙂 ")]).unwrap();
    }
    let written =
        strict_ooxml_write::write_package(&d, None, &strict_ooxml_write::WriteOptions::default())
            .unwrap();
    let package = Package::open_reader(&written.bytes[..], &OpenOptions::default()).unwrap();
    let reopened = parse_document(&package, &ParseOptions::default()).unwrap();
    assert_eq!(text(&reopened, 0), "a 🙂 c");
    assert_eq!(text(&reopened, 1), "safe");
}

#[test]
fn all_small_unicode_ranges_match_independent_splice_and_exact_undo() {
    for left in ["", "a", "🙂Б"] {
        for right in ["", "x", "Ж🙂"] {
            let mut base = doc(&format!("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>{left}</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>{right}</w:t></w:r></w:p>"));
            // Preserve empty runs as well: they are meaningful history metadata.
            let input: Vec<char> = format!("{left}{right}").chars().collect();
            for start in 0..=input.len() {
                for end in start..=input.len() {
                    for replacement in ["", "z", "🙂 "] {
                        let before = base.body.clone();
                        let mut expected = input.clone();
                        expected.splice(start..end, replacement.chars());
                        let expected: String = expected.into_iter().collect();
                        let mut session =
                            EditSession::new(&mut base, EditLimits::default()).unwrap();
                        let change = session
                            .transact(0, &[replace(0, start..end, replacement)])
                            .unwrap();
                        assert_eq!(
                            text(session.document(), 0),
                            expected,
                            "{left:?}/{right:?} {start}..{end} {replacement:?}"
                        );
                        if !change.paragraphs.is_empty() {
                            session.undo(1).unwrap();
                        }
                        assert_eq!(session.document().body, before);
                    }
                }
            }
        }
    }
}

#[test]
fn explicit_off_and_inheritance_are_distinct_and_empty_history_is_disabled() {
    let mut d = doc("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>abc</w:t></w:r></w:p>");
    let mut s = EditSession::new(
        &mut d,
        EditLimits {
            history_transactions: 0,
            ..Default::default()
        },
    )
    .unwrap();
    for (revision, state) in [(0, TriState::Off), (1, TriState::Absent)] {
        s.transact(
            revision,
            &[Command::Format {
                paragraph: 0,
                range: 0..3,
                patch: FormatPatch {
                    bold: Some(state),
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let Block::Paragraph(p) = &s.document().body.blocks[0] else {
            unreachable!()
        };
        let Inline::Run(r) = &p.inlines[0] else {
            unreachable!()
        };
        assert_eq!(r.props.bold, state);
    }
    assert_eq!(s.undo(2), Err(EditError::EmptyHistory));
}
