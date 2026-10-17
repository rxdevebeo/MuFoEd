//! Public HTML model, resource and budget contracts.
use strict_ooxml_html::{import, HtmlOptions};
use strict_ooxml_wml::model::{Block, Inline, RunContent, TriState};
fn png() -> Vec<u8> {
    let mut out = vec![];
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
    }
    out
}
fn text(blocks: &[Block]) -> String {
    let mut s = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &p.inlines {
                    if let Inline::Run(r) = inline {
                        for c in &r.content {
                            if let RunContent::Text(t) = c {
                                s.push_str(&t.text);
                            }
                        }
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        s.push_str(&text(&c.blocks));
                    }
                }
            }
            _ => {}
        }
    }
    s
}

#[test]
fn unicode_entities_and_nested_formatting_are_modeled() {
    let result=import("<p style='text-align:center'>Привет &amp; <b>мир <i>🙂</i></b> <span style='font-size:18pt;color:#123456'>x</span></p>".as_bytes(),&HtmlOptions::default(),None).unwrap();
    assert_eq!(text(&result.document.body.blocks), "Привет & мир 🙂 x");
    assert_eq!(result.report.loss_count, 0);
    let p = result.document.body.blocks[0].as_paragraph().unwrap();
    assert_eq!(
        p.props.alignment,
        Some(strict_ooxml_wml::model::Justification::Center)
    );
    let runs: Vec<_> = p
        .inlines
        .iter()
        .filter_map(|i| {
            if let Inline::Run(r) = i {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    assert!(runs
        .iter()
        .any(|r| r.props.bold == TriState::On && r.props.italic == TriState::On));
    assert!(runs.iter().any(|r| r.props.size.is_some_and(|s| s.0 == 36)
        && r.props
            .color
            .as_ref()
            .is_some_and(|c| c.as_str() == "123456")));
}
#[test]
fn data_and_relative_images_keep_bytes_and_order() {
    use base64::Engine as _;
    let bytes = png();
    let source=format!("<p>before<img width='20' src='data:image/png;base64,{}'>between<img src='image.png'>after</p>",base64::engine::general_purpose::STANDARD.encode(&bytes));
    let options = HtmlOptions {
        base_url: Some("https://example.test/docs/page".into()),
        ..Default::default()
    };
    let resolver = |url: &str, max: usize| {
        assert_eq!(url, "https://example.test/docs/image.png");
        assert!(max >= bytes.len());
        Ok(bytes.clone())
    };
    let result = import(source.as_bytes(), &options, Some(&resolver)).unwrap();
    assert_eq!(result.report.loss_count, 0);
    assert_eq!(result.media.len(), 2);
    assert_eq!(result.media[0].1, bytes);
    let p = result.document.body.blocks[0].as_paragraph().unwrap();
    assert!(matches!(&p.inlines[1], Inline::Drawing(_)));
    assert!(matches!(&p.inlines[3], Inline::Drawing(_)));
    assert_eq!(text(&result.document.body.blocks), "beforebetweenafter");
}
#[test]
fn lists_and_table_cells_are_structural() {
    let result=import(b"<ol start='3'><li>one<li>two<ul><li>nested</li></ul></li></ol><table><tr><th>A</th><td>B</td></tr><tr><td colspan='2'>C</td></tr></table>",&HtmlOptions::default(),None).unwrap();
    assert_eq!(result.document.numbering.nums().count(), 2);
    assert_eq!(text(&result.document.body.blocks), "onetwonestedABC");
    let table = result
        .document
        .body
        .blocks
        .iter()
        .find_map(|b| {
            if let Block::Table(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.grid.len(), 2);
    assert_eq!(table.rows[1].cells[0].props.grid_span, Some(2));
}
#[test]
fn losses_are_named_and_scripts_are_not_imported() {
    let result=import(b"<script>if(a<b){x='bad'}</script><p style='filter:blur(2px)'>ok<img src='missing.png' alt='ALT'></p>",&HtmlOptions::default(),None).unwrap();
    assert_eq!(text(&result.document.body.blocks), "okALT");
    for name in ["element:script", "css:filter", "image:unavailable"] {
        assert!(
            result.report.issues.iter().any(|i| i.feature == name),
            "{name}: {:?}",
            result.report
        );
    }
}
#[test]
fn invalid_encoding_and_budgets_fail_before_returning_a_partial_document() {
    assert!(import(&[255], &HtmlOptions::default(), None).is_err());
    for options in [
        HtmlOptions {
            max_input_bytes: 3,
            ..Default::default()
        },
        HtmlOptions {
            max_nodes: 1,
            ..Default::default()
        },
        HtmlOptions {
            max_depth: 1,
            ..Default::default()
        },
    ] {
        assert!(import(b"<div><p>text</p></div>", &options, None).is_err());
    }
    let options = HtmlOptions {
        max_image_pixels: 1,
        ..Default::default()
    };
    let bytes = png();
    let resolver = |_: &str, _: usize| Ok(bytes.clone());
    assert!(import(
        b"<img src='https://example.test/p.png'>",
        &options,
        Some(&resolver)
    )
    .is_err());
}

#[test]
fn failed_images_also_count_towards_the_image_limit() {
    let options = HtmlOptions {
        max_images: 1,
        ..Default::default()
    };
    let calls = std::cell::Cell::new(0);
    let resolver = |_: &str, _: usize| {
        calls.set(calls.get() + 1);
        Err("missing".into())
    };
    assert!(import(b"<img src='one'><img src='two'>", &options, Some(&resolver)).is_err());
    assert_eq!(calls.get(), 1);
}

#[test]
fn preformatted_line_breaks_and_tabs_are_wml_controls() {
    let result = import(b"<pre>A\tB\nC</pre>", &HtmlOptions::default(), None).unwrap();
    let p = result.document.body.blocks[0].as_paragraph().unwrap();
    let controls: Vec<_> = p
        .inlines
        .iter()
        .filter_map(|i| {
            if let Inline::Run(r) = i {
                Some(&r.content)
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert!(controls.iter().any(|c| matches!(c, RunContent::Tab)));
    assert!(controls.iter().any(|c| matches!(c, RunContent::Break(_))));
}
