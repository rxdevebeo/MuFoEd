//! Format-independent policy for a model's answer and its provenance style.
use crate::RECOVERY_STYLE_NAME;
use strict_ooxml_wml::model::block::Paragraph;
use strict_ooxml_wml::model::ids::StyleId;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent, TextNode};
use strict_ooxml_wml::model::props::{ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::values::{Rsids, Space};
use strict_ooxml_wml::model::Document;
pub(crate) const RECOVERED_STYLE_ID: &str = "Recovered";
/// A model's answer as paragraphs, one per line it gave.
///
/// A vision model returns one line per visual line, so splitting on newlines is
/// the only rule available — and it is the reason the report says the breaks are
/// the model's. Empty lines are dropped: a model pads its answer, and an empty
/// paragraph in a document is a thing a reader stumbles over.
///
/// The indent is the **region's** left edge, which is a number the PDF stated.
/// A recovered paragraph therefore sits where the picture it came from sat, which
/// is the one piece of geometry a model cannot give us and the reader already
/// knows.
pub(crate) fn paragraphs_of(text: &str, left: f64) -> Vec<Paragraph> {
    let style = StyleId::new(RECOVERED_STYLE_ID);
    let indent = strict_ooxml_wml::model::values::Indentation {
        start: Some(strict_ooxml_wml::model::values::Twips(crate::to_twips(
            left,
        ))),
        ..strict_ooxml_wml::model::values::Indentation::default()
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| Paragraph {
            props: ParagraphProperties {
                style: Some(style.clone()),
                indentation: Some(indent),
                ..ParagraphProperties::default()
            },
            inlines: vec![Inline::Run(Run {
                props: RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: line.to_owned(),
                    space: Space::default(),
                })],
                revision: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            })],
            rsids: Rsids::default(),
            para_id: None,
            text_id: None,
            revision: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        })
        .collect()
}

/// A model's answer as paragraphs: its words repaired by `lexicon`, its
/// paragraphs tagged with `language`, and how many words were repaired.
pub(crate) fn recovered_paragraphs(
    text: &str,
    left: f64,
    lexicon: Option<&strict_ooxml_ocr::Lexicon>,
    language: Option<&str>,
) -> (Vec<Paragraph>, usize) {
    let (text, restored) = match lexicon {
        Some(lexicon) => lexicon.restore_text(text),
        None => (text.to_owned(), 0),
    };
    let mut paragraphs = paragraphs_of(&text, left);
    if let Some(language) = language.map(str::trim).filter(|tag| !tag.is_empty()) {
        for paragraph in &mut paragraphs {
            let tag = language_of(language, &paragraph_text(paragraph));
            for inline in &mut paragraph.inlines {
                if let Inline::Run(run) = inline {
                    run.props.language = Some(strict_ooxml_wml::model::props::Language {
                        val: Some(std::sync::Arc::from(tag.as_str())),
                        ..strict_ooxml_wml::model::props::Language::default()
                    });
                }
            }
        }
    }
    (paragraphs, restored)
}

/// Records how many words of a page the lexicon repaired.
pub(crate) fn record_restored(
    report: &mut crate::report::ConversionReport,
    page: usize,
    restored: usize,
    lexicon: Option<&strict_ooxml_ocr::Lexicon>,
) {
    let Some(lexicon) = lexicon.filter(|_| restored > 0) else {
        return;
    };
    report.record(
        "text.recovery.lexicon",
        crate::report::Severity::Recovered,
        format!(
            "page {page}: {restored} recovered word(s) given back diacritics the model dropped, \
             from the {} word list",
            lexicon.language()
        ),
    );
}

/// The tag a paragraph of `language` gets: for Serbian, which is printed in
/// Cyrillic and in Latin, the script the paragraph's letters are in
/// (`sr-Cyrl-RS`, `sr-Latn-RS`); otherwise the tag as given.
pub(crate) fn language_of(language: &str, text: &str) -> String {
    let lower = language.to_ascii_lowercase();
    let serbian = lower == "sr" || lower == "sr-rs";
    if !serbian {
        return language.to_owned();
    }
    let (mut cyrillic, mut latin) = (0usize, 0usize);
    for ch in text.chars().filter(|ch| ch.is_alphabetic()) {
        if ('\u{0400}'..='\u{04FF}').contains(&ch) {
            cyrillic += 1;
        } else if ch.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&ch) {
            latin += 1;
        }
    }
    if cyrillic > latin {
        "sr-Cyrl-RS".to_owned()
    } else {
        "sr-Latn-RS".to_owned()
    }
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(run.content.iter().filter_map(|content| match content {
                RunContent::Text(node) => Some(node.text.as_str()),
                _ => None,
            })),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Declares the recovered-paragraph style in a converted document.
///
/// A style with no formatting of its own, so marking a paragraph costs nothing
/// visually — the point is that the name is there for a reader who asks where the
/// text came from.
pub(crate) fn declare_style(document: &mut Document) {
    use std::sync::Arc;
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_wml::model::ids::StyleId;
    use strict_ooxml_wml::model::props::{
        CellProperties, ParagraphProperties, RowProperties, RunProperties, TableProperties,
    };
    use strict_ooxml_wml::model::styles::Style;
    use strict_ooxml_wml::model::values::{StyleType, TableLook};
    document.styles.insert(Style {
        id: StyleId::new(RECOVERED_STYLE_ID),
        style_type: StyleType::Paragraph,
        name: Some(Arc::from(RECOVERY_STYLE_NAME)),
        aliases: None,
        based_on: None,
        next: None,
        link: None,
        is_default: false,
        custom_style: false,
        auto_redefine: false,
        semi_hidden: false,
        hidden: false,
        q_format: false,
        locked: false,
        unhide_when_used: false,
        ui_priority: None,
        table: TableProperties {
            look: Some(TableLook::default()),
            ..TableProperties::default()
        },
        row: RowProperties::default(),
        cell: CellProperties::default(),
        paragraph: ParagraphProperties::default(),
        run: RunProperties::default(),
        conditions: Vec::new(),
        based_on_chain: Vec::new(),
        location: SourceLocation::unknown(),
    });
}
