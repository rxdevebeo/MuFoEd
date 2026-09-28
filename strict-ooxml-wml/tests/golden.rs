#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Golden-DOM tests: canonical dumps of parsed documents are compared with
//! checked-in snapshots (STAGE-2 §12.1).
//!
//! Run with `UPDATE_GOLDEN=1` to regenerate `tests/golden/*.txt`.

mod common;

use std::fmt::Write as _;
use std::path::Path;

use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::drawing::DrawingKind;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent};
use strict_ooxml_wml::model::Document;

use common::{document_parts, parse_parts, rels};

const BASIC: &str = "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/><w:jc w:val=\"center\"/><w:numPr><w:numId w:val=\"5\"/></w:numPr></w:pPr>\
<w:bookmarkStart w:id=\"0\" w:name=\"b\"/>\
<w:r><w:rPr><w:b/><w:sz w:val=\"28\"/><w:color w:val=\"FF0000\"/></w:rPr><w:t xml:space=\"preserve\">Title</w:t><w:tab/></w:r>\
<w:hyperlink r:id=\"rIdLink\"><w:r><w:t>link</w:t></w:r></w:hyperlink>\
<w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
<w:bookmarkEnd w:id=\"0\"/></w:p>\
<w:tbl><w:tblGrid><w:gridCol w:w=\"2500\"/><w:gridCol w:w=\"2500\"/></w:tblGrid>\
<w:tr><w:trPr><w:trHeight w:val=\"400\" w:hRule=\"atLeast\"/></w:trPr>\
<w:tc><w:tcPr><w:tcW w:w=\"2500\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>";

const TABLE: &str =
    "<w:tbl><w:tblPr><w:tblStyle w:val=\"Grid\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"1000\"/><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p><w:r><w:t>span</w:t></w:r></w:p></w:tc>\
<w:tc><w:tcPr><w:vMerge w:val=\"continue\"/></w:tcPr><w:p/></w:tc></w:tr>\
<w:tr><w:tc><w:p/><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";

const LIST: &str = "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"5\"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>";

const SECTION: &str = "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:type w:val=\"continuous\"/></w:sectPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p>\
<w:p><w:r><w:t>two</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\" w:orient=\"landscape\"/>\
<w:cols w:num=\"2\" w:space=\"425\"><w:col w:w=\"4000\"/><w:col w:w=\"4000\"/></w:cols>\
<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/><w:titlePg/><w:docGrid w:type=\"lines\" w:linePitch=\"360\"/></w:sectPr>";

const DRAWING: &str = "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"457200\"/>\
<wp:docPr id=\"1\" name=\"Picture 1\" descr=\"d\"/>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"image1.png\" descr=\"\"/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg1\"/></pic:blipFill>\
<pic:spPr><a:xfrm><a:ext cx=\"914400\" cy=\"457200\"/></a:xfrm></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>";

const PROPS: &str = "<w:p><w:pPr><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/></w:pBdr>\
<w:shd w:val=\"clear\" w:fill=\"FFFF00\"/><w:spacing w:before=\"120\" w:after=\"240\"/><w:ind w:start=\"720\"/>\
<w:jc w:val=\"both\"/><w:keepNext/></w:pPr>\
<w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/><w:b/><w:i/><w:u w:val=\"single\"/>\
<w:color w:val=\"0000FF\"/><w:sz w:val=\"24\"/></w:rPr><w:t>styled</w:t></w:r></w:p>";

fn parse_case(body: &str, extra: &[(&str, Vec<u8>)]) -> Document {
    parse_parts(&document_parts(body, extra)).expect("parse golden document")
}

fn dump(document: &Document) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "document blocks={} sections={} media={} styles={} numbering={} features={}",
        document.body.blocks.len(),
        document.sections.len(),
        document.media.len(),
        document.styles.len(),
        document.numbering.len(),
        document.support.len()
    );
    for (index, block) in document.body.blocks.iter().enumerate() {
        dump_block(&mut out, index, block, 1);
    }
    for (index, section) in document.sections.iter().enumerate() {
        let props = &section.properties;
        let size = props.page_size.map_or_else(
            || "-".to_owned(),
            |size| {
                format!(
                    "{}x{}/{}",
                    size.width
                        .map_or_else(|| "-".to_owned(), |w| w.value().to_string()),
                    size.height
                        .map_or_else(|| "-".to_owned(), |h| h.value().to_string()),
                    size.orientation.map_or("-", |o| o.as_str())
                )
            },
        );
        let columns = props
            .columns
            .as_ref()
            .map_or_else(|| "-".to_owned(), |c| format!("{:?}", c.count));
        let headers: Vec<String> = props
            .headers
            .iter()
            .map(|reference| format!("{:?}", reference.kind))
            .collect();
        let _ = writeln!(
            out,
            "section[{index}] type={:?} pgSz={size} cols={columns} headers={headers:?} titlePg={}",
            props.section_type, props.title_page
        );
    }
    for item in document.media.iter() {
        let _ = writeln!(out, "media {} {:?}", item.part, item.kind);
    }
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn dump_block(out: &mut String, index: usize, block: &Block, depth: usize) {
    indent(out, depth);
    match block {
        Block::Paragraph(paragraph) => {
            let style = paragraph
                .props
                .style
                .as_ref()
                .map_or("-", |value| value.as_str());
            let alignment = paragraph
                .props
                .alignment
                .map_or_else(|| "-".to_owned(), |value| value.as_str().to_owned());
            let numbering = paragraph.props.numbering.map_or_else(
                || "-".to_owned(),
                |number| format!("{:?}/{:?}", number.ilvl, number.num_id),
            );
            let _ = writeln!(
                out,
                "block[{index}] paragraph style={style} jc={alignment} num={numbering}"
            );
            for inline in &paragraph.inlines {
                dump_inline(out, inline, depth + 1);
            }
        }
        Block::Table(table) => {
            let widths: Vec<String> = table
                .grid
                .iter()
                .map(|col| {
                    col.width
                        .map_or_else(|| "-".to_owned(), |w| w.value().to_string())
                })
                .collect();
            let _ = writeln!(
                out,
                "block[{index}] table grid=[{}] rows={}",
                widths.join(","),
                table.rows.len()
            );
            for row in &table.rows {
                indent(out, depth + 1);
                let _ = writeln!(out, "row cells={}", row.cells.len());
                for cell in &row.cells {
                    indent(out, depth + 2);
                    let _ = writeln!(
                        out,
                        "cell span={:?} vmerge={:?} blocks={}",
                        cell.props.grid_span,
                        cell.props.vertical_merge,
                        cell.blocks.len()
                    );
                }
            }
        }
        Block::SdtBlock(container) => {
            let _ = writeln!(out, "block[{index}] sdt blocks={}", container.blocks.len());
        }
        Block::AltChunk(_) => {
            let _ = writeln!(out, "block[{index}] altChunk");
        }
        Block::Opaque(_) => {
            let _ = writeln!(out, "block[{index}] opaque");
        }
    }
}

fn dump_inline(out: &mut String, inline: &Inline, depth: usize) {
    indent(out, depth);
    match inline {
        Inline::Run(run) => dump_run(out, run, depth),
        Inline::Hyperlink(link) => {
            let rel = link.rel_id.as_ref().map_or("-", |id| id.as_str());
            let _ = writeln!(out, "hyperlink rid={rel}");
            for child in &link.inlines {
                dump_inline(out, child, depth + 1);
            }
        }
        Inline::Field(field) => {
            let instruction = field.instruction.as_deref().unwrap_or("-");
            let _ = writeln!(out, "field instr={}", instruction.trim());
        }
        Inline::Drawing(drawing) => dump_drawing(out, drawing),
        Inline::BookmarkStart(id) => {
            let _ = writeln!(out, "bookmarkStart id={}", id.as_str());
        }
        Inline::BookmarkEnd(id) => {
            let _ = writeln!(out, "bookmarkEnd id={}", id.as_str());
        }
        Inline::Break(kind) => {
            let _ = writeln!(out, "break {}", kind.as_str());
        }
        Inline::Tab => {
            let _ = writeln!(out, "tab");
        }
        other => {
            let _ = writeln!(out, "inline {other:?}");
        }
    }
}

fn dump_drawing(out: &mut String, drawing: &strict_ooxml_wml::model::drawing::Drawing) {
    match &drawing.kind {
        DrawingKind::Inline(inline) => {
            let extent = inline.extent.map_or_else(
                || "-".to_owned(),
                |extent| format!("{}x{}", extent.cx.value(), extent.cy.value()),
            );
            let blip = inline
                .picture
                .as_ref()
                .and_then(|picture| picture.blip.as_ref())
                .and_then(|blip| blip.resolved.as_ref())
                .map_or_else(|| "-".to_owned(), |part| part.as_str().to_owned());
            let _ = writeln!(out, "drawing inline extent={extent} blip={blip}");
        }
        DrawingKind::Anchor(_) => {
            let _ = writeln!(out, "drawing anchor");
        }
        DrawingKind::Opaque(_) => {
            let _ = writeln!(out, "drawing opaque");
        }
    }
}

fn dump_run(out: &mut String, run: &Run, depth: usize) {
    let _ = writeln!(
        out,
        "run b={:?} i={:?} sz={:?} color={:?}",
        run.props.bold,
        run.props.italic,
        run.props
            .size
            .map(strict_ooxml_wml::model::values::HalfPoints::value),
        run.props
            .color
            .as_ref()
            .map(strict_ooxml_wml::model::values::Color::as_str)
    );
    for content in &run.content {
        indent(out, depth + 1);
        match content {
            RunContent::Text(text) => {
                let _ = writeln!(out, "text space={:?} {:?}", text.space, text.text);
            }
            RunContent::Tab => {
                let _ = writeln!(out, "tab");
            }
            RunContent::Break(kind) => {
                let _ = writeln!(out, "break {}", kind.as_str());
            }
            other => {
                let _ = writeln!(out, "{other:?}");
            }
        }
    }
}

fn check(name: &str, document: &Document) {
    let actual = dump(document);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create golden dir");
        std::fs::write(&path, &actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert_eq!(actual, expected, "golden DOM mismatch for {name}");
}

#[test]
fn golden_paragraphs_and_properties() {
    check("basic", &parse_case(BASIC, &[]));
    check("props", &parse_case(PROPS, &[]));
}

#[test]
fn golden_table() {
    check("table", &parse_case(TABLE, &[]));
}

#[test]
fn golden_list() {
    let numbering = format!(
        "<w:numbering xmlns:w=\"{}\"><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum><w:num w:numId=\"5\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>",
        common::W_NS
    )
    .into_bytes();
    let rels = rels(&[(
        "rIdNumbering",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering",
        "numbering.xml",
    )]);
    let document = parse_case(
        LIST,
        &[
            ("word/numbering.xml", numbering),
            ("word/_rels/document.xml.rels", rels),
        ],
    );
    check("list", &document);
}

#[test]
fn golden_section() {
    let rels = rels(&[(
        "rIdHeader",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/header",
        "header1.xml",
    )]);
    let document = parse_case(SECTION, &[("word/_rels/document.xml.rels", rels)]);
    check("section", &document);
}

#[test]
fn golden_drawing() {
    let rels = rels(&[(
        "rIdImg1",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/image",
        "media/image1.png",
    )]);
    let document = parse_case(
        DRAWING,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/media/image1.png", vec![0x89, b'P', b'N', b'G']),
        ],
    );
    check("drawing", &document);
}
