//! Public facade keeps the source and live model coherent through edits.
#![cfg(feature = "edit")]
use strict_ooxml::{
    edit::{Address, Edit, EditLimits, SavePolicy},
    OpenOptions, RenderOptions, StrictDocument, WriteOptions,
};
use strict_ooxml_testkit::DocxBuilder;
#[test]
fn facade_edit_preview_save_refresh_and_reopen() {
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>abc</w:t></w:r></w:p>")
        .build();
    let mut document = StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let saved = {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        editor
            .transact(
                0,
                &[Edit::Text {
                    at: Address::body(0),
                    range: 1..2,
                    text: "🙂".into(),
                }],
            )
            .unwrap();
        let preview = editor.render_svg(&RenderOptions::default()).unwrap();
        assert!(preview.iter().any(|p| p.svg.contains("🙂")));
        editor.visual_map(1, &RenderOptions::default()).unwrap();
        let saved = editor
            .save(1, &WriteOptions::default(), SavePolicy::Lossless)
            .unwrap();
        editor.refresh_support(1, &WriteOptions::default()).unwrap();
        saved
    };
    assert!(!document.support_is_stale());
    let reopened =
        StrictDocument::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    assert!(reopened.render_svg(&RenderOptions::default()).unwrap()[0]
        .svg
        .contains("🙂"));
    {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        editor
            .transact(
                0,
                &[Edit::Split {
                    at: Address::body(0),
                    offset: 1,
                }],
            )
            .unwrap();
    }
    assert!(document.support_is_stale());
    #[cfg(feature = "report")]
    assert!(document
        .support_report()
        .features
        .iter()
        .any(|f| f.feature_id == "edit.support-stale"));
    {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        assert!(editor.support_is_stale());
        editor.refresh_support(0, &WriteOptions::default()).unwrap();
        assert!(!editor.support_is_stale());
    }
    assert!(!document.support_is_stale());
}
#[test]
fn facade_compound_operations_save_and_reopen() {
    use strict_ooxml::edit::{
        OperationLimits, Operations, ReplacePolicy, SearchQuery, SearchScope, TextQuery,
    };
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>one one</w:t></w:r></w:p><w:p><w:r><w:t>tail</w:t></w:r></w:p>")
        .build();
    let mut document = StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let saved = {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        let result = Operations::new(&mut editor, OperationLimits::default())
            .replace_all(
                0,
                &SearchScope::All,
                &TextQuery::new("one"),
                "two",
                ReplacePolicy::Strict,
            )
            .unwrap();
        assert_eq!(result.replaced, 2);
        Operations::new(&mut editor, OperationLimits::default())
            .move_block(1, &Address::body(0), &Address::body(2))
            .unwrap();
        editor
            .save(2, &WriteOptions::default(), SavePolicy::Lossless)
            .unwrap()
    };
    let mut reopened =
        StrictDocument::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    let mut editor = reopened.edit(EditLimits::default()).unwrap();
    let found = Operations::new(&mut editor, OperationLimits::default())
        .search(
            0,
            &SearchScope::All,
            &SearchQuery::Text(TextQuery::new("two")),
        )
        .unwrap();
    assert_eq!(found.hits.len(), 2);
    assert_eq!(found.hits[0].paragraph, Address::body(1));
}
fn rich_point(inline: &[usize], content: usize, offset: usize) -> strict_ooxml::edit::InlinePoint {
    strict_ooxml::edit::InlinePoint {
        inline: inline.to_vec(),
        content,
        offset,
    }
}
/// Text with `*` around bold runs, `<...>` around hyperlinks and `→` for
/// a tab.
fn rich_shape(inlines: &[strict_ooxml::model::Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            strict_ooxml::model::Inline::Run(run) => {
                let bold = run.props.bold == strict_ooxml::model::TriState::On;
                if bold {
                    out.push('*');
                }
                for content in &run.content {
                    match content {
                        strict_ooxml::model::RunContent::Text(node) => out.push_str(&node.text),
                        strict_ooxml::model::RunContent::Tab => out.push('→'),
                        _ => out.push('·'),
                    }
                }
                if bold {
                    out.push('*');
                }
            }
            strict_ooxml::model::Inline::Hyperlink(link) => {
                out.push('<');
                rich_shape(&link.inlines, out);
                out.push('>');
            }
            _ => out.push('·'),
        }
    }
}
#[test]
fn rich_split_format_and_replace_save_and_reopen() {
    use strict_ooxml::edit::FormatPatch;
    use strict_ooxml::model::{Block, Inline, TriState};
    use strict_ooxml_testkit::docx::Rel;
    let builder = DocxBuilder::strict();
    let mut link = Rel::new(
        "rIdLink",
        builder.family().rel_type("hyperlink"),
        "https://example.com/",
    );
    link.external = true;
    let bytes = builder
        .rel_uri(link)
        .body(
            "<w:p><w:r><w:t xml:space=\"preserve\">see </w:t></w:r>\
             <w:hyperlink r:id=\"rIdLink\"><w:r><w:t>this link</w:t></w:r></w:hyperlink>\
             <w:r><w:tab/><w:t>end</w:t></w:r></w:p>",
        )
        .build();
    let mut document = StrictDocument::open_reader(&bytes[..], &OpenOptions::default()).unwrap();
    let saved = {
        let mut editor = document.edit(EditLimits::default()).unwrap();
        editor
            .transact(
                0,
                &[
                    // Bold from "s|ee " to "→e|nd".
                    Edit::FormatAt {
                        at: Address::body(0),
                        from: rich_point(&[0], 0, 1),
                        to: rich_point(&[2], 1, 1),
                        patch: FormatPatch {
                            bold: Some(TriState::On),
                            ..FormatPatch::default()
                        },
                    },
                    // Enter in the link: "this| link".
                    Edit::SplitAt {
                        at: Address::body(0),
                        point: rich_point(&[2, 0], 0, 4),
                    },
                    // " link" becomes " page".
                    Edit::ReplaceAt {
                        at: Address::body(1),
                        from: rich_point(&[0, 0], 0, 1),
                        to: rich_point(&[0, 0], 0, 5),
                        text: "page".into(),
                    },
                ],
            )
            .unwrap();
        editor
            .save(1, &WriteOptions::default(), SavePolicy::Lossless)
            .unwrap()
    };
    let reopened =
        StrictDocument::open_reader(saved.bytes.as_slice(), &OpenOptions::default()).unwrap();
    let mut texts = Vec::new();
    let mut targets = Vec::new();
    for block in &reopened.document().body.blocks {
        let Block::Paragraph(p) = block else {
            continue;
        };
        let mut text = String::new();
        rich_shape(&p.inlines, &mut text);
        texts.push(text);
        for inline in &p.inlines {
            if let Inline::Hyperlink(link) = inline {
                targets.push(link.rel_id.clone());
            }
        }
    }
    assert_eq!(texts, ["s*ee *<*this*>", "<* page*>*→e*nd"]);
    // Both halves of the link refer to its one relationship.
    assert_eq!(targets.len(), 2);
    assert!(targets[0].is_some());
    assert_eq!(targets[0], targets[1]);
}
