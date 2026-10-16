//! What a file is and how it becomes a `StrictDocument`.
//!
//! Every input ends as the same thing - an opened Strict package - so the
//! editor, the renderer and the writer never need to know where a document
//! came from. DOCX is read through the Transitional normalizer, RTF through the
//! importer, PDF and DjVu through the converter (with recognition of scanned
//! pages when it is asked for); a converted document is written to a Strict
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
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Semantic,
            images: true,
            pages: None,
            ocr: None,
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
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "docx" | "docm" | "dotx" => Some(Kind::Docx),
        "rtf" => Some(Kind::Rtf),
        "pdf" => Some(Kind::Pdf),
        "djvu" | "djv" => Some(Kind::Djvu),
        _ => None,
    }
}

/// Extensions the open dialog offers.
pub(crate) const OPEN_EXTENSIONS: &[&str] = &["docx", "docm", "dotx", "rtf", "pdf", "djvu", "djv"];

/// An empty document with the styles a new document needs.
///
/// RTF is the one format the toolkit builds a complete Strict package from
/// with nothing but text in hand, so a new document is a tiny RTF with a style
/// sheet, imported.
const NEW_DOCUMENT_RTF: &str = concat!(
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
        .embed_images(import.images);
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
        let _ = std::fs::remove_dir_all(dir);
    }
}
