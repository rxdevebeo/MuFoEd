//! Acceptance for the PDF backend (`STAGE-8-TASK.md` §4).
//!
//! The oracle is `lopdf`, which shares no code with the writer: it re-reads the
//! file we produced, so a PDF that is merely self-consistent — but malformed, or
//! with an unresolvable font — fails here rather than in a reader.
//!
//! - **SC-1** two renders of one document are byte-identical;
//! - **SC-6** the page count matches the SVG backend's, because both come from
//!   the same placement;
//! - **SC-7** every used font is embedded and its text is extractable.

// The `ToUnicode` in prose is a PDF key, and the fixtures below are built with
// `Default::default()` for model types that have no `new()`.
#![allow(clippy::doc_markdown, clippy::default_trait_access)]

use std::path::Path;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_render_pdf::{render, render_with_source, PdfReport};
use strict_ooxml_render_svg::{
    layout::Item, place_pages, render_with_media, MediaSource, RenderOptions,
};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The Strict documents the render runs on.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict");
    for (name, file) in [
        ("strict-text", "strict-text.docx"),
        ("strict-profile", "strict-profile.docx"),
        ("strict-math", "05-strict-math-simple.docx"),
        ("strict-shapes", "07-strict-drawingml-shapes.docx"),
        // Rounded rectangles and a page border: the fixture whose arcs were
        // dropped from every PDF until the pixel gate measured the page.
        ("strict-stage5b", "strict-stage5b.docx"),
    ] {
        if let Ok(bytes) = std::fs::read(root.join(file)) {
            out.push((name, bytes));
        }
    }
    assert!(!out.is_empty(), "no Strict corpus under {}", root.display());
    out
}

fn open(bytes: &[u8]) -> Package {
    Package::open_reader(bytes, &OpenOptions::default())
        .unwrap_or_else(|error| panic!("open: {error}"))
}

/// Renders a document to PDF the way the CLI does, with media resolved.
fn to_pdf(bytes: &[u8]) -> (Vec<u8>, Vec<strict_ooxml_render_svg::Page>, PdfReport) {
    let package = open(bytes);
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let options = RenderOptions::default();
    let pages = place_pages(&document, &options, Some(&package)).expect("place");
    let output = render_with_source(&pages, &options, Some(&package)).expect("render");
    let svg = render_with_media(&document, &options, Some(&package)).expect("svg");
    (output.bytes, svg, output.report)
}

/// The text a reader gets back out of the PDF, via an independent reader.
fn extracted_text(pdf: &[u8]) -> String {
    let document = lopdf::Document::load_mem(pdf).expect("the PDF parses");
    let mut out = String::new();
    for id in document.page_iter() {
        out.push_str(&String::from_utf8_lossy(&document.get_page_content(id)));
        out.push('\n');
    }
    out
}

#[test]
fn regular_postscript_family_names_embed_and_preserve_text() {
    use strict_ooxml_testkit::DocxBuilder;

    for (alias, canonical) in [
        ("ArialMT", "Arial"),
        ("TimesNewRomanPSMT", "Times New Roman"),
    ] {
        let body = |family: &str| {
            use std::fmt::Write;
            let mut out = String::new();
            for (index, style) in ["", "<w:b/>", "<w:i/>", "<w:b/><w:i/>"].iter().enumerate() {
                write!(out,
                    "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\"/>{style}</w:rPr><w:t>Alias{index}</w:t></w:r></w:p>"
                ).expect("write fixture");
            }
            out
        };
        let alias_docx = DocxBuilder::strict().body(&body(alias)).build();
        let canonical_docx = DocxBuilder::strict().body(&body(canonical)).build();
        let (pdf, _, report) = to_pdf(&alias_docx);
        assert!(report.losses().is_empty(), "{alias}: {:?}", report.losses());
        assert_eq!(
            pdf,
            to_pdf(&canonical_docx).0,
            "{alias}: geometry or font differs"
        );
        let reader = lopdf::Document::load_mem(&pdf).expect("independent PDF reader");
        let text = reader
            .extract_text(&[1])
            .expect("ToUnicode text extraction");
        for index in 0..4 {
            assert!(text.contains(&format!("Alias{index}")), "{alias}: {text:?}");
        }
    }
}

#[test]
fn non_bmp_math_font_text_is_extractable() {
    use strict_ooxml_testkit::DocxBuilder;
    let bytes = DocxBuilder::strict().body(
        "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Cambria Math\" w:hAnsi=\"Cambria Math\"/></w:rPr><w:t>Math𝑥</w:t></w:r></w:p>"
    ).build();
    let (pdf, _, report) = to_pdf(&bytes);
    assert!(report.losses().is_empty(), "{:?}", report.losses());
    let reader = lopdf::Document::load_mem(&pdf).expect("independent PDF reader");
    let text = reader
        .extract_text(&[1])
        .expect("ToUnicode text extraction");
    assert!(text.contains("Math𝑥"), "{text:?}");
}

#[test]
fn a_missing_glyph_is_not_reported_as_a_missing_face() {
    use strict_ooxml_testkit::DocxBuilder;
    let bytes = DocxBuilder::strict()
        .body("<w:p><w:r><w:t>&#x10FFFD;</w:t></w:r></w:p>")
        .build();
    let (_, _, report) = to_pdf(&bytes);
    let ids: Vec<_> = report.losses().iter().map(|loss| loss.id).collect();
    assert!(ids.contains(&"pdf.font.no_glyph"), "{:?}", report.losses());
    assert!(!ids.contains(&"pdf.font.missing"), "{:?}", report.losses());
}

#[test]
fn east_asian_script_face_is_embedded_by_pdf_backend() {
    use strict_ooxml_testkit::DocxBuilder;

    let render = |family: &str| {
        let body = format!(
            "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:eastAsia=\"{family}\"/></w:rPr><w:t>漢字。</w:t></w:r></w:p>"
        );
        let bytes = DocxBuilder::strict().body(&body).build();
        to_pdf(&bytes).0
    };
    let arial = render("Arial");
    let times = render("Times New Roman");
    let font_names = |pdf: &[u8]| {
        let document = lopdf::Document::load_mem(pdf).expect("PDF parses");
        document
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .filter(|dictionary| {
                dictionary
                    .get(b"Type")
                    .ok()
                    .and_then(|value| value.as_name().ok())
                    == Some(&b"Font"[..])
            })
            .filter_map(|dictionary| {
                dictionary
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|value| value.as_name().ok())
                    .map(<[u8]>::to_vec)
            })
            .collect::<Vec<_>>()
    };
    let arial_names = font_names(&arial);
    let times_names = font_names(&times);
    assert!(arial_names
        .iter()
        .any(|name| name.windows(5).any(|part| part == b"Arimo")));
    assert!(
        times_names
            .iter()
            .any(|name| name.windows(5).any(|part| part == b"Tinos")),
        "{times_names:?}"
    );
}

/// SC-7: every font the pages use is embedded as a file, with an `Identity-H`
/// encoding and a `ToUnicode` CMap: the two things that make the text
/// selectable and searchable rather than glyph ids.
#[test]
fn every_used_font_is_embedded() {
    for (name, bytes) in corpus() {
        let (pdf, _svg, _report) = to_pdf(&bytes);
        let document =
            lopdf::Document::load_mem(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut embedded = 0usize;
        let mut type0 = 0usize;
        let mut encodings = 0usize;
        let mut to_unicode = 0usize;
        for (id, object) in &document.objects {
            let Ok(dictionary) = object.as_dict() else {
                continue;
            };
            if dictionary.get(b"FontFile2").is_ok() || dictionary.get(b"FontFile3").is_ok() {
                embedded += 1;
                let bounds = dictionary
                    .get(b"FontBBox")
                    .and_then(lopdf::Object::as_array)
                    .expect("embedded font requires FontBBox");
                assert_eq!(bounds.len(), 4);
                assert!(bounds.iter().all(|value| value.as_float().is_ok()));
            }
            let is_font = dictionary
                .get(b"Type")
                .ok()
                .and_then(|value| value.as_name().ok())
                == Some(&b"Font"[..]);
            if !is_font {
                continue;
            }
            // A `Type0` font names its descendant, which is where the program and
            // the descriptor live; the check is that every one of them is
            // complete, not that some of them are.
            let subtype = dictionary
                .get(b"Subtype")
                .ok()
                .and_then(|value| value.as_name().ok())
                .map(<[u8]>::to_vec);
            if matches!(subtype.as_deref(), Some(b"CIDFontType0" | b"CIDFontType2")) {
                let info = dictionary
                    .get(b"CIDSystemInfo")
                    .and_then(lopdf::Object::as_dict)
                    .unwrap_or_else(|error| {
                        panic!("{name}: CID font {} lacks CIDSystemInfo: {error}", id.0)
                    });
                assert_eq!(info.get(b"Registry").unwrap().as_str().unwrap(), b"Adobe");
                assert_eq!(
                    info.get(b"Ordering").unwrap().as_str().unwrap(),
                    b"Identity"
                );
                assert_eq!(info.get(b"Supplement").unwrap().as_i64().unwrap(), 0);
            }
            if dictionary.get(b"Encoding").is_ok() {
                if subtype.as_deref() == Some(b"Type0") {
                    type0 += 1;
                }
                encodings += 1;
            }
            if let Ok(cmap_id) = dictionary
                .get(b"ToUnicode")
                .and_then(lopdf::Object::as_reference)
            {
                let cmap = document
                    .get_object(cmap_id)
                    .unwrap_or_else(|error| panic!("{name}: dangling ToUnicode: {error}"));
                let content = cmap
                    .as_stream()
                    .and_then(lopdf::Stream::decompressed_content)
                    .unwrap_or_else(|error| panic!("{name}: unreadable ToUnicode CMap: {error}"));
                let text = String::from_utf8_lossy(&content).to_string();
                assert!(
                    text.contains("beginbfchar"),
                    "{name}: the ToUnicode CMap of object {} has no character map",
                    id.0
                );
                to_unicode += 1;
            }
        }
        assert!(embedded > 0, "{name}: no embedded font program in the PDF");
        assert_eq!(type0, encodings, "{name}: a composite font has no encoding");
        assert!(to_unicode > 0, "{name}: no font carries a ToUnicode CMap");
        assert_eq!(
            type0, to_unicode,
            "{name}: a composite font is missing its ToUnicode CMap"
        );
    }
}

/// SC-6: the page count is the one the SVG backend produced, and the media box
/// is a real page rather than a default.
#[test]
fn the_page_count_matches_the_svg_backend() {
    for (name, bytes) in corpus() {
        let (pdf, svg, _report) = to_pdf(&bytes);
        let document =
            lopdf::Document::load_mem(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            document.get_pages().len(),
            svg.len(),
            "{name}: the two backends disagree on the page count"
        );
        for id in document.page_iter() {
            let dictionary = document
                .get_dictionary(id)
                .unwrap_or_else(|error| panic!("{name}: no page dictionary: {error}"));
            let box_dict = dictionary
                .get(b"MediaBox")
                .unwrap_or_else(|error| panic!("{name}: no media box: {error}"));
            let array = box_dict
                .as_array()
                .unwrap_or_else(|error| panic!("{name}: media box is not an array: {error}"));
            assert_eq!(array.len(), 4, "{name}: a media box has four numbers");
            let numbers: Vec<f32> = array
                .iter()
                .map(|item| item.as_float().unwrap_or_default())
                .collect();
            assert!(
                numbers[2] > numbers[0] && numbers[3] > numbers[1],
                "{name}: degenerate media box {numbers:?}"
            );
        }
    }
}

/// SC-1: the same document renders to the same bytes every time.
#[test]
fn rendering_is_reproducible() {
    for (name, bytes) in corpus() {
        let (first, _svg, first_report) = to_pdf(&bytes);
        let (second, _svg, second_report) = to_pdf(&bytes);
        assert_eq!(first, second, "{name}: the render is not reproducible");
        assert_eq!(
            first_report.to_string(),
            second_report.to_string(),
            "{name}: the report is not reproducible"
        );
    }
}

/// The text operators survive, so the file is not a page of drawn boxes.
#[test]
fn the_content_stream_draws_text() {
    for (name, bytes) in corpus() {
        let (pdf, _svg, _report) = to_pdf(&bytes);
        let text = extracted_text(&pdf);
        assert!(
            text.contains("BT") && text.contains("Tj") && text.contains("ET"),
            "{name}: no text object in the content stream"
        );
    }
}

/// A document with no text still produces a valid, openable file.
#[test]
fn an_empty_document_produces_a_valid_page() {
    let document = strict_ooxml_wml::model::Document {
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
        source: strict_ooxml_wml::model::document::DocumentSource {
            main_document: strict_ooxml_core::part::PartId::new("/word/document.xml"),
            styles: None,
            numbering: None,
            settings: None,
            footnotes: None,
            endnotes: None,
            theme: None,
            font_table: None,
        },
    };
    let options = RenderOptions::default();
    let pages = place_pages(&document, &options, None).expect("place");
    let output = render(&pages, &options).expect("render");
    assert_eq!(output.page_count, pages.len());
    let parsed = lopdf::Document::load_mem(&output.bytes).expect("the PDF parses");
    assert_eq!(parsed.get_pages().len(), pages.len());
}

/// A picture whose bytes are available is embedded; without a source it is a
/// recorded placeholder and still a valid file.
#[test]
fn pictures_are_embedded_or_recorded() {
    for (name, bytes) in corpus() {
        let package = open(&bytes);
        let document = parse_document(&package, &ParseOptions::default()).expect("parse");
        if document.media.is_empty() {
            continue;
        }
        let options = RenderOptions::default();
        let with_media = place_pages(&document, &options, Some(&package)).expect("place");
        let pdf = render_with_source(&with_media, &options, Some(&package)).expect("render");
        let text = String::from_utf8_lossy(&pdf.bytes).to_string();
        assert!(text.contains("/XObject"), "{name}: no image was embedded");

        let without_media = place_pages(&document, &options, None).expect("place");
        let bare = render(&without_media, &options).expect("render");
        let parsed = lopdf::Document::load_mem(&bare.bytes).expect("the PDF still parses");
        assert!(!parsed.get_pages().is_empty());
    }
}

/// The reader's own strictness is the check that matters for the CLI path: a
/// package opened under `StrictOnly` is a Strict document, and the PDF is written
/// from exactly that model.
#[test]
fn the_source_package_is_strict() {
    for (name, bytes) in corpus() {
        let options = OpenOptions {
            conformance: ConformancePolicy::StrictOnly,
            ..OpenOptions::default()
        };
        Package::open_reader(&bytes[..], &options)
            .unwrap_or_else(|error| panic!("{name} is not Strict: {error}"));
    }
}

/// Every picture's resource name in the rendered file is unique, and every name
/// the content stream draws with is one the page's `/XObject` dictionary answers
/// to.
///
/// Two pictures whose part names clean to the same resource name —
/// `/word/media/a-b.png` and `/word/media/a_b.png` — would otherwise get one name
/// between them, and a page's `/XObject` dictionary written with both is a
/// dictionary with two entries under one key: the file still opens, and one of the
/// two pictures is simply not drawn. That is the failure `STAGE-8-OPEN.md` Q-4
/// predicted.
///
/// The rule that resolves it is tested where it lives, in `name_the_images`'s unit
/// test in `src/document.rs`. What is checked here is the property on real output,
/// read back by `lopdf` — the oracle the rest of this file uses, and one that
/// shares no code with the writer.
#[test]
fn every_image_resource_name_is_unique_and_drawn() {
    for (name, bytes) in corpus() {
        let (pdf, _svg, _report) = to_pdf(&bytes);
        let document =
            lopdf::Document::load_mem(&pdf).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut seen: Vec<String> = Vec::new();
        let mut drawn: Vec<String> = Vec::new();
        for id in document.page_iter() {
            let content = String::from_utf8_lossy(&document.get_page_content(id)).into_owned();
            // Every `/Im… Do` the stream draws.
            for token in content.split_whitespace() {
                if let Some(resource) = token.strip_prefix('/') {
                    if let Some(name) = resource.strip_suffix("Do") {
                        assert!(
                            name.starts_with("Im"),
                            "{name}: a name that is not ours came from the writer: {token}"
                        );
                        drawn.push(name.to_owned());
                    }
                }
            }
            // Every `/XObject` entry the page offers, and each name at most once.
            let Ok(resources) = document
                .get_dictionary(id)
                .and_then(|dictionary| dictionary.get(b"Resources"))
                .and_then(lopdf::Object::as_dict)
                .and_then(|objects| objects.get(b"XObject"))
                .and_then(lopdf::Object::as_dict)
            else {
                continue;
            };
            for key in resources.iter().map(|(key, _)| key) {
                let key = String::from_utf8_lossy(key).into_owned();
                assert!(
                    !seen.contains(&key),
                    "{name}: resource {key} is written twice, so one of the two pictures is \
                     never drawn"
                );
                seen.push(key);
            }
        }
        for resource in &drawn {
            assert!(
                seen.iter().any(|key| key == resource),
                "{name}: the content stream draws /{resource} and no /XObject entry answers to it"
            );
        }
    }
}

/// The media source the render is given is the one the crate defines, so the
/// meta-crate can pass its own package without an adapter.
#[test]
fn the_package_is_a_media_source() {
    fn assert_source<S: MediaSource>(_: &S) {}
    for (_name, bytes) in corpus() {
        assert_source(&open(&bytes));
    }
}

/// The path data the layout emits must stay inside the grammar
/// `strict-ooxml-render-pdf/src/path.rs` parses.
///
/// The parser skips a command it does not know instead of failing, which is the
/// right behaviour for a drawing and a terrible way to find out that a drawing
/// lost a corner: the PDF is still a valid page, just a different one. `A` was
/// skipped like that until stage 8's PDF pixel gate measured the page and found
/// that every rounded rectangle in every PDF this crate wrote had square
/// corners, and that `07-strict-drawingml-shapes`' circle was not there at all.
///
/// So the claim "`Q`, `T`, `S` and `R` are not emitted by any producer" is
/// **checked** here rather than asserted in a doc comment. The day a producer
/// emits one, this test names it and the parser has to grow.
#[test]
fn every_producer_path_uses_only_the_supported_commands() {
    /// The commands `path.rs` parses. `M`, `L`, `H`, `V`, `C` and `A`, in either
    /// case; a lowercase command after a `moveto` means the implicit-lineto form
    /// of the same grammar and parses.
    const SUPPORTED: &[char] = &[
        'M', 'm', 'L', 'l', 'H', 'h', 'V', 'v', 'C', 'c', 'A', 'a', 'Z', 'z',
    ];

    let mut unsupported: Vec<(String, char)> = Vec::new();
    let mut arcs = 0usize;
    for (name, bytes) in corpus() {
        let package = open(&bytes);
        let document = parse_document(&package, &ParseOptions::default()).expect("parse");
        let pages =
            place_pages(&document, &RenderOptions::default(), Some(&package)).expect("place");
        for page in &pages {
            for item in &page.items {
                let Item::Path(shape) = item else {
                    continue;
                };
                for ch in shape.d.chars() {
                    if !ch.is_ascii_alphabetic() {
                        continue;
                    }
                    if ch == 'A' || ch == 'a' {
                        arcs += 1;
                    }
                    if !SUPPORTED.contains(&ch) {
                        unsupported.push((name.to_owned(), ch));
                    }
                }
            }
        }
    }
    assert!(
        unsupported.is_empty(),
        "the layout emits path commands `src/path.rs` does not parse, so the PDF silently \
         drops that geometry: {unsupported:?}"
    );
    // The other direction too: a parser that quietly stopped reading `A` would
    // pass the test above, and the corners would be gone with nothing failing.
    // So the corpus has to actually carry the geometry this test watches.
    assert!(
        arcs >= 4,
        "the corpus is expected to carry elliptical arcs and carried {arcs}: a fixture \
         change has removed the geometry this test exists to watch"
    );
}

/// AUD-80: a picture in a header on a three-page document is drawn on every
/// page, and the image XObject is written once.
///
/// Before the fix the XObject lived only on page 1's `/Resources`: pages 2 and 3
/// still said `/Im… Do` but had no entry answering to that name, so a reader saw
/// one placed image instead of three and the picture vanished from later pages.
#[test]
#[allow(clippy::too_many_lines)]
fn a_header_image_is_on_every_page_as_one_xobject() {
    use strict_ooxml_testkit::DocxBuilder;

    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 2);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer
            .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .expect("data");
    }
    let picture = concat!(
        "<w:drawing><wp:inline>",
        "<wp:extent cx=\"914400\" cy=\"914400\"/>",
        "<wp:docPr id=\"1\" name=\"hdr\"/>",
        "<a:graphic><a:graphicData uri=\"http://purl.oclc.org/ooxml/drawingml/picture\">",
        "<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"hdr\"/><pic:cNvPicPr/>",
        "</pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rIdImg\"/></pic:blipFill>",
        "<pic:spPr><a:xfrm><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm></pic:spPr>",
        "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
    );
    let body = concat!(
        "<w:p><w:r><w:t>One</w:t></w:r></w:p>",
        "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>",
        "<w:p><w:r><w:t>Two</w:t></w:r></w:p>",
        "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>",
        "<w:p><w:r><w:t>Three</w:t></w:r></w:p>",
        "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHdr\"/>",
        "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>",
        "<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" ",
        "w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>"
    );
    let header = format!("<w:p><w:r>{picture}</w:r></w:p>");
    let bytes = DocxBuilder::strict()
        .body(body)
        .rel("rIdHdr", "header", "header1.xml")
        .content_type(
            "/word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        )
        .content_type("/word/media/header.png", "image/png")
        .part_xml("word/header1.xml", "w:hdr", &header)
        .part(
            "word/_rels/header1.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdImg" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/image" Target="media/header.png"/>
</Relationships>"#
                .to_vec(),
        )
        .part("word/media/header.png", png)
        .build();

    let (pdf, svg, _report) = to_pdf(&bytes);
    assert_eq!(svg.len(), 3, "fixture must be three pages");

    let document = lopdf::Document::load_mem(&pdf).expect("the PDF parses");
    let mut page_image_refs: Vec<lopdf::ObjectId> = Vec::new();
    let mut drawn = 0usize;
    for id in document.page_iter() {
        let content = String::from_utf8_lossy(&document.get_page_content(id)).into_owned();
        let resources = document
            .get_dictionary(id)
            .and_then(|dictionary| dictionary.get(b"Resources"))
            .and_then(lopdf::Object::as_dict)
            .and_then(|objects| objects.get(b"XObject"))
            .and_then(lopdf::Object::as_dict)
            .unwrap_or_else(|error| panic!("page {id:?} has no /XObject: {error}"));
        let mut page_refs = Vec::new();
        for (name, value) in resources {
            let name = String::from_utf8_lossy(name).into_owned();
            assert!(
                content.contains(&format!("/{name}"))
                    && content.split_whitespace().any(|token| token == "Do"),
                "page {id:?}: resource /{name} is listed but never drawn; content={content:?}"
            );
            drawn += 1;
            let reference = value
                .as_reference()
                .unwrap_or_else(|error| panic!("XObject entry is not a reference: {error}"));
            page_refs.push(reference);
        }
        assert_eq!(
            page_refs.len(),
            1,
            "each page should name exactly the header picture: {page_refs:?}"
        );
        page_image_refs.push(page_refs[0]);
    }
    assert_eq!(
        drawn, 3,
        "content streams should draw the header on every page"
    );
    assert_eq!(page_image_refs.len(), 3);
    assert!(
        page_image_refs.iter().all(|id| *id == page_image_refs[0]),
        "every page must point at the same XObject: {page_image_refs:?}"
    );

    let colour_images = document
        .objects
        .iter()
        .filter(|(_, object)| {
            object.as_stream().ok().is_some_and(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|value| value.as_name().ok())
                    == Some(&b"Image"[..])
                    && stream
                        .dict
                        .get(b"ColorSpace")
                        .ok()
                        .and_then(|value| value.as_name().ok())
                        == Some(&b"DeviceRGB"[..])
            })
        })
        .count();
    assert_eq!(
        colour_images, 1,
        "the header picture must be one XObject, not one per page"
    );
}

/// Direct `w:i` together with the paragraph mark selects the italic face.
#[test]
fn direct_italic_embeds_the_italic_face() {
    let bytes = strict_ooxml_testkit::docx::DocxBuilder::strict()
        .body(
            "<w:p><w:pPr><w:rPr><w:b/><w:i/></w:rPr></w:pPr>\
             <w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>Hi</w:t></w:r></w:p>",
        )
        .build();
    let (pdf, svg, _) = to_pdf(&bytes);
    assert!(
        svg.iter()
            .any(|page| page.svg.contains("font-style=\"italic\"")),
        "svg italic"
    );
    let raw = String::from_utf8_lossy(&pdf);
    assert!(
        raw.contains("/ItalicAngle -12") || raw.contains("/ItalicAngle -12.0"),
        "the embedded face is italic"
    );
    assert!(!extracted_text(&pdf).is_empty(), "the page draws text");
}

#[test]
fn explicit_font_assets_subset_exact_instances_and_keep_unicode_after_options_drop() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{
        resolve_face, FontAsset, FontAssetManifest, FontAssetRegistry, FontEmbeddingPermission,
        FontRegistryLimits,
    };
    use strict_ooxml_testkit::DocxBuilder;
    let assets: Vec<_> = [
        ("AliasA", "Arial", false),
        ("AliasB", "Times New Roman", false),
        ("AliasA", "Arial", true),
    ]
    .into_iter()
    .map(|(request, source, bold)| {
        let face = resolve_face(source, bold, false).unwrap();
        FontAsset {
            bytes: Arc::from(face.bytes),
            manifest: FontAssetManifest {
                family: request.into(),
                resolved_family: "same declared name".into(),
                bold,
                italic: false,
                resource_hash: face.resource_hash,
                face_index: 0,
                variations: face.instance_key().variations,
                source: "bundled fixture".into(),
                license_id: "OFL-1.1".into(),
                license_reference: "bundled fonts licenses".into(),
                embedding: FontEmbeddingPermission::EmbedAndSubset,
            },
        }
    })
    .collect();
    let registry =
        Arc::new(FontAssetRegistry::from_assets(assets, FontRegistryLimits::default()).unwrap());
    let options = RenderOptions::default().font_assets(Arc::clone(&registry));
    let bytes = DocxBuilder::strict().body("<w:p><w:r><w:rPr><w:rFonts w:ascii=\"AliasA\"/></w:rPr><w:t>office AV e&#x301;</w:t></w:r></w:p><w:p><w:r><w:rPr><w:rFonts w:ascii=\"AliasB\"/></w:rPr><w:t>Serif text</w:t></w:r></w:p><w:p><w:r><w:rPr><w:rFonts w:ascii=\"AliasA\"/><w:b/></w:rPr><w:t>Bold text</w:t></w:r></w:p>").build();
    let package = open(&bytes);
    let document = parse_document(&package, &ParseOptions::default()).unwrap();
    let pages = place_pages(&document, &options, None).unwrap();
    drop(options);
    drop(registry);
    let output = render(&pages, &RenderOptions::default()).unwrap();
    assert!(
        output.report.losses().is_empty(),
        "{:?}",
        output.report.losses()
    );
    assert_eq!(output.embedded_faces.len(), 3);
    assert!(output
        .embedded_faces
        .iter()
        .all(|key| key.instance.is_some()));
    let reader = lopdf::Document::load_mem(&output.bytes).unwrap();
    let raw_extracted = reader.extract_text(&[1]).unwrap();
    let extracted = raw_extracted
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in ["office AV e\u{301}", "Serif text", "Bold text"] {
        assert!(extracted.contains(expected), "{extracted:?}");
    }
    assert_eq!(
        output.bytes,
        render(&pages, &RenderOptions::default()).unwrap().bytes
    );
    // Empty admission must leave the established default PDF bytes unchanged.
    let plain = place_pages(&document, &RenderOptions::default(), None).unwrap();
    let empty = RenderOptions::default().font_assets(Arc::new(FontAssetRegistry::default()));
    let empty_pages = place_pages(&document, &empty, None).unwrap();
    assert_eq!(
        render(&plain, &RenderOptions::default()).unwrap().bytes,
        render(&empty_pages, &empty).unwrap().bytes
    );
}

fn collection_asset() -> strict_ooxml_render_svg::font::FontAsset {
    use strict_ooxml_render_svg::font::{
        hash_face_bytes, resolve_face, FontAsset, FontAssetManifest, FontEmbeddingPermission,
    };
    let mut bytes = b"ttcf\x00\x01\x00\x00\x00\x00\x00\x02".to_vec();
    bytes.resize(20, 0);
    for (index, family) in ["Arial", "Times New Roman"].iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let offset = u32::try_from(bytes.len()).unwrap();
        bytes[12 + index * 4..16 + index * 4].copy_from_slice(&offset.to_be_bytes());
        let mut program = resolve_face(family, false, false).unwrap().bytes.to_vec();
        let tables = usize::from(u16::from_be_bytes([program[4], program[5]]));
        for table in 0..tables {
            let pos = 12 + table * 16 + 8;
            let old = u32::from_be_bytes(program[pos..pos + 4].try_into().unwrap());
            program[pos..pos + 4].copy_from_slice(&(old + offset).to_be_bytes());
        }
        bytes.extend(program);
    }
    FontAsset {
        manifest: FontAssetManifest {
            family: "Collection Face".into(),
            resolved_family: "Tinos".into(),
            bold: false,
            italic: false,
            resource_hash: hash_face_bytes(&bytes),
            face_index: 1,
            variations: resolve_face("Times New Roman", false, false)
                .unwrap()
                .instance_key()
                .variations,
            source: "two bundled fonts test collection".into(),
            license_id: "OFL-1.1".into(),
            license_reference: "bundled font licenses".into(),
            embedding: FontEmbeddingPermission::EmbedAndSubset,
        },
        bytes: std::sync::Arc::from(bytes),
    }
}

#[test]
fn pdf_subsets_collection_face_one_and_matches_single_face_geometry() {
    use std::sync::Arc;
    use strict_ooxml_render_svg::font::{FontAssetRegistry, FontRegistryLimits};
    use strict_ooxml_testkit::DocxBuilder;
    let registry = Arc::new(
        FontAssetRegistry::from_assets(vec![collection_asset()], FontRegistryLimits::default())
            .unwrap(),
    );
    let options = RenderOptions::default().font_assets(registry);
    let body = |family| {
        format!("<w:p><w:r><w:rPr><w:rFonts w:ascii=\"{family}\"/></w:rPr><w:t>AV office</w:t></w:r></w:p>")
    };
    let docx = DocxBuilder::strict().body(&body("Collection Face")).build();
    let package = open(&docx);
    let document = parse_document(&package, &ParseOptions::default()).unwrap();
    let pages = place_pages(&document, &options, None).unwrap();
    let output = render(&pages, &options).unwrap();
    assert!(
        output.report.losses().is_empty(),
        "{:?}",
        output.report.losses()
    );
    assert_eq!(
        output.embedded_faces[0]
            .instance
            .as_ref()
            .unwrap()
            .face_index,
        1
    );
    let mut single_asset = collection_asset();
    let single_face =
        strict_ooxml_render_svg::font::resolve_face("Times New Roman", false, false).unwrap();
    single_asset.bytes = Arc::from(single_face.bytes);
    single_asset.manifest.resource_hash = single_face.resource_hash;
    single_asset.manifest.face_index = 0;
    let single_options = RenderOptions::default().font_assets(Arc::new(
        FontAssetRegistry::from_assets(vec![single_asset], FontRegistryLimits::default()).unwrap(),
    ));
    let single = DocxBuilder::strict().body(&body("Collection Face")).build();
    let single_package = open(&single);
    let single_doc = parse_document(&single_package, &ParseOptions::default()).unwrap();
    let single_pages = place_pages(&single_doc, &single_options, None).unwrap();
    let widths = |pages: &[strict_ooxml_render_svg::layout::PlacedPage]| {
        pages[0]
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Text(text) = item {
                    Some(text.width)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(widths(&pages), widths(&single_pages));
    let reader = lopdf::Document::load_mem(&output.bytes).unwrap();
    assert!(reader.extract_text(&[1]).unwrap().contains("office"));
}

fn policy_asset(request: &str) -> strict_ooxml_render_svg::font::FontAsset {
    let face =
        strict_ooxml_render_svg::font::resolve_face("Times New Roman", false, false).unwrap();
    let mut asset = collection_asset();
    asset.bytes = std::sync::Arc::from(face.bytes);
    asset.manifest.family = request.into();
    asset.manifest.resolved_family = request.into();
    asset.manifest.face_index = 0;
    asset.manifest.resource_hash = face.resource_hash;
    asset.manifest.variations = face.instance_key().variations;
    asset.manifest.source = "bundled Tinos test fixture".into();
    asset
}

fn policy_pages(
    asset: strict_ooxml_render_svg::font::FontAsset,
    text: &str,
) -> Vec<strict_ooxml_render_svg::layout::PlacedPage> {
    use strict_ooxml_render_svg::font::{FontAssetRegistry, FontRegistryLimits};
    let body = format!(
        "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"{}\"/></w:rPr><w:t>{text}</w:t></w:r></w:p>",
        asset.manifest.family
    );
    let bytes = strict_ooxml_testkit::DocxBuilder::strict()
        .body(&body)
        .build();
    let package = open(&bytes);
    let document = parse_document(&package, &ParseOptions::default()).unwrap();
    let options = RenderOptions::default().font_assets(std::sync::Arc::new(
        FontAssetRegistry::from_assets(vec![asset], FontRegistryLimits::default()).unwrap(),
    ));
    place_pages(&document, &options, None).unwrap()
}

#[test]
fn pdf_policy_keeps_failure_counts_without_details_and_refuses_glyph_loss() {
    use strict_ooxml_render_svg::font::FontFailurePolicy;
    let pages = policy_pages(policy_asset("CJK Alias"), "&#x6f22;&#x5b57;");
    let options = RenderOptions {
        font_report_limit: 0,
        ..RenderOptions::default()
    };
    let continued = render(&pages, &options).unwrap();
    assert!(continued.report.losses().is_empty());
    assert!(!continued.report.is_clean());
    assert!(continued.report.font_report().failure_count >= 2);
    assert!(continued.report.font_report().details_truncated);
    assert!(continued.report.to_string().contains("font failures:"));
    let strict = options.font_failure_policy(FontFailurePolicy::Reject);
    assert!(render(&pages, &strict)
        .unwrap_err()
        .to_string()
        .contains("font.glyph.missing: CJK Alias"));
}

#[test]
fn pdf_embedding_failure_reports_request_and_strict_policy_refuses_output() {
    use strict_ooxml_render_svg::font::{hash_face_bytes, FontDiagnosticKind, FontFailurePolicy};
    let mut asset = policy_asset("Broken Outline Alias");
    let mut bytes = asset.bytes.to_vec();
    let table_count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    let table = (0..table_count)
        .map(|index| 12 + index * 16)
        .find(|&offset| &bytes[offset..offset + 4] == b"glyf")
        .unwrap();
    bytes[table..table + 4].copy_from_slice(b"BAD!");
    asset.manifest.resource_hash = hash_face_bytes(&bytes);
    asset.bytes = std::sync::Arc::from(bytes);
    let mut pages = policy_pages(asset, "AV office");
    // Exercise the scalar-CID subset path independently of malformed-font shaping rejection.
    for page in &mut pages {
        for item in &mut page.items {
            if let Item::Text(text) = item {
                text.advance = strict_ooxml_render_svg::TextAdvanceKind::Metric;
            }
        }
    }
    let output = render(&pages, &RenderOptions::default()).unwrap();
    let failure = output
        .report
        .font_report()
        .diagnostics()
        .iter()
        .find(|entry| entry.kind == FontDiagnosticKind::EmbeddingFailed)
        .unwrap();
    assert_eq!(failure.requested_family, "Broken Outline Alias");
    assert!(failure.resource.is_some());
    assert!(output.embedded_faces.is_empty());
    assert!(!output.report.is_clean());
    let strict = RenderOptions {
        font_report_limit: 0,
        ..RenderOptions::default().font_failure_policy(FontFailurePolicy::Reject)
    };
    let error = render(&pages, &strict).unwrap_err().to_string();
    assert!(
        error.contains("font.embed.failed: Broken Outline Alias"),
        "{error}"
    );
}

#[test]
fn licensed_programs_embed_with_cjk_unicode_and_explicit_indic_degradation() {
    use strict_ooxml_render_svg::font::{hash_face_bytes, FontDiagnosticKind, FontFailurePolicy};
    for (family, bytes, text, degraded) in [
        (
            "StrictLib Witness CJK SC",
            &include_bytes!("../../strict-ooxml-render-svg/fonts/witnesses/cjk.otf")[..],
            include_str!("../../strict-ooxml-render-svg/fonts/witnesses/cjk.txt"),
            false,
        ),
        (
            "StrictLib Witness Devanagari",
            &include_bytes!("../../strict-ooxml-render-svg/fonts/witnesses/devanagari.ttf")[..],
            include_str!("../../strict-ooxml-render-svg/fonts/witnesses/devanagari.txt"),
            true,
        ),
    ] {
        let mut asset = policy_asset(family);
        asset.bytes = std::sync::Arc::from(bytes);
        asset.manifest.resource_hash = hash_face_bytes(bytes);
        asset.manifest.variations.clear();
        asset.manifest.source = "fonts/witnesses/manifest.json pinned Noto derivative".into();
        asset.manifest.license_reference = "fonts/witnesses OFL and copyright".into();
        let pages = policy_pages(asset, text);
        let output = render(&pages, &RenderOptions::default()).unwrap();
        assert_eq!(output.embedded_faces.len(), 1, "{family}");
        assert!(output.embedded_faces[0].instance.is_some());
        assert!(!output
            .report
            .font_report()
            .diagnostics()
            .iter()
            .any(|entry| matches!(
                entry.kind,
                FontDiagnosticKind::MissingGlyph
                    | FontDiagnosticKind::ShapingFailed
                    | FontDiagnosticKind::EmbeddingFailed
            )));
        assert_eq!(output.report.font_report().is_clean(), !degraded);
        let reader = lopdf::Document::load_mem(&output.bytes).unwrap();
        let extracted: String = reader
            .extract_text(&[1])
            .unwrap()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        if degraded {
            // Known continuation-CID extraction gap: keep visible until Indic acceptance closes.
            assert!(extracted.contains('\u{fffd}'));
            assert_eq!(extracted.replace('\u{fffd}', ""), text);
            assert!(output
                .report
                .font_report()
                .diagnostics()
                .iter()
                .any(|entry| entry.kind == FontDiagnosticKind::DegradedScript));
        } else {
            assert_eq!(
                extracted, text,
                "{family}: ToUnicode must preserve logical clusters"
            );
        }
        let program_refs: Vec<_> = reader
            .objects
            .values()
            .filter_map(|obj| obj.as_dict().ok())
            .filter_map(|dict| {
                dict.get(b"FontFile2")
                    .or_else(|_| dict.get(b"FontFile3"))
                    .ok()
            })
            .map(|obj| obj.as_reference().unwrap())
            .collect();
        assert_eq!(program_refs.len(), 1);
        let stream = reader
            .get_object(program_refs[0])
            .unwrap()
            .as_stream()
            .unwrap();
        let program = stream.decompressed_content().unwrap();
        assert!(!program.is_empty());
        let strict = render(
            &pages,
            &RenderOptions::default().font_failure_policy(FontFailurePolicy::Reject),
        );
        if degraded {
            assert!(strict
                .unwrap_err()
                .to_string()
                .contains("font.script.degraded"));
        } else {
            assert!(strict.unwrap().report.is_clean());
        }
    }
}
