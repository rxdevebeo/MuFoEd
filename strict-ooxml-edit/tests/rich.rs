//! K06 (second part): `Edit::SplitAt` and joins of paragraphs holding more
//! than plain runs.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{
    Address, Edit, EditError, EditLimits, Editor, FormatPatch, InlinePoint, Unsupported,
};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{Block, Document, Inline, RunContent, TriState};
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

/// The inlines of a paragraph as text: `<...>` around a hyperlink, `{...}`
/// around a content control, `→` for a tab.
fn shape(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                for content in &run.content {
                    match content {
                        RunContent::Text(node) => out.push_str(&node.text),
                        RunContent::Tab => out.push('→'),
                        _ => out.push('·'),
                    }
                }
            }
            Inline::Hyperlink(h) => {
                out.push('<');
                out.push_str(&shape(&h.inlines));
                out.push('>');
            }
            Inline::SdtInline(s) => {
                out.push('{');
                out.push_str(&shape(&s.inlines));
                out.push('}');
            }
            _ => out.push('·'),
        }
    }
    out
}

fn paragraphs(editor: &Editor<'_>) -> Vec<String> {
    editor
        .document()
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(shape(&p.inlines)),
            _ => None,
        })
        .collect()
}

fn point(inline: &[usize], content: usize, offset: usize) -> InlinePoint {
    InlinePoint {
        inline: inline.to_vec(),
        content,
        offset,
    }
}

fn split(inline: &[usize], content: usize, offset: usize) -> Edit {
    Edit::SplitAt {
        at: Address::body(0),
        point: point(inline, content, offset),
    }
}

const LINKED: &str = "<w:p><w:r><w:t xml:space=\"preserve\">see </w:t></w:r>\
                      <w:hyperlink w:anchor=\"x\"><w:r><w:t>this link</w:t></w:r></w:hyperlink>\
                      <w:r><w:tab/><w:t>end</w:t></w:r></w:p>";

#[test]
fn a_split_inside_a_hyperlink_continues_it_in_the_next_paragraph() {
    let mut d = doc(LINKED);
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(0, &[split(&[1, 0], 0, 4)]).expect("split");
    assert_eq!(paragraphs(&e), ["see <this>", "< link>→end"]);
    let anchors: Vec<_> = e
        .document()
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => p.inlines.iter().find_map(|inline| match inline {
                Inline::Hyperlink(h) => h.anchor.clone(),
                _ => None,
            }),
            _ => None,
        })
        .collect();
    assert_eq!(anchors.len(), 2);
    assert!(anchors.iter().all(|anchor| &**anchor == "x"));
    let [Block::Paragraph(left), Block::Paragraph(right)] = e.document().body.blocks.as_slice()
    else {
        panic!("two paragraphs");
    };
    assert!(left.para_id.is_some() && right.para_id.is_some());
    assert_ne!(left.para_id, right.para_id);
    assert!(e.validate().is_ok());
    // Joined again, the text is whole; the link stays in two pieces.
    e.transact(
        1,
        &[Edit::Join {
            at: Address::body(0),
        }],
    )
    .expect("join");
    assert_eq!(paragraphs(&e), ["see <this>< link>→end"]);
    e.undo(2).expect("undo join");
    e.undo(3).expect("undo split");
    assert_eq!(e.document().body, original.body);
}

#[test]
fn points_at_the_edges_of_runs_and_around_a_tab() {
    for (point, expected) in [
        // Between the tab and `end`.
        ((vec![2], 1, 0), ["see <this link>→", "end"]),
        // Before the tab.
        ((vec![2], 0, 0), ["see <this link>", "→end"]),
        // At the start and at the end of the paragraph.
        ((vec![0], 0, 0), ["", "see <this link>→end"]),
        ((vec![2], 2, 0), ["see <this link>→end", ""]),
        // After the hyperlink's last text.
        ((vec![1, 0], 0, 9), ["see <this link>", "→end"]),
    ] {
        let mut d = doc(LINKED);
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        let (inline, content, offset) = point;
        e.transact(0, &[split(&inline, content, offset)])
            .expect("split");
        assert_eq!(paragraphs(&e), expected);
        assert!(e.validate().is_ok());
    }
}

#[test]
fn points_that_cannot_be_split_are_refused() {
    let cases = [
        // Into a tab.
        (LINKED, split(&[2], 0, 1), EditError::InvalidRange),
        // Past the text.
        (LINKED, split(&[0], 0, 5), EditError::InvalidRange),
        // A path that does not end at a run.
        (
            LINKED,
            split(&[1], 0, 0),
            EditError::UnsupportedContent(Unsupported::Target),
        ),
        // The result of a complex field.
        (
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
             <w:r><w:instrText>PAGE</w:instrText></w:r>\
             <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
             <w:r><w:t>12</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            split(&[3], 0, 1),
            EditError::UnsupportedContent(Unsupported::FieldResult),
        ),
        // A simple field.
        (
            "<w:p><w:fldSimple w:instr=\"PAGE\"><w:r><w:t>12</w:t></w:r></w:fldSimple></w:p>",
            split(&[0, 0], 0, 1),
            EditError::UnsupportedContent(Unsupported::FieldResult),
        ),
        // An inline content control.
        (
            "<w:p><w:sdt><w:sdtContent><w:r><w:t>ab</w:t></w:r></w:sdtContent></w:sdt></w:p>",
            split(&[0, 0], 0, 1),
            EditError::UnsupportedContent(Unsupported::Indivisible),
        ),
        // Inside a tracked insertion.
        (
            "<w:p><w:ins w:id=\"1\" w:author=\"a\"><w:r><w:t>ab</w:t></w:r></w:ins></w:p>",
            split(&[0], 0, 1),
            EditError::UnsupportedContent(Unsupported::TrackedChange),
        ),
    ];
    for (body, command, error) in cases {
        let mut d = doc(body);
        let original = d.clone();
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        assert_eq!(e.transact(0, &[command]), Err(error), "{body}");
        assert_eq!(e.document().body, original.body);
    }
}

#[test]
fn a_complex_field_ended_before_the_point_and_tracked_run_edges_split() {
    let field = "<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>\
                 <w:r><w:instrText>PAGE</w:instrText></w:r>\
                 <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>\
                 <w:r><w:t>12</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>\
                 <w:r><w:t>after</w:t></w:r></w:p>";
    let mut d = doc(field);
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(0, &[split(&[5], 0, 2)])
        .expect("after the field");
    assert_eq!(paragraphs(&e), ["···12·af", "ter"]);
    let tracked = "<w:p><w:ins w:id=\"1\" w:author=\"a\"><w:r><w:t>ab</w:t></w:r></w:ins></w:p>";
    for (offset, expected) in [(0, ["", "ab"]), (2, ["ab", ""])] {
        let mut d = doc(tracked);
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        e.transact(0, &[split(&[0], 0, offset)])
            .expect("edge of a tracked run");
        assert_eq!(paragraphs(&e), expected);
    }
}

#[test]
fn paragraphs_with_tabs_links_and_controls_join() {
    let mut d = doc("<w:p><w:r><w:t>a</w:t><w:tab/></w:r></w:p>\
         <w:p><w:hyperlink w:anchor=\"x\"><w:r><w:t>b</w:t></w:r></w:hyperlink>\
         <w:sdt><w:sdtContent><w:r><w:t>c</w:t></w:r></w:sdtContent></w:sdt></w:p>");
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(
        0,
        &[Edit::Join {
            at: Address::body(0),
        }],
    )
    .expect("join");
    assert_eq!(paragraphs(&e), ["a→<b>{c}"]);
    // The size limit still holds.
    let mut d = doc("<w:p><w:r><w:t>ab</w:t></w:r></w:p><w:p><w:r><w:t>cd</w:t></w:r></w:p>");
    let mut e = Editor::new(
        &mut d,
        EditLimits {
            paragraph_scalars: 3,
            ..EditLimits::default()
        },
    )
    .expect("editor");
    assert_eq!(
        e.transact(
            0,
            &[Edit::Join {
                at: Address::body(0),
            }],
        ),
        Err(EditError::LimitExceeded)
    );
}

fn bold(from: InlinePoint, to: InlinePoint) -> Edit {
    Edit::FormatAt {
        at: Address::body(0),
        from,
        to,
        patch: FormatPatch {
            bold: Some(TriState::On),
            ..FormatPatch::default()
        },
    }
}

/// The paragraph's text with bold content in `*...*`, wrappers as in
/// [`shape`].
fn bolded(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                let on = run.props.bold == TriState::On;
                if on {
                    out.push('*');
                }
                out.push_str(&shape(std::slice::from_ref(inline)));
                if on {
                    out.push('*');
                }
            }
            Inline::Hyperlink(h) => {
                out.push('<');
                out.push_str(&bolded(&h.inlines));
                out.push('>');
            }
            _ => out.push_str(&shape(std::slice::from_ref(inline))),
        }
    }
    out
}

fn first(editor: &Editor<'_>) -> String {
    match editor.document().body.blocks.first() {
        Some(Block::Paragraph(p)) => bolded(&p.inlines),
        _ => String::new(),
    }
}

#[test]
fn a_range_across_a_hyperlink_and_a_tab_is_formatted_without_dividing_the_link() {
    let mut d = doc(LINKED);
    let original = d.clone();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    // From "s|ee " to "e|nd": the link stays one link.
    e.transact(0, &[bold(point(&[0], 0, 1), point(&[2], 1, 1))])
        .expect("format");
    assert_eq!(first(&e), "s*ee *<*this link*>*→e*nd");
    let Some(Block::Paragraph(p)) = e.document().body.blocks.first() else {
        panic!("paragraph");
    };
    assert_eq!(
        p.inlines
            .iter()
            .filter(|inline| matches!(inline, Inline::Hyperlink(_)))
            .count(),
        1
    );
    assert!(e.validate().is_ok());
    // Inside the link only.
    e.undo(1).expect("undo");
    assert_eq!(e.document().body, original.body);
    e.transact(2, &[bold(point(&[1, 0], 0, 5), point(&[1, 0], 0, 9))])
        .expect("inside the link");
    assert_eq!(first(&e), "see <this *link*>→end");
    // Formatting what is bold already changes nothing.
    let revision = e.revision();
    e.transact(
        revision,
        &[bold(point(&[1, 1], 0, 0), point(&[1, 1], 0, 4))],
    )
    .expect("again");
    assert_eq!(e.revision(), revision);
}

#[test]
fn a_bad_range_is_refused() {
    let tracked = "<w:p><w:r><w:t>ab</w:t></w:r>\
                   <w:ins w:id=\"1\" w:author=\"a\"><w:r><w:t>cd</w:t></w:r></w:ins></w:p>";
    let cases = [
        // Backwards.
        (
            LINKED,
            bold(point(&[2], 0, 0), point(&[0], 0, 1)),
            EditError::InvalidRange,
        ),
        // A point that is not in a run.
        (
            LINKED,
            bold(point(&[1], 0, 0), point(&[2], 0, 0)),
            EditError::UnsupportedContent(Unsupported::Target),
        ),
        // Into a tab.
        (
            LINKED,
            bold(point(&[0], 0, 0), point(&[2], 0, 1)),
            EditError::InvalidRange,
        ),
        // Over a tracked run.
        (
            tracked,
            bold(point(&[0], 0, 1), point(&[1], 0, 1)),
            EditError::UnsupportedContent(Unsupported::TrackedChange),
        ),
    ];
    for (body, command, error) in cases {
        let mut d = doc(body);
        let original = d.clone();
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        assert_eq!(e.transact(0, &[command]), Err(error), "{body}");
        assert_eq!(e.document().body, original.body);
    }
    // Up to the tracked run, not into it, is fine.
    let mut d = doc(tracked);
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.transact(0, &[bold(point(&[0], 0, 1), point(&[1], 0, 0))])
        .expect("up to the tracked run");
    assert_eq!(first(&e), "a*b*cd");
}
