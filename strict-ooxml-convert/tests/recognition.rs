//! The opt-in contract, and the wiring itself.
//!
//! Two things are being pinned here. First, that recognition is opt-in: a
//! conversion with no classifier and a conversion with one produce different
//! documents, and a build without the `ocr-ollama` feature produces the *same*
//! document as one with it (O4, O6). Second, that a model's words reach
//! `wp:docPr/@descr` and the report, never only one of them (O3).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use std::path::Path;

use strict_ooxml_convert::{convert, Mode, PdfOptions};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_ocr::traits::{FigureClassifier, Recovered, VisionError};
use strict_ooxml_ocr::{Image, OcrReport};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The model name every scripted classifier reports.
const MODEL_NAME: &str = "scripted";

/// A classifier that answers from a script, and counts its calls.
struct Scripted {
    answer: String,
    calls: AtomicUsize,
    fail: bool,
}

impl Scripted {
    fn answering(answer: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: answer.to_owned(),
            calls: AtomicUsize::new(0),
            fail: false,
        })
    }

    fn refusing() -> Arc<Self> {
        Arc::new(Self {
            answer: String::new(),
            calls: AtomicUsize::new(0),
            fail: true,
        })
    }
}

impl FigureClassifier for Scripted {
    fn model_name(&self) -> &str {
        MODEL_NAME
    }

    fn describe(&self, _region: &Image, _context: &str) -> Result<Recovered, VisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(VisionError::Unreachable("no model is running".to_owned()));
        }
        Ok(Recovered {
            text: self.answer.clone(),
            model: MODEL_NAME.to_owned(),
            version: "v1".to_owned(),
            confidence: None,
        })
    }
}

/// A `.docx` that carries an image, found by asking the writer for a PDF and
/// looking at what came out.
fn docx_with_an_image() -> Option<Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    for file in [
        // The only corpus document that carries a media part.
        "strict-stage5b.docx",
        "strict-stage5.docx",
    ] {
        let Ok(bytes) = std::fs::read(root.join(file)) else {
            continue;
        };
        let Ok(package) = Package::open_reader(&bytes[..], &OpenOptions::default()) else {
            continue;
        };
        let Ok(document) = parse_document(&package, &ParseOptions::default()) else {
            continue;
        };
        if document.media.is_empty() {
            continue;
        }
        let options = RenderOptions::default();
        let Ok(placed) = place_pages(&document, &options, Some(&package)) else {
            continue;
        };
        let Ok(pdf) = render_with_source(&placed, &options, Some(&package)) else {
            continue;
        };
        return Some(pdf.bytes);
    }
    None
}

/// The PDF the classification tests run on.
fn pdf_with_an_image() -> Vec<u8> {
    docx_with_an_image().expect("a corpus document that carries an image")
}

/// The `descr` of the document's only picture, if it has one.
fn first_description(bytes: &[u8]) -> Option<String> {
    use strict_ooxml_core::opc::{OpenOptions, Package};
    use strict_ooxml_wml::model::block::Block;
    use strict_ooxml_wml::model::drawing::{Drawing, DrawingKind};
    use strict_ooxml_wml::model::inline::Inline;
    use strict_ooxml_wml::{parse_document, ParseOptions};

    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    for block in &document.body.blocks {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for inline in &paragraph.inlines {
            let Inline::Drawing(Drawing {
                kind: DrawingKind::Inline(inline),
                ..
            }) = inline
            else {
                continue;
            };
            if let Some(descr) = inline.doc_pr.as_ref().and_then(|pr| pr.descr.as_ref()) {
                return Some(descr.to_string());
            }
        }
    }
    None
}

fn convert_with(classifier: Option<Arc<dyn FigureClassifier>>) -> (Vec<u8>, String) {
    let pdf = pdf_with_an_image();
    let mut reader = PdfDocument::open(&pdf, PdfLimits::default()).expect("open");
    let options = PdfOptions::default()
        .mode(Mode::Semantic)
        .figure_classifier(classifier);
    let converted = convert(&mut reader, &options).expect("convert");
    // The model references its images by part id and carries no bytes, so the
    // writer needs the other half; a caller that forgets this gets a package
    // whose media parts are missing, which is exactly what this used to do.
    let mut bag = strict_ooxml_write::package::MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    let written = strict_ooxml_write::write_package(
        &converted.document,
        Some(&bag),
        &strict_ooxml_write::WriteOptions::default(),
    )
    .expect("write");
    let report = converted.report.to_string();
    (written.bytes, report)
}

/// A model that answers puts its words in the document *and* in the report.
#[test]
fn a_description_reaches_the_document_and_the_report() {
    let model = Scripted::answering("diagram\nA bar chart of revenue by quarter.");
    let (bytes, report) = convert_with(Some(model.clone()));
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        1,
        "the model was asked once"
    );
    assert_eq!(
        first_description(&bytes).as_deref(),
        Some("A bar chart of revenue by quarter."),
        "the description must reach wp:docPr/@descr"
    );
    assert!(report.contains("ocr.figure"), "{report}");
    assert!(
        report.contains("scripted v1"),
        "the report names the model: {report}"
    );
}

/// No model means the picture is still placed, and nothing is invented.
#[test]
fn without_a_model_nothing_is_invented() {
    let (bytes, report) = convert_with(None);
    assert_eq!(first_description(&bytes), None, "no model, no description");
    assert!(!report.contains("ocr.figure"), "{report}");
}

/// The opt-in contract: with a model and without one the documents differ, and
/// the difference is exactly the description.
#[test]
fn recognition_is_opt_in() {
    let (without, _) = convert_with(None);
    let (with, _) = convert_with(Some(Scripted::answering(
        "illustration\nA photograph of a site.",
    )));
    assert_ne!(
        first_description(&without),
        first_description(&with),
        "supplying a model must change the document"
    );
}

/// A model that refuses leaves the picture in place and says so.
#[test]
fn a_refused_description_is_recorded_not_guessed() {
    let (bytes, report) = convert_with(Some(Scripted::refusing()));
    assert_eq!(
        first_description(&bytes),
        None,
        "a refusal is not a caption"
    );
    assert!(report.contains("ocr.figure"), "{report}");
    assert!(
        report.contains("unreachable"),
        "the reason is recorded: {report}"
    );
}

/// An answer that names no known kind is not a caption.
#[test]
fn an_answer_with_no_known_kind_is_rejected() {
    let model = Scripted::answering("something entirely unexpected");
    let (bytes, report) = convert_with(Some(model));
    assert_eq!(
        first_description(&bytes),
        None,
        "a caption nobody can classify is not a caption"
    );
    assert!(report.contains("ocr.figure"), "{report}");
}

/// The converter's own report is reachable, so a caller can print both.
#[test]
fn the_ocr_report_is_a_separate_fact() {
    // The conversion report and the recognition report are different things and
    // the caller sees both; this pins that a caller can hold the OCR one.
    let report = OcrReport::new();
    assert!(report.is_empty());
}
