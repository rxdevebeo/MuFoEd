//! Read -> write -> read keeps every paragraph's text, for any text XML can
//! carry: markup characters, tabs inside runs, leading and trailing spaces,
//! every script and astral-plane characters.

use std::fmt::Write as _;

use proptest::prelude::*;
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::{Block, Document};
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

/// Characters XML 1.0 carries in character data, weighted toward the ones a
/// serializer gets wrong.
fn text_char() -> impl Strategy<Value = char> {
    prop_oneof![
        3 => prop::sample::select(vec!['<', '>', '&', '"', '\'', ' ', ']']),
        4 => prop::char::range('a', 'z'),
        2 => prop::sample::select(vec!['é', 'Ж', 'ש', 'ع', '中', 'ह', '\u{a0}', '\u{200d}']),
        1 => prop::sample::select(vec!['😀', '𝔸', '\u{10ffff}', '\u{fffd}']),
    ]
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(text_char(), 1..40).prop_map(|chars| chars.into_iter().collect())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text of each body paragraph, runs concatenated.
fn paragraphs(document: &Document) -> Vec<String> {
    document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .map(|paragraph| {
            let mut out = String::new();
            for inline in &paragraph.inlines {
                if let Inline::Run(run) = inline {
                    for content in &run.content {
                        if let RunContent::Text(node) = content {
                            out.push_str(&node.text);
                        }
                    }
                }
            }
            out
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn paragraph_text_survives_a_write(texts in prop::collection::vec(text(), 1..6)) {
        let mut body = String::new();
        for text in &texts {
            let _ = write!(
                body,
                "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escape(text)
            );
        }
        let bytes = DocxBuilder::strict().body(&body).build();
        let package = Package::open_reader(bytes.as_slice(), &OpenOptions::default())
            .expect("open source");
        let source = parse_document(&package, &ParseOptions::default()).expect("parse source");
        prop_assert_eq!(paragraphs(&source), texts.clone());

        let written = write_package(&source, Some(&package), &WriteOptions::default())
            .expect("write");
        let reopened = Package::open_reader(written.bytes.as_slice(), &OpenOptions::default())
            .expect("reopen");
        let again = parse_document(&reopened, &ParseOptions::default()).expect("reparse");
        prop_assert_eq!(paragraphs(&again), texts);
    }
}
