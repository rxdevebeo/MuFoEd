//! What a file is and how it becomes a `StrictDocument`.
//!
//! Every input ends as the same thing - an opened Strict package - so the
//! editor, the renderer and the writer never need to know where a document
//! came from. DOCX is read through the Transitional normalizer, RTF, HTML and
//! LaTeX through their importers, PDF and DjVu through the converter (with
//! recognition of scanned pages when it is asked for); a converted document is written to a Strict
//! package and opened again.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use strict_ooxml::{
    ConformancePolicy, MediaBag, Mode, OpenControl, OpenOptions, RtfOptions, StrictDocument,
    TransitionalNormalizer, WriteOptions,
};

/// Where a document came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A new, empty document.
    New,
    /// A Strict DOCX.
    Docx,
    /// A Transitional DOCX, normalized on the way in.
    DocxTransitional,
    /// RTF, imported.
    Rtf,
    /// HTML, imported.
    Html,
    /// LaTeX, imported.
    Latex,
    /// PDF, converted.
    Pdf,
    /// DjVu, converted.
    Djvu,
}

impl Kind {
    /// The short badge on a document tab.
    pub(crate) fn badge(self) -> &'static str {
        match self {
            Self::New | Self::Docx | Self::DocxTransitional => "W",
            Self::Rtf => "R",
            Self::Html => "H",
            Self::Latex => "L",
            Self::Pdf => "P",
            Self::Djvu => "D",
        }
    }

    /// Localization key of the kind's name.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::New => "format.new",
            Self::Docx => "format.docx",
            Self::DocxTransitional => "format.docx_transitional",
            Self::Rtf => "format.rtf",
            Self::Html => "format.html",
            Self::Latex => "format.latex",
            Self::Pdf => "format.pdf",
            Self::Djvu => "format.djvu",
        }
    }

    /// Whether the input had to be converted (and so whether import options apply).
    pub(crate) fn converted(self) -> bool {
        matches!(self, Self::Pdf | Self::Djvu)
    }
}

/// Recognition settings for scanned pages.
#[derive(Clone, Debug)]
pub(crate) struct OcrSettings {
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) language: Option<String>,
    /// Rebuild scanned pages block by block instead of as plain paragraphs.
    pub(crate) layout: bool,
}

/// How a PDF or DjVu is converted.
#[derive(Clone, Debug)]
pub(crate) struct ImportOptions {
    pub(crate) mode: Mode,
    pub(crate) images: bool,
    /// `1`-based inclusive page span.
    pub(crate) pages: Option<(usize, usize)>,
    pub(crate) ocr: Option<OcrSettings>,
    /// Every page set as the original laid it out (PDF): text in boxes where
    /// its blocks stood, one PDF page to a page.
    pub(crate) placed: bool,
}

impl ImportOptions {
    /// How an opened (not imported) PDF or DjVu is read: as the original
    /// looks - every page at its own size, its text in boxes where its blocks
    /// stood, its pictures and shapes where they stood. Re-flowing the text
    /// into paragraphs is an import's choice to make, not an open's; and the
    /// visual mode, a paragraph per line, stacked a designed page's side by
    /// side blocks under each other and set a one-page leaflet on five.
    pub(crate) fn as_original() -> Self {
        Self {
            placed: true,
            ..Self::default()
        }
    }
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Semantic,
            images: true,
            pages: None,
            ocr: None,
            placed: false,
        }
    }
}

/// What to open.
#[derive(Clone, Debug)]
pub(crate) enum Source {
    New,
    File {
        path: PathBuf,
        import: ImportOptions,
    },
}

/// An opened document and what it cost to get it.
pub(crate) struct Loaded {
    pub(crate) document: StrictDocument,
    pub(crate) kind: Kind,
    /// Import, normalization or conversion log, for the report pane.
    pub(crate) log: String,
}

/// Detects the kind of a file by its first bytes, then by its extension.
pub(crate) fn detect(path: &Path) -> Option<Kind> {
    let mut head = [0u8; 8];
    if let Ok(mut file) = std::fs::File::open(path) {
        use std::io::Read as _;
        let read = file.read(&mut head).unwrap_or(0);
        let head = head.get(..read).unwrap_or(&[]);
        if head.starts_with(b"PK\x03\x04") {
            return Some(Kind::Docx);
        }
        if head.starts_with(b"{\\rtf") {
            return Some(Kind::Rtf);
        }
        if head.starts_with(b"%PDF") {
            return Some(Kind::Pdf);
        }
        if head.starts_with(b"AT&TFORM") {
            return Some(Kind::Djvu);
        }
    }
    kind_by_extension(path)
}

/// The kind implied by the file name alone, without opening the file.
pub(crate) fn kind_by_extension(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "docx" | "docm" | "dotx" => Some(Kind::Docx),
        "rtf" => Some(Kind::Rtf),
        "html" | "htm" => Some(Kind::Html),
        "tex" | "ltx" => Some(Kind::Latex),
        "pdf" => Some(Kind::Pdf),
        "djvu" | "djv" => Some(Kind::Djvu),
        _ => None,
    }
}

/// Extensions the open dialog offers.
pub(crate) const OPEN_EXTENSIONS: &[&str] = &[
    "docx", "docm", "dotx", "rtf", "html", "htm", "tex", "ltx", "pdf", "djvu", "djv",
];

/// An empty document with the styles a new document needs.
///
/// RTF is the one format the toolkit builds a complete Strict package from
/// with nothing but text in hand, so a new document is a tiny RTF with a style
/// sheet, imported.
pub(crate) const NEW_DOCUMENT_RTF: &str = concat!(
    r"{\rtf1\ansi\ansicpg1252\deff0",
    r"{\fonttbl{\f0\fswiss Calibri;}{\f1\froman Cambria;}}",
    r"{\colortbl;\red0\green0\blue0;\red31\green56\blue100;}",
    r"{\stylesheet{\s0\f0\fs22\sa160\sl259\slmult1 Normal;}",
    r"{\s1\sbasedon0\snext0\b\f0\fs32\cf2\sb240\sa60\keepn heading 1;}",
    r"{\s2\sbasedon0\snext0\b\f0\fs26\cf2\sb200\sa60\keepn heading 2;}",
    r"{\s3\sbasedon0\snext0\b\f0\fs24\cf2\sb160\sa40\keepn heading 3;}",
    r"{\s4\sbasedon0\snext0\f1\fs56\sa120 Title;}",
    r"{\s5\sbasedon0\snext0\i\f0\fs28\sa160 Subtitle;}",
    r"{\s6\sbasedon0\snext0\i\f0\fs22\li720\ri720 Quote;}}",
    r"\paperw11906\paperh16838\margl1701\margr850\margt1134\margb1134",
    r"\pard\plain\s0\f0\fs22\sa160\sl259\slmult1\par}"
);

/// Opens `source`; `control` reports progress and stops a DOCX read.
pub(crate) fn open(source: &Source, control: &OpenControl) -> Result<Loaded, String> {
    match source {
        Source::New => {
            let opened =
                StrictDocument::import_rtf(NEW_DOCUMENT_RTF.as_bytes(), &RtfOptions::default())
                    .map_err(|error| error.to_string())?;
            Ok(Loaded {
                document: opened.document.with_file_name("new.docx"),
                kind: Kind::New,
                log: String::new(),
            })
        }
        Source::File { path, import } => {
            let kind = detect(path).ok_or_else(|| "unknown format".to_owned())?;
            match kind {
                Kind::Docx | Kind::DocxTransitional | Kind::New => open_docx(path, control),
                Kind::Rtf => open_rtf(path),
                Kind::Html => open_html(path),
                Kind::Latex => open_latex(path),
                Kind::Pdf => open_pdf(path, import),
                Kind::Djvu => open_djvu(path, import),
            }
        }
    }
}

fn open_docx(path: &Path, control: &OpenControl) -> Result<Loaded, String> {
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone())
        .control(control.clone());
    let document = StrictDocument::open_path(path, &options).map_err(|error| error.to_string())?;
    let normalized = document.package().was_normalized();
    let mut log = String::new();
    if let Some(report) = document.package().normalization_report() {
        let _ = writeln!(log, "{report}");
    }
    Ok(Loaded {
        document,
        kind: if normalized {
            Kind::DocxTransitional
        } else {
            Kind::Docx
        },
        log,
    })
}

fn open_rtf(path: &Path) -> Result<Loaded, String> {
    let opened = StrictDocument::import_rtf_path(path, &RtfOptions::default())
        .map_err(|error| error.to_string())?;
    let mut log = String::new();
    let _ = writeln!(log, "{:?}", opened.status());
    for issue in opened.pipeline_summary().issues {
        let _ = writeln!(log, "{} {}: {}", issue.stage, issue.id, issue.detail);
    }
    Ok(Loaded {
        document: opened.document,
        kind: Kind::Rtf,
        log,
    })
}

fn open_latex(path: &Path) -> Result<Loaded, String> {
    let opened = StrictDocument::import_latex_path(path, &strict_ooxml::LatexOptions::default())
        .map_err(|error| error.to_string())?;
    let mut log = String::new();
    let _ = writeln!(log, "{:?}", opened.status());
    for issue in opened.pipeline_summary().issues {
        let _ = writeln!(log, "{} {}: {}", issue.stage, issue.id, issue.detail);
    }
    Ok(Loaded {
        document: opened.document,
        kind: Kind::Latex,
        log,
    })
}

fn open_html(path: &Path) -> Result<Loaded, String> {
    use std::io::Read as _;
    let mut options = strict_ooxml::HtmlOptions::default();
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(options.max_input_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > options.max_input_bytes {
        return Err("HTML input exceeds size limit".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    options.base_url = Some(
        url::Url::from_file_path(&path)
            .map_err(|()| "invalid HTML file path")?
            .into(),
    );
    let html = html_text(&bytes)?;
    let resources = crate::clipboard::Resources::new();
    let opened = StrictDocument::import_html(html.as_bytes(), &options, Some(&resources))
        .map_err(|e| e.to_string())?;
    let mut log = format!(
        "HTML import: {} named losses\n",
        opened.import_report.loss_count
    );
    for issue in opened.import_report.issues {
        let _ = writeln!(log, "{}: {}", issue.feature, issue.detail);
    }
    let _ = writeln!(log, "{:?}", opened.write_report);
    Ok(Loaded {
        document: opened.document.with_file_name(path.display().to_string()),
        kind: Kind::Html,
        log,
    })
}

/// File encoding belongs to the application; the HTML crate accepts UTF-8.
fn html_text(bytes: &[u8]) -> Result<String, String> {
    let (encoding, skip) = encoding_rs::Encoding::for_bom(bytes).unwrap_or_else(|| {
        let head: String = bytes
            .iter()
            .take(4096)
            .map(|b| char::from(*b))
            .collect::<String>()
            .to_ascii_lowercase();
        let declared = head.split("<meta").skip(1).find_map(|tag| {
            let tag = tag.split('>').next()?;
            let value = tag
                .split_once("charset")?
                .1
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            let label = value
                .trim_start_matches(['\'', '"'])
                .split(['\'', '"', ';', '/', ' ', '\t', '\r', '\n'])
                .next()?;
            encoding_rs::Encoding::for_label(label.as_bytes())
        });
        (declared.unwrap_or(encoding_rs::UTF_8), 0)
    });
    let (text, errors) = encoding.decode_without_bom_handling(&bytes[skip..]);
    if errors {
        return Err(format!("Invalid {} HTML text", encoding.name()));
    }
    Ok(text.into_owned())
}

/// Installs recognition on a converter's options.
#[cfg(feature = "ocr")]
fn vision(settings: &OcrSettings) -> Arc<strict_ooxml::OllamaVision> {
    let mut config = strict_ooxml::OllamaConfig::from_env();
    config.endpoint.clone_from(&settings.endpoint);
    config.model.clone_from(&settings.model);
    Arc::new(strict_ooxml::OllamaVision::new(config))
}

fn open_pdf(path: &Path, import: &ImportOptions) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut reader = strict_ooxml::PdfDocument::open(&bytes, strict_ooxml::PdfLimits::default())
        .map_err(|error| error.to_string())?;
    let mut options = strict_ooxml::PdfOptions::default()
        .mode(import.mode)
        .embed_images(import.images)
        .place_every_page(import.placed);
    options.pages = import.pages;
    if let Some(settings) = &import.ocr {
        #[cfg(feature = "ocr")]
        {
            let vision = vision(settings);
            options.text_recovery = Some(vision.clone());
            options.figure_classifier = Some(vision);
            options.recovery_layout = settings.layout;
            options.recovery_language.clone_from(&settings.language);
        }
        #[cfg(not(feature = "ocr"))]
        {
            let _ = settings;
            return Err("recognition needs a build with the `ocr` feature".to_owned());
        }
    }
    let converted =
        strict_ooxml::convert_pdf(&mut reader, &options).map_err(|error| error.to_string())?;
    let log = converted.report.to_string();
    let mut bag = MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    finish_conversion(&converted.document, &bag, path, Kind::Pdf, log)
}

fn open_djvu(path: &Path, import: &ImportOptions) -> Result<Loaded, String> {
    let limits = strict_ooxml::DjvuLimits::default();
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let base = path
        .canonicalize()
        .ok()
        .and_then(|canonical| canonical.parent().map(Path::to_path_buf));
    let mut reader = strict_ooxml::DjvuDocument::open_with_resolver(&bytes, limits, |id| {
        // An indirect document names its pages as files beside it; nothing
        // outside that directory is read.
        let base = base
            .as_ref()
            .ok_or_else(|| strict_ooxml::DjvuError::Component(id.to_owned()))?;
        let component = base
            .join(id)
            .canonicalize()
            .map_err(|error| strict_ooxml::DjvuError::Component(format!("{id}: {error}")))?;
        if component.parent() != Some(base.as_path()) {
            return Err(strict_ooxml::DjvuError::Component(
                "component escapes the input directory".into(),
            ));
        }
        std::fs::read(&component)
            .map_err(|error| strict_ooxml::DjvuError::Component(format!("{id}: {error}")))
    })
    .map_err(|error| error.to_string())?;
    let mut options = strict_ooxml::DjvuOptions::default()
        .mode(import.mode)
        .embed_images(import.images);
    options.pages = import.pages;
    if let Some(settings) = &import.ocr {
        #[cfg(feature = "ocr")]
        {
            let vision = vision(settings);
            options.text_recovery = Some(vision.clone());
            options.figure_classifier = Some(vision);
            options.recovery_language.clone_from(&settings.language);
        }
        #[cfg(not(feature = "ocr"))]
        {
            let _ = settings;
            return Err("recognition needs a build with the `ocr` feature".to_owned());
        }
    }
    let converted =
        strict_ooxml::convert_djvu(&mut reader, &options).map_err(|error| error.to_string())?;
    let log = converted.report.to_string();
    let mut bag = MediaBag::new();
    for (part, bytes) in &converted.media {
        bag.insert(part.clone(), bytes.clone());
    }
    finish_conversion(&converted.document, &bag, path, Kind::Djvu, log)
}

/// The file a recognition was started from, so its pages can be shown beside
/// the recognized text. `first` is the 1-based source page of document page 0.
#[derive(Clone, Debug)]
pub(crate) struct OriginalSource {
    pub(crate) path: PathBuf,
    pub(crate) kind: Kind,
    pub(crate) first: usize,
}

/// The PNG of one source page. `document_page` is zero-based in the opened
/// document.
#[cfg(feature = "ocr")]
pub(crate) fn original_png(
    source: &OriginalSource,
    document_page: usize,
) -> Result<Vec<u8>, String> {
    let number = source.first.saturating_add(document_page);
    let bytes = std::fs::read(&source.path).map_err(|error| error.to_string())?;
    match source.kind {
        Kind::Pdf => {
            let pdf = strict_ooxml::PdfDocument::open(&bytes, strict_ooxml::PdfLimits::default())
                .map_err(|error| error.to_string())?;
            let raster = pdf.rasterizer().map_err(|error| error.to_string())?;
            raster
                .page_png(
                    number.max(1),
                    &strict_ooxml_pdf::raster::RasterOptions {
                        scale: 1.25,
                        ..strict_ooxml_pdf::raster::RasterOptions::default()
                    },
                )
                .map_err(|error| error.to_string())
        }
        Kind::Djvu => {
            let document =
                strict_ooxml::DjvuDocument::open(&bytes, strict_ooxml::DjvuLimits::default())
                    .map_err(|error| error.to_string())?;
            let index = number.saturating_sub(1);
            document
                .pages()
                .get(index)
                .ok_or_else(|| format!("page {number} is not in the file"))?
                .image_png(strict_ooxml::DjvuLimits::default())
                .map_err(|error| error.to_string())
        }
        Kind::New | Kind::Docx | Kind::DocxTransitional | Kind::Rtf | Kind::Html | Kind::Latex => {
            Err("this file has no page images".to_owned())
        }
    }
}

/// Writes a converted model to a Strict package and opens that package.
fn finish_conversion(
    document: &strict_ooxml::Document,
    media: &MediaBag,
    path: &Path,
    kind: Kind,
    mut log: String,
) -> Result<Loaded, String> {
    let written = strict_ooxml::write_package(document, Some(media), &WriteOptions::default())
        .map_err(|error| error.to_string())?;
    for loss in written.report.losses() {
        let _ = writeln!(log, "write: {loss}");
    }
    let document = StrictDocument::open_reader(&written.bytes[..], &OpenOptions::default())
        .map_err(|error| error.to_string())?
        .with_file_name(path.display().to_string());
    Ok(Loaded {
        document,
        kind,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_document_opens_with_heading_styles() {
        let loaded = open(&Source::New, &OpenControl::new()).expect("new document");
        assert_eq!(loaded.kind, Kind::New);
        let document = loaded.document.document();
        assert_eq!(document.body.blocks.len(), 1);
        let names: Vec<String> = document
            .styles
            .iter()
            .filter_map(|style| style.name.as_deref().map(str::to_owned))
            .collect();
        assert!(
            names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("heading 1")),
            "{names:?}"
        );
    }

    #[test]
    fn kinds_are_told_apart_by_their_first_bytes() {
        let dir = std::env::temp_dir().join(format!("mufoed-detect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cases: [(&str, &[u8], Kind); 4] = [
            ("a.bin", b"%PDF-1.7", Kind::Pdf),
            ("b.bin", b"{\\rtf1 x}", Kind::Rtf),
            ("c.bin", b"PK\x03\x04rest", Kind::Docx),
            ("d.bin", b"AT&TFORM....", Kind::Djvu),
        ];
        for (name, bytes, kind) in cases {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(detect(&path), Some(kind), "{name}");
        }
        assert_eq!(detect(&dir.join("missing.rtf")), Some(Kind::Rtf));
        assert_eq!(detect(&dir.join("missing.HTML")), Some(Kind::Html));
        assert_eq!(detect(&dir.join("missing.htm")), Some(Kind::Html));
        assert_eq!(detect(&dir.join("missing.tex")), Some(Kind::Latex));
        assert_eq!(detect(&dir.join("missing.LTX")), Some(Kind::Latex));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn html_file_encodings_are_decoded_before_import() {
        assert_eq!(html_text(b"\xef\xbb\xbf<p>ok</p>").unwrap(), "<p>ok</p>");
        let text = "<p>Привет</p>";
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(html_text(&utf16).unwrap(), text);
        let source = "<meta charset='windows-1251'><p>Привет</p>";
        let (encoded, _, errors) = encoding_rs::WINDOWS_1251.encode(source);
        assert!(!errors);
        assert_eq!(html_text(&encoded).unwrap(), source);
        assert!(html_text(&[0xff]).is_err());
    }

    #[test]
    fn a_latex_file_opens_as_an_imported_document() {
        let dir = std::env::temp_dir().join(format!("mufoed-latex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.tex");
        std::fs::write(
            &path,
            "\\documentclass{article}\n\\begin{document}\nHello\n\\end{document}\n",
        )
        .unwrap();
        let loaded = open(
            &Source::File {
                path,
                import: ImportOptions::default(),
            },
            &OpenControl::new(),
        )
        .expect("latex");
        assert_eq!(loaded.kind, Kind::Latex);
        let text: String = loaded
            .document
            .document()
            .body
            .blocks
            .iter()
            .filter_map(|block| {
                let strict_ooxml::model::Block::Paragraph(paragraph) = block else {
                    return None;
                };
                Some(
                    paragraph
                        .inlines
                        .iter()
                        .filter_map(|inline| {
                            let strict_ooxml::model::Inline::Run(run) = inline else {
                                return None;
                            };
                            Some(
                                run.content
                                    .iter()
                                    .filter_map(|content| match content {
                                        strict_ooxml::model::RunContent::Text(text) => {
                                            Some(text.text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect::<String>(),
                            )
                        })
                        .collect::<String>(),
                )
            })
            .collect();
        assert!(text.contains("Hello"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
