//! `verify_no_silent_loss` must be able to fail.
//!
//! `STAGE-10-TASK.md` E35 records why this file exists. The gate compares two
//! counters, `removed_nodes` and `reported_nodes`, and until the stage-8 work
//! **every increment of one was an increment of the other**:
//! `Ctx::report_unsupported` called `count_reported_removal`, and nothing
//! anywhere called `count_unreported_removal`. So for a writer's report
//! `removed_nodes == reported_nodes` was an identity, not a finding: the gate
//! could not fail on any input, which made "nothing is lost silently" (SC-10)
//! an assertion rather than a check.
//!
//! Three tests, and the middle one is the point:
//!
//! 1. an unreported removal **fails** the gate - the gate is falsifiable;
//! 2. a reported one passes - the counters mean what they say;
//! 3. the theme placeholder loss is recorded by the writer itself, so a document
//!    that was **built** rather than parsed does not lose it silently.

// Spelling the fields out is the point: this is a whole `Document`, and the
// lint that objects would have me hide which parts of it are default.
#![allow(clippy::default_trait_access)]

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::normalize::report::{LossRecord, NormalizationReport, Severity};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::document::DocumentSource;
use strict_ooxml_wml::model::theme::{FontSet, Theme, ThemeColors, ThemeFonts, ThemeTypeface};
use strict_ooxml_wml::model::Document;
use strict_ooxml_write::{verify_no_silent_loss, write_package, WriteOptions};

fn empty_document() -> Document {
    Document {
        body: Default::default(),
        styles: Default::default(),
        numbering: Default::default(),
        footnotes: Default::default(),
        endnotes: Default::default(),
        settings: Default::default(),
        font_table: None,
        theme: None,
        sections: Vec::new(),
        headers_footers: Vec::new(),
        media: Default::default(),
        support: Default::default(),
        source: DocumentSource {
            main_document: PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            font_table: None,
            footnotes: None,
            endnotes: None,
            theme: None,
        },
    }
}

fn theme() -> Theme {
    let mut colors = ThemeColors::default();
    colors.insert("accent1", "#4472C4");
    Theme {
        fonts: ThemeFonts {
            major: FontSet {
                latin: ThemeTypeface::named("Calibri Light"),
                ..FontSet::default()
            },
            minor: FontSet {
                latin: ThemeTypeface::named("Calibri"),
                ..FontSet::default()
            },
        },
        colors,
        shape_defaults: None,
        text_defaults: None,
        object_defaults_xml: None,
        location: SourceLocation::unknown(),
    }
}

#[test]
fn an_unreported_removal_fails_the_gate() {
    let mut report = NormalizationReport::new();
    report.count_unreported_removal(1);
    let verdict = verify_no_silent_loss(&report);
    assert!(
        verdict.is_err(),
        "a removal with no LossRecord must fail the gate, and it passed: {verdict:?}"
    );
}

#[test]
fn a_reported_removal_passes_the_gate() {
    let mut report = NormalizationReport::new();
    report.record_loss(LossRecord {
        transform_id: "W1.unserializable",
        feature_id: "w:sdt".to_owned(),
        reason: "written inline".to_owned(),
        severity: Severity::Lossy,
        locations: vec![SourceLocation::unknown()],
    });
    report.count_reported_removal(1);
    assert_eq!(verify_no_silent_loss(&report), Ok(()));
}

#[test]
fn the_theme_placeholder_loss_is_recorded_by_the_writer() {
    let document = Document {
        theme: Some(theme()),
        font_table: None,
        ..empty_document()
    };
    let written = write_package(&document, None, &WriteOptions::default())
        .expect("a document with a theme writes");

    let losses = written.report.losses();
    let theme_losses: Vec<&LossRecord> = losses
        .iter()
        .filter(|loss| loss.feature_id == "a:fmtScheme")
        .collect();

    assert!(
        !theme_losses.is_empty(),
        "the writer wrote a placeholder a:fmtScheme and said nothing: a document \
         built rather than parsed never passed through the reader that would \
         have recorded it"
    );
    assert_eq!(
        theme_losses[0].severity,
        Severity::Ignorable,
        "a placeholder scheme changes no colour the cascade reads, so it is \
         ignorable rather than lossy"
    );
}
