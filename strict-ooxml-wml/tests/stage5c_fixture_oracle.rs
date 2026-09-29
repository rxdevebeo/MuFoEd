#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal,
    clippy::case_sensitive_file_extension_comparisons
)]
//! Independent XML oracle for the Stage-5C fixture
//! `strict-ooxml-core/tests/strict/strict-stage5c.docx`
//! (`STAGE-5C-TASK.md` §7.2, structural oracle).
//!
//! The fixture is read with an independent ZIP reader (`zip`) and parsed with an
//! independent XML parser (`roxmltree`); the OMML constructs it contains are
//! then cross-checked against our model. Neither step goes through our own
//! reader, so a namespace or dispatch mistake cannot hide from both.

use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::math::MathNode;
use strict_ooxml_wml::model::{Block, Inline};
use strict_ooxml_wml::{parse_document, ParseOptions};

const MATH_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5c.docx")
}

fn read_part(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open fixture");
    let mut archive = zip::ZipArchive::new(file).expect("independent zip open");
    let mut entry = archive.by_name(name).expect("part present");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read part");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn the_fixture_uses_the_strict_math_namespace() {
    let path = fixture();
    assert!(path.is_file(), "missing fixture {}", path.display());
    let document = read_part(&path, "word/document.xml");
    let parsed = roxmltree::Document::parse(&document).expect("valid document xml");
    // Every OMML element of the fixture must sit in the Strict math namespace —
    // a Transitional `m:` prefix would be a different (rejected) namespace.
    for node in parsed.descendants().filter(roxmltree::Node::is_element) {
        let tag = node.tag_name();
        if tag.namespace() == Some(MATH_NS) {
            assert!(
                !tag.name().is_empty(),
                "a math element must have a local name"
            );
        }
    }
    let in_math_ns = |name: &str| {
        parsed
            .descendants()
            .filter(roxmltree::Node::is_element)
            .any(|node| {
                node.tag_name().name() == name && node.tag_name().namespace() == Some(MATH_NS)
            })
    };
    for element in [
        "oMath",
        "oMathPara",
        "r",
        "t",
        "f",
        "num",
        "den",
        "dPr",
        "mPr",
    ] {
        assert!(
            in_math_ns(element),
            "element {element} is missing from the math namespace"
        );
    }
    // And no OMML element may carry a non-Strict namespace.
    assert!(!document.contains("schemas.openxmlformats.org/officeDocument/2006/math"));
}

#[test]
fn the_fixture_contains_every_construct_of_the_order() {
    let path = fixture();
    let document = read_part(&path, "word/document.xml");
    let parsed = roxmltree::Document::parse(&document).expect("valid document xml");
    // Only elements in the Strict math namespace count: the same local names
    // also occur in the WML namespace (`w:r`, `w:p`).
    let count = |name: &str| {
        parsed
            .descendants()
            .filter(|node| {
                node.is_element()
                    && node.tag_name().name() == name
                    && node.tag_name().namespace() == Some(MATH_NS)
            })
            .count()
    };
    // §3.1: wrappers, runs, and every structure of the list.
    assert_eq!(count("oMathPara"), 3, "display formulas");
    assert!(count("oMath") >= 16, "inline formulas: {}", count("oMath"));
    assert!(count("r") >= 40, "runs: {}", count("r"));
    for construct in [
        "f",
        "rad",
        "sSup",
        "sSub",
        "sSubSup",
        "sPre",
        "nary",
        "d",
        "func",
        "limLow",
        "limUpp",
        "m",
        "eqArr",
        "acc",
        "bar",
        "groupChr",
        "box",
        "borderBox",
        "phant",
    ] {
        assert!(count(construct) >= 1, "construct {construct} is missing");
    }
    // §3.1: the property elements of §3.1's last bullet.
    for property in [
        "maxDist",
        "objDist",
        "baseJc",
        "lit",
        "smallFrac",
        "type",
        "pos",
        "jc",
    ] {
        assert!(count(property) >= 1, "property {property} is missing");
    }
    // The fixture must not contain a Transitional construct: every OMML element
    // already checked above is in the Strict namespace, and no Transitional
    // namespace appears anywhere in the part.
    assert!(!document.contains("schemas.openxmlformats.org/officeDocument/2006/math"));
    assert!(
        !document.contains("schemas.microsoft.com/office/word"),
        "the formula fixture must not rely on a Microsoft extension namespace"
    );
}

#[test]
fn our_model_sees_the_same_constructs() {
    let path = fixture();
    let package = Package::open_path(&path, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");

    // Every node of every formula, nested ones included: the comparison below
    // is against the constructs the independent XML parser saw anywhere in the
    // document, not only at the top level.
    let mut inline: Vec<MathNode> = Vec::new();
    let mut display = 0usize;
    for block in &document.body.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for entry in &paragraph.inlines {
            let expression = match entry {
                Inline::Math(expression) => expression,
                Inline::MathParagraph(display_formula) => {
                    display += 1;
                    &display_formula.expression
                }
                _ => continue,
            };
            expression.walk(&mut |node| inline.push(node.clone()));
        }
    }
    assert_eq!(display, 3, "the fixture has three display formulas");
    assert!(
        !inline.is_empty() && !document.body.blocks.is_empty(),
        "the formula content must be modelled"
    );

    // Every construct of §3.1 must appear in the model, and nothing may be an
    // unmodelled construct (the fixture is schema-conformant).
    let mut seen: Vec<&str> = inline.iter().map(MathNode::element_name).collect();
    seen.sort_unstable();
    for expected in [
        "m:f",
        "m:rad",
        "m:sSup",
        "m:sSub",
        "m:sSubSup",
        "m:sPre",
        "m:nary",
        "m:d",
        "m:func",
        "m:limLow",
        "m:limUpp",
        "m:m",
        "m:eqArr",
        "m:acc",
        "m:bar",
        "m:groupChr",
        "m:box",
        "m:borderBox",
        "m:phant",
    ] {
        assert!(
            seen.contains(&expected),
            "{expected} is missing from the model: {seen:?}"
        );
    }
    assert!(
        !seen.contains(&"m:unknown"),
        "the fixture is schema-conformant, so nothing may be unmodelled: {seen:?}"
    );
    assert!(
        !seen.contains(&"m:oMath"),
        "an oMath element is not a construct node: {seen:?}"
    );
}

#[test]
fn our_model_agrees_with_the_oracle_on_the_text() {
    let path = fixture();
    let package = Package::open_path(&path, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");

    // Concatenate every `m:t` the oracle sees and every run our model holds.
    let document_xml = read_part(&path, "word/document.xml");
    let parsed = roxmltree::Document::parse(&document_xml).expect("valid xml");
    let oracle: String = parsed
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "t"
                && node.tag_name().namespace() == Some(MATH_NS)
        })
        .filter_map(|node| node.text())
        .collect();

    let mut ours = String::new();
    for block in &document.body.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for entry in &paragraph.inlines {
            let expression = match entry {
                Inline::Math(expression) => expression,
                Inline::MathParagraph(display) => &display.expression,
                _ => continue,
            };
            expression.walk(&mut |node| {
                if let MathNode::Run(run) = node {
                    ours.push_str(&run.text);
                }
            });
        }
    }
    assert_eq!(ours, oracle, "the two readers must see the same characters");
    assert!(!ours.is_empty(), "the fixture must carry text");
}
