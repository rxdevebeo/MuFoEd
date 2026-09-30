#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal,
    clippy::case_sensitive_file_extension_comparisons
)]
//! Independent XML oracle for the Stage-5B fixture
//! `strict-ooxml-core/tests/strict/strict-stage5b.docx` (STAGE-5B §7.2).
//!
//! Reads the fixture with an independent ZIP reader (`zip`) and parses it with
//! an independent XML parser (`roxmltree`), then cross-checks the same
//! structures through our parser (anchors, shapes, groups, text boxes, page
//! borders).

use std::io::Read;
use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_wml::model::drawing::{Graphic, ShapeFill, ShapeGeometry};
use strict_ooxml_wml::model::{Block, DrawingKind, Inline, RunContent};
use strict_ooxml_wml::{parse_document, ParseOptions};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/strict/strict-stage5b.docx")
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
fn independent_oracle_sees_stage5b_markup() {
    let path = fixture();
    assert!(path.is_file(), "missing fixture {}", path.display());
    let document = read_part(&path, "word/document.xml");
    let parsed = roxmltree::Document::parse(&document).expect("valid document xml");
    let count = |name: &str| {
        parsed
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == name)
            .count()
    };
    assert!(count("anchor") >= 4, "anchors: {}", count("anchor"));
    assert!(count("wgp") >= 1, "groups: {}", count("wgp"));
    assert!(count("wsp") >= 3, "shapes: {}", count("wsp"));
    assert_eq!(count("txbxContent"), 1);
    assert_eq!(count("pgBorders"), 1);
    assert_eq!(
        count("custGeom"),
        0,
        "a:custGeom is reported as unsupported, not kept"
    );

    // The fixture uses the *real* Microsoft extension namespaces for shapes and
    // groups (STAGE-5B-REWORK-1 5B-1), keeping ISO namespaces for the core.
    let namespace = |name: &str| {
        parsed
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == name)
            .and_then(|node| node.tag_name().namespace())
            .map(str::to_owned)
    };
    assert_eq!(
        namespace("wsp").as_deref(),
        Some("http://schemas.microsoft.com/office/word/2010/wordprocessingShape")
    );
    assert_eq!(
        namespace("wgp").as_deref(),
        Some("http://schemas.microsoft.com/office/word/2010/wordprocessingGroup")
    );
    assert_eq!(
        namespace("anchor").as_deref(),
        Some("http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing")
    );
}

#[test]
fn our_parser_matches_the_fixture() {
    let path = fixture();
    let package = Package::open_path(&path, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");

    let mut anchors = Vec::new();
    for block in &document.body.blocks {
        if let Block::Paragraph(paragraph) = block {
            for inline in &paragraph.inlines {
                collect_anchors(inline, &mut anchors);
            }
        }
    }
    assert!(
        anchors.len() >= 4,
        "expected four anchored objects, found {}",
        anchors.len()
    );

    // At least one anchor carries a shape and one carries a group.
    let shapes = anchors
        .iter()
        .filter_map(|anchor| match anchor {
            Graphic::Shape(shape) => Some(shape),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!shapes.is_empty(), "no anchored shape");
    assert!(
        shapes.iter().any(|shape| matches!(
            shape.geometry,
            ShapeGeometry::Preset(ref preset) if preset.as_ref() == "roundRect"
        )),
        "roundRect preset missing"
    );
    assert!(shapes
        .iter()
        .any(|shape| matches!(shape.fill, Some(ShapeFill::Solid { .. }))));

    let groups = anchors
        .iter()
        .filter_map(|anchor| match anchor {
            Graphic::Group(group) => Some(group),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(groups.len(), 1, "exactly one group");
    assert_eq!(groups[0].children.len(), 2, "two group children");

    // The text box content parsed into blocks.
    assert!(
        shapes.iter().any(|shape| shape
            .text
            .as_ref()
            .is_some_and(|text| !text.blocks.is_empty())),
        "text box content missing"
    );

    // The section carries page borders.
    let borders = document
        .sections
        .last()
        .and_then(|section| section.properties.page_borders.as_ref())
        .expect("page borders");
    assert!(borders.top.is_some() && borders.bottom.is_some());
}

fn collect_anchors<'a>(inline: &'a Inline, out: &mut Vec<&'a Graphic>) {
    match inline {
        Inline::Run(run) => {
            for content in &run.content {
                if let RunContent::Drawing(drawing) = content {
                    if let DrawingKind::Anchor(ref anchor) = drawing.kind {
                        out.push(anchor.graphic.as_ref());
                    }
                }
            }
        }
        Inline::Hyperlink(link) => {
            for child in &link.inlines {
                collect_anchors(child, out);
            }
        }
        _ => {}
    }
}
