//! K10 (first part): `Editor::import` of blocks from another document.
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_edit::{Address, EditError, EditLimits, Editor, Unsupported};
use strict_ooxml_testkit::DocxBuilder;
use strict_ooxml_wml::model::{
    Block, Document, Inline, NumId, Paragraph, RunContent, StyleId, TriState,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

const W: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

fn build(styles: &str, numbering: &str, body: &str) -> Document {
    let bytes = DocxBuilder::strict()
        .body(body)
        .part(
            "word/styles.xml",
            format!("<w:styles xmlns:w=\"{W}\">{styles}</w:styles>").into_bytes(),
        )
        .rel("styles", "styles", "styles.xml")
        .part(
            "word/numbering.xml",
            format!("<w:numbering xmlns:w=\"{W}\">{numbering}</w:numbering>").into_bytes(),
        )
        .rel("numbering", "numbering", "numbering.xml")
        .build();
    let package = Package::open_reader(&bytes[..], &OpenOptions::default()).expect("package");
    parse_document(&package, &ParseOptions::default()).expect("document")
}

const NORMAL: &str = "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\">\
                      <w:name w:val=\"Normal\"/></w:style>";
const LIST: &str =
    "<w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/>\
                    <w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum>\
                    <w:num w:numId=\"1\"><w:abstractNumId w:val=\"1\"/></w:num>";

/// One paragraph holding the bookmark `kept`, in one section.
fn target() -> Document {
    build(
        &format!(
            "{NORMAL}<w:style w:type=\"paragraph\" w:styleId=\"Heading\"><w:name w:val=\"Heading\"/>\
             <w:basedOn w:val=\"Normal\"/><w:rPr><w:b/></w:rPr></w:style>\
             <w:style w:type=\"character\" w:styleId=\"Emph\"><w:name w:val=\"Emph\"/></w:style>"
        ),
        LIST,
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"kept\"/><w:r><w:t>target</w:t></w:r>\
         <w:bookmarkEnd w:id=\"1\"/></w:p>\
         <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>",
    )
}

/// Paragraphs using a style the target has (`Heading`, italic here), styles
/// it lacks (`Quote` with its linked `QuoteChar`, `Listy` with a list), the
/// list `1`, two bookmarks and a section break.
fn source() -> Document {
    build(
        &format!(
            "{NORMAL}<w:style w:type=\"paragraph\" w:styleId=\"Heading\"><w:name w:val=\"Heading\"/>\
             <w:rPr><w:i/></w:rPr></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/>\
             <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:link w:val=\"QuoteChar\"/></w:style>\
             <w:style w:type=\"character\" w:styleId=\"QuoteChar\"><w:name w:val=\"Quote Char\"/>\
             <w:link w:val=\"Quote\"/></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Listy\"><w:name w:val=\"Listy\"/>\
             <w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>"
        ),
        LIST,
        "<w:p><w:pPr><w:pStyle w:val=\"Heading\"/></w:pPr><w:r><w:t>h</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr>\
         <w:bookmarkStart w:id=\"5\" w:name=\"kept\"/><w:bookmarkStart w:id=\"6\" w:name=\"fresh\"/>\
         <w:r><w:rPr><w:rStyle w:val=\"QuoteChar\"/></w:rPr><w:t>q</w:t></w:r>\
         <w:bookmarkEnd w:id=\"5\"/><w:bookmarkEnd w:id=\"6\"/></w:p>\
         <w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
         <w:r><w:t>item</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Listy\"/></w:pPr><w:r><w:t>styled</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Gone\"/><w:sectPr><w:pgSz w:w=\"15840\" w:h=\"12240\"/></w:sectPr></w:pPr>\
         <w:r><w:t>end</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>",
    )
}

fn id(value: &str) -> StyleId {
    StyleId::new(value)
}

fn paragraphs(document: &Document) -> Vec<&Paragraph> {
    document
        .body
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn text(paragraph: &Paragraph) -> String {
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
}

fn bookmarks(document: &Document) -> Vec<(String, String)> {
    paragraphs(document)
        .into_iter()
        .flat_map(|p| &p.inlines)
        .filter_map(|inline| match inline {
            Inline::BookmarkStart(b) => Some((b.name.to_string(), b.id.as_str().to_owned())),
            _ => None,
        })
        .collect()
}

fn num(paragraph: &Paragraph) -> Option<NumId> {
    paragraph.props.numbering.and_then(|n| n.num_id)
}

#[test]
fn blocks_come_over_with_the_styles_and_lists_they_use() {
    let mut d = target();
    let original = d.clone();
    let from = source();
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.import(0, &Address::body(1), &from, &from.body.blocks)
        .expect("import");
    let after = e.document();
    let ps = paragraphs(after);
    assert_eq!(
        ps.iter().copied().map(text).collect::<Vec<_>>(),
        ["target", "h", "q", "item", "styled", "end"]
    );
    // The target's Heading stays bold and not italic.
    let heading = after.styles.get(&id("Heading")).expect("heading");
    assert_eq!(heading.run.bold, TriState::On);
    assert_eq!(heading.run.italic, TriState::Absent);
    // Quote comes with what it names.
    let quote = after.styles.get(&id("Quote")).expect("quote");
    assert_eq!(quote.based_on, Some(id("Normal")));
    assert_eq!(quote.link, Some(id("QuoteChar")));
    assert!(after.styles.contains(&id("QuoteChar")));
    assert!(!quote.is_default);
    // The list is a new instance of a new abstract definition, the same one
    // for the paragraph and for the style that names it.
    let item = num(ps[3]).expect("numbered");
    assert_ne!(item, NumId(1));
    let listy = after.styles.get(&id("Listy")).expect("listy");
    assert_eq!(listy.paragraph.numbering.and_then(|n| n.num_id), Some(item));
    let instance = after.numbering.num(item).expect("instance");
    assert_ne!(instance.abstract_num_id.0, 1);
    assert!(
        after.numbering.num(NumId(1)).is_some(),
        "the target's list stays"
    );
    // A style the source does not define is not referred to.
    assert_eq!(ps[5].props.style, None);
    // The section break is gone; the bookmark named like the target's too.
    assert!(ps.iter().all(|p| p.props.section.is_none()));
    assert_eq!(after.sections.len(), 1);
    let marks = bookmarks(after);
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert_eq!(marks[0], ("kept".to_owned(), "1".to_owned()));
    assert_eq!(marks[1].0, "fresh");
    assert_ne!(marks[1].1, "1");
    assert!(e.validate().is_ok());
    // One transaction: undo takes the definitions back too.
    e.undo(1).expect("undo");
    assert_eq!(e.document().body, original.body);
    assert_eq!(e.document().styles, original.styles);
    assert_eq!(e.document().numbering, original.numbering);
}

#[test]
fn importing_twice_makes_two_lists() {
    let mut d = target();
    let from = source();
    let item = &from.body.blocks[2..3];
    let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
    e.import(0, &Address::body(1), &from, item).expect("first");
    e.import(1, &Address::body(2), &from, item).expect("second");
    let ps = paragraphs(e.document());
    let (first, second) = (num(ps[1]).expect("first"), num(ps[2]).expect("second"));
    assert_ne!(first, second);
    assert!(e.validate().is_ok());
}

#[test]
fn what_an_import_cannot_carry_is_refused() {
    // A hyperlink by relationship belongs to the source package.
    let linked = build(
        NORMAL,
        "",
        "<w:p><w:hyperlink r:id=\"rIdLink\"><w:r><w:t>x</w:t></w:r></w:hyperlink></w:p>",
    );
    // One id, two kinds of style.
    let clash = build(
        &format!(
            "{NORMAL}<w:style w:type=\"paragraph\" w:styleId=\"Emph\"><w:name w:val=\"Emph\"/></w:style>"
        ),
        "",
        "<w:p><w:pPr><w:pStyle w:val=\"Emph\"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>",
    );
    for from in [linked, clash] {
        let mut d = target();
        let original = d.clone();
        let mut e = Editor::new(&mut d, EditLimits::default()).expect("editor");
        assert_eq!(
            e.import(0, &Address::body(1), &from, &from.body.blocks),
            Err(EditError::UnsupportedContent(Unsupported::Import))
        );
        assert_eq!(e.document().body, original.body);
        assert_eq!(e.document().styles, original.styles);
        assert_eq!(e.revision(), 0);
    }
}
