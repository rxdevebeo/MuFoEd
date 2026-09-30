//! Reading the text of a page that has none (`STAGE-8-TASK.md` §6, O11).
//!
//! The fixture is a PDF **by hand**: one page, one image, no text operators at
//! all. That is what a scan looks like to a reader, and nothing in this
//! workspace can produce one — our renderer draws glyphs, not pictures of them —
//! so a fixture built from our own output would test nothing.
//!
//! The model is scripted, because what is under test is the *pipeline*: which
//! pages are sent, what happens to the answer, and what the document and the
//! report say afterwards. Whether a real model reads a page well is 8D's question
//! and its live probe, not this file's.
//!
//! Every test here runs with the `raster` feature, since the whole point is that
//! a page image comes from somewhere.

#![cfg(feature = "raster")]
#![allow(clippy::doc_markdown)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::PoisonError;

use strict_ooxml_convert::{convert, Mode, PdfOptions, Severity, RECOVERY_STYLE_NAME};
use strict_ooxml_ocr::traits::{Recovered as Answer, TextRecovery, VisionError};
use strict_ooxml_pdf::{PdfDocument, PdfLimits, TextLayer};
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::ids::StyleId;
use strict_ooxml_wml::model::inline::{Inline, RunContent};

/// A one-page PDF whose only content is a 1×1 red image, and which therefore has
/// no text layer at all.
///
/// Written out by hand, and **as bytes all the way down**: the image's samples
/// are binary, and this fixture got them wrong twice — once by putting a `0xFF`
/// in a Rust `String`, and once by passing the bytes through
/// `String::from_utf8_lossy` on the way into the file. Both times the picture
/// arrived as three bytes of U+FFFD and the reader said, correctly,
/// `raw samples are not a PNG` (`Q-19` is not a story about somebody else).
fn scan_pdf() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject \
             << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(b"q 200 0 0 100 0 0 cm /Im0 Do Q"),
        image(),
    ])
}

/// A two-page PDF: the scan above, then a page with real text.
fn scan_and_text_pdf() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject \
             << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(b"q 200 0 0 100 0 0 cm /Im0 Do Q"),
        image(),
        // Object numbering: 1 catalog, 2 pages, 3 page one, 4 its content,
        // 5 the image, 6 page two, 7 its content, 8 the font, 9 the CMap.
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << \
             /F1 8 0 R >> >> /Contents 7 0 R >>",
        ),
        stream(b"BT /F1 12 Tf 1 0 0 1 20 50 Tm (Real text) Tj ET"),
        // A font with a `ToUnicode` CMap, because a page whose glyphs cannot be
        // mapped is a page this reader cannot read either - and then it would
        // (rightly) go to the model instead of staying as text.
        text(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 82 /LastChar 82 \
             /Widths [556] /ToUnicode 9 0 R >>",
        ),
        stream(cmap().as_bytes()),
    ])
}

/// An object body that is text.
fn text(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

/// An object body that is a stream of bytes.
fn stream(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A 1×1 DeviceRGB image, unfiltered, as bytes: three samples and no codec.
fn image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
          /BitsPerComponent 8 /Length 3 >>\nstream\n"
        .to_vec();
    out.extend_from_slice(&[0xff, 0x00, 0x00]);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A `ToUnicode` CMap mapping one code to the letter `R`.
fn cmap() -> String {
    "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
     /CMapName /Adobe-Identity-UCS def /CMapType 2 def \
     1 begincodespacerange <52> <52> endcodespacerange \
     1 beginbfchar <0052> <0052> endbfchar endcmap \
     CMapName currentdict /CMap defineresource pop end end"
        .to_owned()
}

/// A `ToUnicode` CMap mapping one code to the letter `C`, for the pages that carry
/// a caption.
fn cmap_c() -> String {
    "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
     /CMapName /Adobe-Identity-UCS def /CMapType 2 def \
     1 begincodespacerange <43> <43> endcodespacerange \
     1 beginbfchar <0043> <0043> endbfchar endcmap \
     CMapName currentdict /CMap defineresource pop end end"
        .to_owned()
}

/// Assembles a PDF from object bodies, with a cross-reference table.
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
        // `write!` into the string rather than `push_str(&format!(..))`: the
        // latter allocates a String per entry to append three bytes.
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

/// A model that answers from a script, counts the pages it was shown, and keeps
/// the first bytes of what it was shown.
struct Scripted {
    answer: Option<String>,
    fail: bool,
    calls: AtomicUsize,
    first_bytes: std::sync::Mutex<Vec<u8>>,
}

impl Scripted {
    fn answering(answer: &str) -> Arc<Self> {
        Self::build(Some(answer.to_owned()), false)
    }

    fn refusing() -> Arc<Self> {
        Self::build(None, true)
    }

    /// A model that answers "there is no text here", which is an answer.
    fn finding_nothing() -> Arc<Self> {
        Self::build(None, false)
    }

    fn build(answer: Option<String>, fail: bool) -> Arc<Self> {
        Arc::new(Self {
            answer,
            fail,
            calls: AtomicUsize::new(0),
            first_bytes: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl TextRecovery for Scripted {
    fn model_name(&self) -> &'static str {
        "scripted"
    }

    fn recover_page(&self, page: &strict_ooxml_ocr::Image) -> Result<Option<Answer>, VisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut first = self
            .first_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if first.is_empty() {
            first.extend_from_slice(&page.bytes[..page.bytes.len().min(16)]);
        }
        if self.fail {
            return Err(VisionError::Unreachable("no model is running".to_owned()));
        }
        Ok(self.answer.clone().map(|text| Answer {
            text,
            model: "scripted".to_owned(),
            version: "v1".to_owned(),
            confidence: None,
        }))
    }
}

/// A picture of a given size, as bytes.
///
/// Every picture in these fixtures is built from the same three samples where the
/// size allows it, so a page with three pictures has three *identical* parts. The
/// reader reports three boxes, and the recovery pass has to treat them as three
/// regions: where a picture sits on the page is not a property of its content.
fn picture(width: u32, height: u32) -> Vec<u8> {
    let samples: Vec<u8> = (0..width * height * 3)
        .map(|index| (index % 251) as u8)
        .collect();
    let mut out = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
        samples.len()
    )
    .into_bytes();
    out.extend_from_slice(&samples);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A page whose only ink is a path: no glyphs, no picture, something drawn.
fn vector_only_pdf() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>"),
        stream(b"10 10 m 190 90 l 2 w S"),
    ])
}

/// A page with three pictures of different sizes, at three positions.
fn three_pictures_pdf() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << \
             /A 5 0 R /B 6 0 R /C 7 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(
            b"q 120 0 0 90 10 5 cm /A Do Q q 40 0 0 30 90 60 cm /B Do Q q 30 0 0 20 150 70 cm \
              /C Do Q",
        ),
        picture(120, 90),
        picture(40, 30),
        picture(30, 20),
    ])
}

/// A page whose only picture is a 12 pt icon: too small to be a line of type.
fn icon_only_pdf() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << \
             /I 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(b"q 12 0 0 12 20 20 cm /I Do Q"),
        picture(12, 12),
    ])
}

/// The recovered paragraphs themselves, in document order.
fn recovered_paragraphs(blocks: &[Block]) -> Vec<&Paragraph> {
    blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter(|paragraph| paragraph.props.style.is_some())
        .collect()
}

/// The text of one paragraph.
fn paragraph_text(paragraph: &Paragraph) -> String {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Run(run) => Some(
                run.content
                    .iter()
                    .filter_map(|content| match content {
                        RunContent::Text(node) => Some(node.text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

/// The text of every paragraph of a document, in order.
fn text_of(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .map(paragraph_text)
        .collect()
}

/// The paragraphs that say something.
///
/// An embedded picture is a paragraph too, and says nothing: the scan's own
/// image comes along with it and must not be mistaken for recovered text.
fn texts_of(blocks: &[Block]) -> Vec<String> {
    text_of(blocks)
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect()
}

/// The paragraphs a model produced, by their text.
fn recovered_of(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter(|paragraph| paragraph.props.style.is_some())
        .map(paragraph_text)
        .collect()
}

/// A page with **real text** and a picture covering a quarter of it: the mixed
/// page, «a paragraph and a photograph».
///
/// The text is the same `ToUnicode` font as `scan_and_text_pdf`, so the page is
/// genuinely `Readable` — which is the whole premise: the reader is not missing
/// this page, it is missing the words *inside* the picture.
fn mixed_page_pdf(height_percent: u32) -> Vec<u8> {
    // A 200 × 100 page; the picture is `picture_share` of its area, so its width
    // runs the full 200 pt and its height is the share times the page's height.
    // A whole number of rows, given as a percentage of the page's height: the
    // option it exercises is a `f64` ratio, and a fixture that had to cast one
    // into the other would be the second place in this file where a number is
    // quietly rounded.
    let height = height_percent.clamp(1, 100);
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << \
             /F1 6 0 R >> /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(
            format!(
                "q 200 0 0 {height} 0 0 cm /Im0 Do Q BT /F1 12 Tf 1 0 0 1 10 90 Tm (Caption) Tj ET"
            )
            .as_bytes(),
        ),
        picture(200, height),
        text(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 67 /LastChar 67 \
             /Widths [556] /ToUnicode 7 0 R >>",
        ),
        stream(cmap_c().as_bytes()),
    ])
}

/// The recovered paragraphs, with the model's own text.
fn mixed_recovered(height_percent: u32) -> (Vec<String>, String, usize) {
    let model = Scripted::answering("Text inside the picture");
    let mut document =
        PdfDocument::open(&mixed_page_pdf(height_percent), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut document,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    (
        recovered_of(&converted.document.body.blocks),
        converted.report.to_string(),
        model.calls.load(Ordering::SeqCst),
    )
}

/// The text inside a big picture on a page we can already read is recovered, and
/// the page's own text is not asked about again.
#[test]
fn a_mixed_page_offers_the_picture_and_not_the_page() {
    let (recovered, report, calls) = mixed_recovered(50);
    assert_eq!(calls, 1, "one picture, one call:\n{report}");
    assert_eq!(
        recovered,
        vec!["Text inside the picture".to_owned()],
        "{report}"
    );
}

/// The call is about the **picture**, not about the page: the recovered paragraph
/// is indented to the picture's left edge, and it is marked as recovered so a
/// reader of the document can tell.
#[test]
fn a_recovered_region_carries_the_picture_indent_and_the_recovered_style() {
    let model = Scripted::answering("Inside");
    let mut document = PdfDocument::open(&mixed_page_pdf(50), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut document,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    let paragraphs: Vec<&Paragraph> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter(|paragraph| paragraph.props.style.is_some())
        .collect();
    assert_eq!(paragraphs.len(), 1);
    // The paragraph refers to the style by id; what a reader of the file sees in
    // Word's styles pane is the style's *name*, so both are checked: a paragraph
    // pointing at a style nobody can find in the pane is a mark nobody sees.
    let style_id = paragraphs[0]
        .props
        .style
        .as_ref()
        .map_or("", StyleId::as_str);
    let style = converted
        .document
        .styles
        .get(&StyleId::new(style_id))
        .unwrap_or_else(|| panic!("the document declares no style {style_id:?}"));
    assert_eq!(
        style.name.as_deref(),
        Some(RECOVERY_STYLE_NAME),
        "and it is named as recovered"
    );
}

/// A picture too small to hold lines of type is not asked about: a logo, a bullet
/// or a rule is not a paragraph, and thirty calls to be told «no text» is a page of
/// text turned into an afternoon.
#[test]
fn a_small_picture_on_a_readable_page_is_not_asked_about() {
    let (recovered, report, calls) = mixed_recovered(2);
    assert_eq!(
        calls, 0,
        "2 % of the page is a mark, not a picture of text:\n{report}"
    );
    assert!(recovered.is_empty(), "{report}");
}

/// Vector art is never a candidate, whatever its size: a chart drawn with paths
/// has its labels as real glyphs, and the reader has them.
#[test]
fn a_vector_drawing_on_a_readable_page_is_not_asked_about() {
    let model = Scripted::answering("Axis labels");
    let bytes = pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << \
             /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(b"10 10 m 190 90 l 2 w S BT /F1 12 Tf 1 0 0 1 10 90 Tm (Caption) Tj ET"),
        text(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 67 /LastChar 67 \
             /Widths [556] /ToUnicode 6 0 R >>",
        ),
        stream(cmap_c().as_bytes()),
    ]);
    let mut document = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
    let converted = convert(
        &mut document,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        0,
        "there is no picture on this page, only a path and glyphs"
    );
    assert!(recovered_of(&converted.document.body.blocks).is_empty());
}

/// The reader sees the fixture as a page with no text layer, which is the whole
/// premise of the pass.
#[test]
fn a_page_of_picture_is_a_page_without_text() {
    let mut document = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    assert_eq!(page.text_layer(), TextLayer::Absent);
    assert!(page.has_ink(), "the image is ink, decoded or not");
    assert_eq!(page.items().len(), 1, "one item, and it is not a glyph");
    let mut both = PdfDocument::open(&scan_and_text_pdf(), PdfLimits::default()).expect("open");
    let text_page = both.page(2).expect("page 2");
    assert_eq!(
        text_page.text_layer(),
        TextLayer::Readable,
        "the second page has real text: {:?}",
        text_page.text()
    );
}

/// Without a model the page's missing text is a **recorded loss**. This is the
/// half of the feature that needs no rasterizer, and it is a silent loss today.
#[test]
fn a_page_without_text_is_reported_when_no_model_is_configured() {
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(&mut reader, &PdfOptions::default()).expect("convert");
    let text = converted.report.to_string();
    assert!(text.contains("text.absent"), "{text}");
    assert!(text.contains("[lost]"), "{text}");
    assert!(
        !converted.report.is_lossless(),
        "a page is missing and it says so"
    );
    assert!(
        texts_of(&converted.document.body.blocks).is_empty(),
        "nothing was invented for the page"
    );
}

/// With a model the page's text comes back as paragraphs, and it is marked in
/// the document and in the report.
#[test]
fn a_model_reads_the_page_and_the_document_says_so() {
    let model = Scripted::answering("INVOICE 42\nAmount due: 100");
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(model.calls.load(Ordering::SeqCst), 1, "one page, one call");
    let shown = model
        .first_bytes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(
        shown.get(..8),
        Some(b"\x89PNG\r\n\x1a\n".as_slice()),
        "the model was shown a picture, not a text request: {shown:?}"
    );
    let text = converted.report.to_string();
    assert!(text.contains("text.recovered"), "{text}");
    assert!(text.contains("[recovered]"), "{text}");
    assert!(text.contains("scripted v1"), "{text}");
    assert_eq!(converted.report.recovered_pages(), 1);
    assert_eq!(converted.report.recovered_paragraphs(), 2);
    let paragraphs = texts_of(&converted.document.body.blocks);
    assert_eq!(paragraphs, vec!["INVOICE 42", "Amount due: 100"]);
    // And the document itself says which text is not from the PDF.
    assert_eq!(
        recovered_of(&converted.document.body.blocks),
        paragraphs,
        "every paragraph of text on the page is marked as recovered"
    );
    assert!(
        converted
            .document
            .styles
            .get(&strict_ooxml_wml::model::ids::StyleId::new("Recovered"))
            .is_some(),
        "the style is declared in the document, not only referenced"
    );
    let style = converted
        .document
        .styles
        .get(&strict_ooxml_wml::model::ids::StyleId::new("Recovered"))
        .expect("declared");
    assert_eq!(
        style.name.as_deref(),
        Some(RECOVERY_STYLE_NAME),
        "a reader of the file sees what the style means"
    );
}

/// A page that already has text is not shown to a model: the glyphs are the
/// truth, and a model's reading of them would be strictly worse.
#[test]
fn a_page_with_text_is_not_shown_to_a_model() {
    let model = Scripted::answering("nonsense");
    let mut reader = PdfDocument::open(&scan_and_text_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        1,
        "the scan was sent and the text page was not"
    );
    let paragraphs = text_of(&converted.document.body.blocks);
    assert!(
        paragraphs.contains(&"Real text".to_owned()),
        "the page that had text still has it: {paragraphs:?}"
    );
    // Exactly one paragraph is a model's: the scan's. The text page's own
    // paragraph is not one, which is the whole claim of this test.
    let recovered: Vec<String> = converted
        .document
        .body
        .blocks
        .iter()
        .filter_map(Block::as_paragraph)
        .filter(|paragraph| paragraph.props.style.is_some())
        .map(paragraph_text)
        .collect();
    assert_eq!(
        recovered,
        vec!["nonsense".to_owned()],
        "only the scan's text came from the model: {paragraphs:?}"
    );
    let text = converted.report.to_string();
    assert!(text.contains("page 1"), "{text}");
    assert!(!text.contains("page 2: 1 paragraph"), "{text}");
}

/// A model that refuses costs the page its text, and nothing else.
#[test]
fn a_model_that_refuses_is_recorded_and_the_rest_converts() {
    let model = Scripted::refusing();
    let mut reader = PdfDocument::open(&scan_and_text_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model)),
    )
    .expect("convert");
    let text = converted.report.to_string();
    assert!(text.contains("text.recovery"), "{text}");
    assert!(text.contains("unreachable"), "{text}");
    let paragraphs = text_of(&converted.document.body.blocks);
    assert!(
        paragraphs.contains(&"Real text".to_owned()),
        "{paragraphs:?}"
    );
    assert_eq!(converted.report.recovered_pages(), 0);
}

/// A model that says "no text here" on a page with ink on it is a failure, not a
/// finding: the page has a picture on it, so there is text somebody could read.
#[test]
fn a_model_that_finds_nothing_on_a_picture_is_a_loss() {
    let model = Scripted::finding_nothing();
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model)),
    )
    .expect("convert");
    let text = converted.report.to_string();
    assert!(text.contains("found no text"), "{text}");
    assert!(text.contains("[lost]"), "{text}");
    assert!(texts_of(&converted.document.body.blocks).is_empty());
}

/// The visual mode gets the recovered text too, with no geometry of its own.
#[test]
fn the_visual_mode_keeps_the_recovered_text_without_inventing_a_position() {
    let model = Scripted::answering("recovered line");
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default()
            .mode(Mode::Visual)
            .text_recovery(Some(model)),
    )
    .expect("convert");
    let paragraphs = recovered_paragraphs(&converted.document.body.blocks);
    assert_eq!(paragraphs.len(), 1);
    // The one piece of geometry a recovered block has is the left edge of the
    // region it came from, which the PDF stated. There is no leading and no size,
    // because a model returns words and not baselines: a fabricated leading would
    // be a lie about where the lines sat.
    assert_eq!(
        paragraphs[0]
            .props
            .indentation
            .and_then(|indent| indent.start)
            .map(|start| start.0),
        Some(0),
        "the scan covers the page, so its left edge is the page's"
    );
    assert!(
        paragraphs[0].props.spacing.is_none(),
        "and nothing else about its position is invented: {:?}",
        paragraphs[0].props.spacing
    );
}

/// A page of **one** picture is a scan, and it is sent as that picture's region.
///
/// The region's own pixels are what the model is shown, not a whole sheet with
/// one scan somewhere on it: the region is rasterized, so the call costs the
/// region's pixels and the report names the region's box.
#[test]
fn a_scan_is_sent_as_its_picture() {
    let model = Scripted::answering("INVOICE 42");
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let text = converted.report.to_string();
    assert!(text.contains("a picture of the page"), "{text}");
    // The report names the region: its size and position, in points.
    assert!(text.contains("200x100"), "{text}");
    assert!(text.contains("at (0,0)"), "{text}");
    let paragraphs = recovered_of(&converted.document.body.blocks);
    assert_eq!(paragraphs, vec!["INVOICE 42".to_owned()]);
}

/// A page whose ink is **not** a picture has no region to name, so the page is the
/// region — and the report says so in as many words.
#[test]
fn a_page_of_vector_ink_is_sent_whole() {
    let model = Scripted::answering("vector text");
    let mut reader = PdfDocument::open(&vector_only_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let text = converted.report.to_string();
    assert!(text.contains("the whole page"), "{text}");
    assert!(text.contains("its ink is not a picture"), "{text}");
}

/// Several pictures on a page are sent **largest first**, because a scan is one
/// big picture and a page of thumbnails is several small ones.
///
/// The order is read from the *indents*, not from the report: the report is
/// sorted by id and detail, so its lines come out alphabetical and a test that
/// read the send order off it would be testing the sort.
#[test]
fn the_largest_picture_is_sent_first_and_the_rest_follow() {
    let model = Scripted::answering("line");
    let mut reader = PdfDocument::open(&three_pictures_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        3,
        "one call per picture"
    );
    assert_eq!(
        indents_of(&converted.document.body.blocks),
        vec![200, 1800, 3000],
        "the 120x90 picture (at 10 pt) is sent before the 40x30 (at 90 pt) and the 30x20 \
         (at 150 pt)"
    );
    let text = converted.report.to_string();
    for expected in ["120x90", "40x30", "30x20"] {
        assert!(text.contains(expected), "{expected} is named:\n{text}");
    }
}

/// The left edge of each recovered region, in twips, in send order.
fn indents_of(blocks: &[Block]) -> Vec<i32> {
    recovered_paragraphs(blocks)
        .into_iter()
        .filter_map(|paragraph| paragraph.props.indentation)
        .filter_map(|indentation| indentation.start)
        .map(|start| start.0)
        .collect()
}

/// A picture too small to be a line of type is not sent, and a page whose only
/// pictures are icons falls back to the page rather than to nothing.
#[test]
fn a_picture_too_small_to_be_text_is_not_sent() {
    let model = Scripted::answering("text");
    let mut reader = PdfDocument::open(&icon_only_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model.clone())),
    )
    .expect("convert");
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let text = converted.report.to_string();
    assert!(text.contains("the whole page"), "{text}");
}

/// The cap is the caller's: a page with ten pictures and a budget of two is asked
/// about two, and the other eight are not a loss (the page's text is unreadable
/// for other reasons) — but the report must not claim otherwise.
#[test]
fn the_region_cap_is_the_callers() {
    let model = Scripted::answering("line");
    let mut reader = PdfDocument::open(&three_pictures_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default()
            .text_recovery(Some(model.clone()))
            .recovery_regions(2, 24.0),
    )
    .expect("convert");
    assert_eq!(model.calls.load(Ordering::SeqCst), 2, "two of the three");
    let text = converted.report.to_string();
    assert!(
        !text.contains("30x20"),
        "the smallest was not sent:\n{text}"
    );
}

/// A recovered paragraph carries the **region's** left edge as its indent, which
/// is a number the PDF stated: the text lands where the picture it came from was.
#[test]
fn a_recovered_paragraph_is_indented_to_its_region() {
    let model = Scripted::answering("indented");
    let mut reader = PdfDocument::open(&three_pictures_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model)),
    )
    .expect("convert");
    // 10 pt, 90 pt and 150 pt of picture left edge, in twips.
    assert_eq!(
        indents_of(&converted.document.body.blocks),
        vec![200, 1800, 3000],
        "one indent per region, in the order the regions were sent"
    );
}

/// Two runs of the same conversion with a scripted model are the same document.
#[test]
fn a_recovered_document_is_reproducible() {
    let convert_once = || {
        let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
        let converted = convert(
            &mut reader,
            &PdfOptions::default().text_recovery(Some(Scripted::answering("same text\ntwice"))),
        )
        .expect("convert");
        (converted.document.body.blocks, converted.report.to_string())
    };
    let (first, first_report) = convert_once();
    let (second, second_report) = convert_once();
    assert_eq!(first, second);
    assert_eq!(first_report, second_report);
}

/// Recovery is a loss of a different colour, not a loss: `is_lossless` must not
/// claim the document is lossless *and* stay silent, and a caller filtering on
/// `Lost` must not be told a recovered page is missing.
#[test]
fn a_recovered_page_is_not_a_lost_page() {
    let model = Scripted::answering("text");
    let mut reader = PdfDocument::open(&scan_pdf(), PdfLimits::default()).expect("open");
    let converted = convert(
        &mut reader,
        &PdfOptions::default().text_recovery(Some(model)),
    )
    .expect("convert");
    let entry = converted
        .report
        .losses()
        .iter()
        .find(|loss| loss.id == "text.recovered")
        .expect("the recovery is recorded");
    assert_eq!(entry.severity, Severity::Recovered);
    assert!(
        converted.report.is_lossless(),
        "the page's text is in the document; it is a model's reading, not a loss.\n{}",
        converted.report
    );
}
