//! Never-crash over the whole public pipeline (`docs/WORDCRAFT_ADOPTION_2026-10-07.md` §5 step 4).
//!
//! `open (Normalize) → support_report → render_svg → render_pdf → write → reopen`
//! must not panic, hang or overflow a 1 MiB stack, and bytes that `write_package`
//! returned must open again. The same invariant as the `fuzz_docx_full` target,
//! held in every CI run instead of only under the fuzzer:
//!
//! - on the CC0 `ci-core` documents (skipped when the corpus is not fetched);
//! - on a feature-rich synthetic body mutated by proptest — spans deleted and
//!   duplicated, structural tags and hostile attribute values spliced in — so
//!   the parsers meet unbalanced nesting and out-of-range numbers, not only
//!   a ZIP that fails its CRC.

#![allow(missing_docs)]

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use strict_ooxml::{
    write_package, ConformancePolicy, OpenOptions, PageSelection, RenderOptions, ResourceLimits,
    StrictDocument, TransitionalNormalizer, WriteOptions,
};
use strict_ooxml_testkit::harness::{bounded_with, Outcome, STACK};
use strict_ooxml_testkit::DocxBuilder;

/// Real documents in a debug build take longer than the harness's default 10 s.
const TIMEOUT: Duration = Duration::from_secs(60);

fn options() -> OpenOptions {
    OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(Arc::new(TransitionalNormalizer::new()))
}

/// Runs the pipeline; `Err` names the stage whose contract broke.
fn pipeline(bytes: Vec<u8>, limits: ResourceLimits) -> Result<(), String> {
    let options = options().limits(limits);
    let Ok(doc) = StrictDocument::open_reader(Cursor::new(bytes), &options) else {
        return Ok(());
    };
    let _ = doc.support_report();
    let render = RenderOptions {
        pages: PageSelection::Range { start: 1, end: 3 },
        limits,
        ..RenderOptions::default()
    };
    let _ = doc.render_svg(&render);
    let _ = doc.render_pdf(&render);
    let Ok(written) = write_package(
        doc.document(),
        Some(doc.package()),
        &WriteOptions::default(),
    ) else {
        return Ok(());
    };
    match StrictDocument::open_reader(Cursor::new(written.bytes), &options) {
        Ok(_) => Ok(()),
        Err(error) => Err(format!("written bytes do not reopen: {error}")),
    }
}

/// [`pipeline`] on a 1 MiB thread; panics with `what` on any broken contract.
fn survives(what: &str, bytes: Vec<u8>, limits: ResourceLimits) {
    match bounded_with(STACK, TIMEOUT, move || pipeline(bytes, limits)) {
        Outcome::Returned(Ok(())) => {}
        Outcome::Returned(Err(message)) => panic!("{what}: {message}"),
        Outcome::Panicked(message) => panic!("{what}: panicked: {message}"),
        Outcome::TimedOut => panic!("{what}: did not return within {TIMEOUT:?}"),
    }
}

mod corpus {
    use super::*;
    use strict_ooxml_testkit::corpus::{tier, Tier};

    #[test]
    fn every_ci_core_document_survives_the_pipeline() {
        for doc in tier(Tier::CiCore) {
            let bytes = std::fs::read(&doc.path).expect("read corpus document");
            survives(&doc.id, bytes, ResourceLimits::default());
        }
    }
}

mod mutated {
    use super::*;

    /// One of everything the recursive paths walk: nested tables, inline
    /// containers, a block SDT, a field, math, a list item, a break.
    const BODY: &str = concat!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>"#,
        r#"<w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve">Head </w:t></w:r>"#,
        r#"<w:hyperlink w:anchor="x"><w:r><w:t>link</w:t></w:r></w:hyperlink>"#,
        r#"<w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>"#,
        r#"<w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="1"/></w:tcPr><w:p><w:r><w:t>a</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="2000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>in</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:p/></w:tc><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:sdt><w:sdtPr><w:alias w:val="s"/></w:sdtPr><w:sdtContent><w:p><w:sdt><w:sdtContent><w:r><w:t>sdt</w:t></w:r></w:sdtContent></w:sdt></w:p></w:sdtContent></w:sdt>"#,
        r#"<w:p><m:oMath><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:den></m:f></m:oMath></w:p>"#,
        r#"<w:p><w:r><w:tab/><w:t>tail</w:t><w:br w:type="page"/></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/><w:cols w:num="2"/></w:sectPr>"#,
    );

    /// Spliced in at random offsets: openers without closers, closers without
    /// openers, and attributes at the edges of their types.
    const TOKENS: &[&str] = &[
        "<w:tbl>",
        "</w:tbl>",
        "<w:tr>",
        "<w:tc>",
        "</w:tc>",
        "<w:p>",
        "</w:p>",
        "<w:r>",
        "<w:sdt><w:sdtContent>",
        "</w:sdtContent></w:sdt>",
        "<w:hyperlink>",
        "<m:oMath><m:d><m:e>",
        r#"<w:gridCol w:w="-1"/>"#,
        r#"<w:tblW w:w="2147483647" w:type="dxa"/>"#,
        r#"<w:gridSpan w:val="65535"/>"#,
        r#"<w:sz w:val="0"/>"#,
        r#"<w:spacing w:line="-2147483648" w:lineRule="exact"/>"#,
        r#"<w:ind w:left="99999999999"/>"#,
        r#"<w:pgSz w:w="0" w:h="0"/>"#,
        r#"<w:cols w:num="0"/>"#,
        r#"<w:numPr><w:ilvl w:val="9"/><w:numId w:val="4294967295"/></w:numPr>"#,
        "<w:t>&#x10FFFF;</w:t>",
        r#" w:val="NaN""#,
        "<w:br/>",
    ];

    #[derive(Clone, Debug)]
    enum Mutation {
        Delete { at: usize, len: usize },
        Duplicate { at: usize, len: usize, to: usize },
        Insert { at: usize, token: usize },
    }

    fn mutation() -> impl Strategy<Value = Mutation> {
        prop_oneof![
            (any::<usize>(), 1usize..64).prop_map(|(at, len)| Mutation::Delete { at, len }),
            (any::<usize>(), 1usize..256, any::<usize>())
                .prop_map(|(at, len, to)| Mutation::Duplicate { at, len, to }),
            (any::<usize>(), 0..TOKENS.len())
                .prop_map(|(at, token)| Mutation::Insert { at, token }),
        ]
    }

    /// Applies `mutations` to the ASCII `body`; offsets wrap, so every one lands.
    fn mutate(body: &str, mutations: &[Mutation]) -> String {
        let mut bytes = body.as_bytes().to_vec();
        for mutation in mutations {
            let len = bytes.len().max(1);
            match *mutation {
                Mutation::Delete { at, len: count } => {
                    let start = (at % len).min(bytes.len());
                    let end = (start + count).min(bytes.len());
                    bytes.drain(start..end);
                }
                Mutation::Duplicate { at, len: count, to } => {
                    let start = (at % len).min(bytes.len());
                    let end = (start + count).min(bytes.len());
                    let span = bytes[start..end].to_vec();
                    let to = (to % len).min(bytes.len());
                    bytes.splice(to..to, span);
                }
                Mutation::Insert { at, token } => {
                    let at = (at % len).min(bytes.len());
                    bytes.splice(at..at, TOKENS[token].bytes());
                }
            }
        }
        String::from_utf8(bytes).expect("ASCII stays UTF-8")
    }

    /// Small limits so a mutation that multiplies content meets a cap, not the
    /// time limit.
    fn limits() -> ResourceLimits {
        ResourceLimits {
            max_pages: 3,
            max_render_items: 50_000,
            ..ResourceLimits::default()
        }
    }

    #[test]
    fn the_unmutated_body_survives_and_round_trips() {
        survives("strict", DocxBuilder::strict().body(BODY).build(), limits());
        let transitional = DocxBuilder::transitional().body(BODY).build();
        survives("transitional", transitional, limits());
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

        #[test]
        fn a_mutated_body_survives_the_pipeline(
            mutations in prop::collection::vec(mutation(), 1..8),
            strict in any::<bool>(),
        ) {
            let body = mutate(BODY, &mutations);
            let builder = if strict { DocxBuilder::strict() } else { DocxBuilder::transitional() };
            survives(&format!("{mutations:?}"), builder.body(&body).build(), limits());
        }
    }
}
