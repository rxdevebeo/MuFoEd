//! The resource dictionaries of a page, and the shape a producer writes them in
//! (`STAGE-8-TASK.md` §5, C1).
//!
//! The fixture here is hand-built for a reason that is worth stating: our own
//! renderer writes `/Resources` with `/Font` and `/XObject` **inline**, and a
//! reader that only understands what we produce will pass every test we write
//! against our own output. Word — and most producers that share one font table
//! between pages — write them **indirect**: `/Resources<</Font 17 0 R>>`. The
//! first version of this reader read the entries off the raw object, found
//! nothing, and reported every page of `cpio.5.pdf`, `mtree.5.pdf` and
//! `tar.5.pdf` as having no text at all. Three documents, forty thousand glyphs,
//! no error anywhere: the fonts were simply never found, and a font that is not
//! found is reported as missing while the text that needed it is not drawn.
//!
//! The fix is one call. This test is the thing that keeps it.
#![allow(clippy::doc_markdown)]

use std::path::Path;

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits, TextLayer};

/// Assembles a PDF from object bodies, all of them text except where a body says
/// otherwise.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        body.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        body.extend_from_slice(object);
        body.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(&body);
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in &offsets {
        use std::fmt::Write as _;
        let _ = write!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n0\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// An object body that is text.
fn text(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

/// A 1×1 red image, as bytes: three samples, and no route through a `String`.
fn image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
          /BitsPerComponent 8 /Length 3 >>\nstream\n"
        .to_vec();
    out.extend_from_slice(&[0xff, 0x00, 0x00]);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A content stream that writes one word and draws the picture.
fn content() -> Vec<u8> {
    stream_of(b"q 20 0 0 20 150 50 cm /Im0 Do Q BT /F1 12 Tf 1 0 0 1 20 50 Tm (Indirect) Tj ET")
}

/// A content stream from raw operators.
fn stream_of(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A form XObject: a content stream, an optional `/Matrix`, an optional
/// `/Resources` and the `/BBox` the specification requires and this reader does
/// not use.
fn form(body: &[u8], matrix: Option<&str>, resources: Option<&str>) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut out = String::from("<< /Type /XObject /Subtype /Form /BBox [0 0 100 100]");
    if let Some(matrix) = matrix {
        let _ = write!(out, " /Matrix [{matrix}]");
    }
    if let Some(resources) = resources {
        let _ = write!(out, " /Resources {resources}");
    }
    let _ = write!(out, " /Length {} >>\nstream\n", body.len());
    out.push_str(&String::from_utf8_lossy(body));
    out.push_str("\nendstream");
    out.into_bytes()
}

/// The font the fixture draws with: `I` at 278/1000 em, WinAnsi.
fn font() -> Vec<u8> {
    text(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 73 /LastChar 100 \
         /Widths [278] /Encoding /WinAnsiEncoding >>",
    )
}

/// A page whose `/Font` and `/XObject` are indirect references — Word's shape.
///
/// Object numbering: 1 catalog, 2 pages, 3 the page, 4 its content, 5 the font
/// table, 6 the font, 7 the XObject table, 8 the image.
fn indirect_resources() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font 5 0 R \
             /XObject 7 0 R >> /Contents 4 0 R >>",
        ),
        content(),
        text("<< /F1 6 0 R >>"),
        font(),
        text("<< /Im0 8 0 R >>"),
        image(),
    ])
}

/// The same page with the two dictionaries written inline, which is what our own
/// renderer does. Both shapes must give the same answer, and this test says so
/// rather than assuming it.
fn inline_resources() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 6 0 R \
             >> /XObject << /Im0 8 0 R >> >> /Contents 4 0 R >>",
        ),
        content(),
        text("<< unused >>"),
        font(),
        text("<< unused >>"),
        image(),
    ])
}

/// A font reached through an indirect `/Font` is a font, and the page's text is
/// the page's text.
#[test]
fn an_indirect_font_table_is_a_font_table() {
    let mut document =
        PdfDocument::open(&indirect_resources(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    assert_eq!(page.text_layer(), TextLayer::Readable);
    let glyphs: Vec<&strict_ooxml_pdf::Glyph> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 8, "one glyph per character");
    assert!(
        glyphs.iter().all(|glyph| glyph.mapped),
        "a font in an indirect table is not a missing font, and a named encoding is a \
         stated mapping: {:?}",
        glyphs.iter().map(|glyph| &glyph.text).collect::<Vec<_>>()
    );
    let text: String = glyphs.iter().map(|glyph| glyph.text.as_str()).collect();
    assert_eq!(text, "Indirect");
}

/// The same for a picture: `/XObject` reached indirectly decodes.
#[test]
fn an_indirect_xobject_table_is_an_xobject_table() {
    let mut document =
        PdfDocument::open(&indirect_resources(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let images = page
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Image(_)))
        .count();
    assert_eq!(images, 1, "the image in an indirect table is found");
}

/// Inline and indirect are the same dictionary, and a reader that says otherwise
/// is a reader that only understands its own producer.
#[test]
fn both_shapes_of_resource_dictionary_agree() {
    let read = |bytes: Vec<u8>| {
        let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
        let page = document.page(1).expect("page");
        (
            page.text().trim().to_owned(),
            page.items()
                .iter()
                .filter(|item| matches!(item, Item::Image(_)))
                .count(),
            page.text_layer(),
        )
    };
    assert_eq!(
        read(indirect_resources()),
        read(inline_resources()),
        "the shape of the dictionary is not a property of the page"
    );
}

/// A page whose picture arrives **through a form** that declares the picture
/// itself, and whose own `/Matrix` places it.
///
/// Object numbering: 1 catalog, 2 pages, 3 the page, 4 the page's content, 5 the
/// XObject table, 6 the form, 7 the image.
fn form_draws_picture() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        // The page declares the form and **nothing else**: `/Im0` appears only
        // inside the form.
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject 5 0 R >> \
             /Contents 4 0 R >>",
        ),
        stream_of(b"q 1 0 0 1 0 0 cm /Fm0 Do Q"),
        text("<< /Fm0 6 0 R >>"),
        form(
            b"q 30 0 0 40 10 20 cm /Im0 Do Q",
            Some("1 0 0 1 0 0"),
            Some("<< /XObject << /Im0 7 0 R >> >>"),
        ),
        image(),
    ])
}

/// A form with an **empty** `/Resources` drawing names the page declares.
fn form_borrows_page_resources() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject 5 0 R \
             /Font << /F1 6 0 R >> >> /Contents 4 0 R >>",
        ),
        stream_of(b"/Fm0 Do"),
        text("<< /Fm0 7 0 R >>"),
        font(),
        form(
            b"q 20 0 0 20 10 10 cm /Im0 Do Q BT /F1 12 Tf 1 0 0 1 20 80 Tm (I) Tj ET",
            None,
            Some("<< >>"),
        ),
        image(),
    ])
}

/// A form that draws itself: the loop a producer can write and a reader must not
/// follow.
fn form_draws_itself() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject 5 0 R >> \
             /Contents 4 0 R >>",
        ),
        stream_of(b"/Fm0 Do"),
        text("<< /Fm0 6 0 R >>"),
        form(b"/Fm0 Do", None, Some("<< /XObject << /Fm0 6 0 R >> >>")),
    ])
}

/// Three forms deep, each scaling the next, so the matrices have to compose.
///
/// Object numbering: 1 catalog, 2 pages, 3 the page, 4 the page's content, 5 the
/// XObject table, 6 the outermost form, 7 the middle form, 8 the innermost form,
/// 9 the image.
fn nested_forms() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject 5 0 R >> \
             /Contents 4 0 R >>",
        ),
        stream_of(b"q 2 0 0 2 5 5 cm /Fm0 Do Q"),
        text("<< /Fm0 6 0 R >>"),
        form(
            b"/Fm1 Do",
            Some("3 0 0 3 0 0"),
            Some("<< /XObject << /Fm1 7 0 R >> >>"),
        ),
        form(b"/Fm2 Do", None, Some("<< /XObject << /Fm2 8 0 R >> >>")),
        form(
            b"q 5 0 0 5 0 0 cm /Im0 Do Q",
            None,
            Some("<< /XObject << /Im0 9 0 R >> >>"),
        ),
        image(),
    ])
}

/// A form drawn thirty times from the page: breadth, at a depth of one, which is
/// what the shared operation budget is for — a per-invocation count would call
/// thirty forms of two operators each «sixty items, well within budget».
fn form_drawn_often() -> Vec<u8> {
    let body = "/Fm0 Do ".repeat(30);
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject 5 0 R >> \
             /Contents 4 0 R >>",
        ),
        stream_of(body.as_bytes()),
        text("<< /Fm0 6 0 R >>"),
        // A form that draws nothing at all: two operators, thirty times over.
        form(b"q Q", None, None),
    ])
}

/// The corpus this reader is measured against lives outside the repository
/// (`testdata/pdf`, a licensed set of foreign PDFs), so it is not a test — but a
/// short note on what it is for, and where it was, belongs next to the code that
/// would otherwise be measured only against our own writer.
#[test]
fn the_foreign_corpus_is_not_part_of_the_build() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/pdf");
    assert!(
        !corpus.exists() || corpus.is_dir(),
        "if the corpus is there it is a directory of PDFs, not a stray file"
    );
}

/// A picture drawn from **inside a form XObject**, with the form declaring the
/// resource itself.
///
/// This is the shape the corpus found: `language-center.pdf`'s first object is a
/// form with `/Resources <<>>` whose content refers to names the page never
/// declares, and every one of those names came back as `pdf.image.missing`. A
/// reader that treats `/XObject` as a table of pictures loses the drawing without
/// saying so, because the name *is* in the page's table — it is a form, not a
/// picture.
#[test]
fn a_picture_drawn_inside_a_form_is_the_forms_picture() {
    let mut document =
        PdfDocument::open(&form_draws_picture(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let images: Vec<&strict_ooxml_pdf::PlacedImage> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(
        images.len(),
        1,
        "the form's picture is drawn, not reported missing: {:?}",
        page.items()
    );
    let image = images[0];
    assert!(image.missing.is_none(), "{:?}", image.missing);
    assert!(image.image.is_some(), "and it is decoded");
    // The form's `/Matrix` places it: the unit square at 10,20 scaled by 30×40
    // covers x 10..40 and y 20..60.
    assert!(
        (image.x - 10.0).abs() < 0.01 && (image.width - 30.0).abs() < 0.01,
        "x {:.2} width {:.2}",
        image.x,
        image.width
    );
    assert!(
        (image.y - 20.0).abs() < 0.01 && (image.height - 40.0).abs() < 0.01,
        "y {:.2} height {:.2}",
        image.y,
        image.height
    );
    assert!(
        !document
            .report()
            .losses()
            .iter()
            .any(|loss| loss.id == "pdf.image.missing"),
        "a name the form declares is not a missing name:\n{}",
        document.report()
    );
}

/// A form that declares no resources of its own uses the page's.
///
/// A producer that writes `/Resources <<>>` meant «nothing extra», not «nothing at
/// all», and the form's content is full of names the page does declare.
#[test]
fn a_form_without_resources_uses_the_pages() {
    let mut document =
        PdfDocument::open(&form_borrows_page_resources(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let images = page
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Image(_)))
        .count();
    assert_eq!(images, 1, "the page's picture is reachable from the form");
    let glyphs = page
        .items()
        .iter()
        .filter(|item| matches!(item, Item::Glyph(_)))
        .count();
    assert_eq!(glyphs, 1, "and so is the page's font");
}

/// A form that draws itself is a loop, and a page is not allowed to be one.
#[test]
fn a_form_that_draws_itself_is_refused_by_the_depth_bound() {
    let mut document = PdfDocument::open(&form_draws_itself(), PdfLimits::default()).expect("open");
    let error = document
        .page(1)
        .expect_err("a self-drawing form is not a document");
    assert!(
        error.to_string().contains("form_depth"),
        "the nesting bound is what refuses it: {error}"
    );
}

/// Breadth is counted too: a form drawn many times is many times the work, and a
/// per-invocation count would call that cheap. Past the budget the page stops
/// soft and records `pdf.page.budget` (AUD-13), rather than refusing the page.
#[test]
fn a_form_drawn_often_is_stopped_by_the_operation_budget() {
    let mut document = PdfDocument::open(
        &form_drawn_often(),
        PdfLimits {
            max_operations: 20,
            ..PdfLimits::default()
        },
    )
    .expect("open");
    let page = document
        .page(1)
        .expect("thirty invocations stop soft, they do not refuse the page");
    assert!(
        page.report
            .losses()
            .iter()
            .any(|loss| loss.id == "pdf.page.budget" && loss.detail.contains("operations")),
        "the page budget names operations:\n{}",
        page.report
    );
}

/// A form nested a few deep, with its own matrix, is read and placed where the
/// matrices put it.
#[test]
fn nested_forms_compose_their_matrices() {
    let mut document = PdfDocument::open(&nested_forms(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let images: Vec<&strict_ooxml_pdf::PlacedImage> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1, "one picture through three forms");
    // The page scales by 2 and moves to (5, 5); the outermost form's `/Matrix`
    // scales by 3; the innermost content scales by 5. A 1x1 picture in the unit
    // square therefore covers 2 x 3 x 5 = 30 points, at (5, 5).
    let image = images[0];
    for (got, want, what) in [
        (image.x, 5.0, "x"),
        (image.y, 5.0, "y"),
        (image.width, 30.0, "width"),
        (image.height, 30.0, "height"),
    ] {
        assert!(
            (got - want).abs() < 0.01,
            "{what} is {got}, expected {want}"
        );
    }
}
