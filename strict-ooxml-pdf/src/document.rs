//! Page and document assembly.
//!
//! `lopdf` gives the object graph; this module decides what a page *is*: its
//! size, its rotation, the fonts and images its resources declare, and the
//! content its stream describes. Inherited attributes matter here — `/Resources`,
//! `/MediaBox` and `/Rotate` walk down the page tree until a page sets them —
//! and getting that wrong yields a page of the wrong size rather than an error.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use lopdf::Object;

use crate::content::{Content, Item, Matrix, PageGeometry, Resources as ResourceProvider};
use crate::error::{PdfError, PdfLimits, Result};
use crate::fonts::{resolver, PdfFont};
use crate::image::{Encoded, ImageCache, Reject};

/// Decompresses a stream, refusing output past `limit` bytes (AUD-13).
///
/// Every call site that used to ask `lopdf` for unbounded
/// `decompressed_content` goes through this function, so a flate bomb in a
/// form, a `ToUnicode` CMap or an image is rejected before the allocation.
/// Page content uses `get_page_content_with_limit` instead, which is already
/// bounded the same way.
///
/// The underlying decoder is lopdf's bounded path (its Flate stage is
/// `miniz_oxide` with a limit); naming the bound here is what keeps every
/// call site honest under `rg "decompressed_content"`.
pub(crate) fn bounded_decompress(
    stream: &lopdf::Stream,
    limit: usize,
) -> std::result::Result<Vec<u8>, Reject> {
    match stream.decompressed_content_with_limit(limit) {
        Ok(bytes) => Ok(bytes),
        Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })) => {
            Err(Reject::TooLarge)
        }
        Err(_) => Err(Reject::Broken("stream could not be inflated")),
    }
}

/// A form's content, decoded once for the document (AUD-13).
struct DecodedForm {
    operations: Rc<Vec<lopdf::content::Operation>>,
    /// Inline images pulled out before tokenisation (AUD-84).
    inlines: Rc<Vec<crate::inline::InlineImage>>,
    matrix: Matrix,
    /// The form's own `/Resources` object, when it declares one.
    resources: Option<Object>,
}

/// Forms already decoded, by object id.
///
/// Bounded by [`PdfLimits::max_cached_form_bytes`]: a form decoded once the
/// cache is full is returned to the caller but not kept.
struct FormCache {
    entries: RefCell<HashMap<lopdf::ObjectId, Rc<DecodedForm>>>,
    /// Decompressed content bytes of the forms in `entries`.
    held_bytes: std::cell::Cell<usize>,
    ceiling: usize,
}

impl FormCache {
    fn new(ceiling: usize) -> Self {
        Self {
            entries: RefCell::new(HashMap::new()),
            held_bytes: std::cell::Cell::new(0),
            ceiling,
        }
    }

    fn get_or_decode(
        &self,
        id: lopdf::ObjectId,
        document: &lopdf::Document,
        limits: &PdfLimits,
    ) -> Option<Rc<DecodedForm>> {
        if let Some(hit) = self.entries.borrow().get(&id).cloned() {
            return Some(hit);
        }
        let stream = document.get_object(id).ok()?.as_stream().ok()?;
        let bytes = bounded_decompress(stream, limits.max_content_bytes).ok()?;
        let (cleaned, inlines) = crate::inline::extract(&bytes);
        let operations = lopdf::content::Content::decode(&cleaned).ok()?.operations;
        let matrix = stream
            .dict
            .get(b"Matrix")
            .ok()
            .and_then(|value| {
                crate::fonts::array_items(value, resolver(document))
                    .iter()
                    .map(number_of_object)
                    .collect::<Option<Vec<f64>>>()
            })
            .and_then(|values| match *values.as_slice() {
                // ISO 32000-1 8.3.3: [sx ky kx sy tx ty].
                [sx, ky, kx, sy, tx, ty] => Some(Matrix {
                    a: sx,
                    b: ky,
                    c: kx,
                    d: sy,
                    e: tx,
                    f: ty,
                }),
                _ => None,
            })
            .unwrap_or(Matrix::IDENTITY);
        let resources = stream.dict.get(b"Resources").ok().cloned();
        let decoded = Rc::new(DecodedForm {
            operations: Rc::new(operations),
            inlines: Rc::new(inlines),
            matrix,
            resources,
        });
        let held = self.held_bytes.get().saturating_add(bytes.len());
        if held <= self.ceiling {
            self.held_bytes.set(held);
            self.entries.borrow_mut().insert(id, Rc::clone(&decoded));
        }
        Some(decoded)
    }
}

/// Fonts already decoded, by object id, bounded by [`PdfLimits::max_fonts`].
struct FontCache {
    entries: RefCell<HashMap<lopdf::ObjectId, Rc<PdfFont>>>,
    /// Notes produced while filling the cache (`pdf.font.budget`, …).
    notes: RefCell<Vec<(String, String)>>,
}

impl FontCache {
    fn new() -> Self {
        Self {
            entries: RefCell::new(HashMap::new()),
            notes: RefCell::new(Vec::new()),
        }
    }

    /// The font under `name`, decoded at most once per object id.
    ///
    /// Past [`PdfLimits::max_fonts`] the font is not decoded: a stub with no
    /// Unicode map is returned so glyphs still advance, and `pdf.font.budget`
    /// is recorded once (AUD-13).
    fn get_or_build(
        &self,
        id: lopdf::ObjectId,
        name: &str,
        dictionary: &lopdf::Dictionary,
        document: &lopdf::Document,
        limits: &PdfLimits,
    ) -> PdfFont {
        if let Some(hit) = self.entries.borrow().get(&id).cloned() {
            let mut font = (*hit).clone();
            name.clone_into(&mut font.name);
            return font;
        }
        if self.entries.borrow().len() >= limits.max_fonts {
            self.notes.borrow_mut().push((
                "pdf.font.budget".to_owned(),
                format!(
                    "font `{name}` was not decoded: the document already holds {} fonts",
                    limits.max_fonts
                ),
            ));
            return PdfFont::stub(name);
        }
        match PdfFont::build(name, dictionary, document, limits) {
            Ok(font) => {
                self.entries.borrow_mut().insert(id, Rc::new(font.clone()));
                font
            }
            Err(_) => {
                // A font the reader cannot build is not a reason to lose the
                // page: its text is recorded as unmapped instead.
                PdfFont::stub(name)
            }
        }
    }

    fn take_notes(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.notes.borrow_mut())
    }
}

/// An opened PDF.
pub struct PdfDocument {
    document: lopdf::Document,
    /// The bytes the document was opened from.
    ///
    /// Kept because two things need the file itself rather than what this reader
    /// made of it: the `raster` feature hands the whole file to a rasterizer, and
    /// a caller converting a document may want the original bytes for a report or
    /// a hash. One copy of a file that has already been read into memory.
    source: Vec<u8>,
    limits: PdfLimits,
    /// Pictures already decoded, shared by every page (Q-26).
    ///
    /// A document that draws the same picture on many pages decodes it once
    /// here instead of once per draw; the budget is
    /// [`PdfLimits::max_cached_image_bytes`].
    images: ImageCache,
    /// Forms already decoded, shared by every page (AUD-13).
    forms: FormCache,
    /// Fonts already decoded, shared by every page (AUD-13).
    fonts: FontCache,
    report: crate::report::ReadReport,
    /// Decompressed page-content bytes read so far, charged against
    /// [`PdfLimits::max_total_content_bytes`]. A `Cell`, because reading a page
    /// takes `&self` and the budget is the document's, not the page's.
    content_spent: std::cell::Cell<u64>,
}

impl PdfDocument {
    /// Opens a PDF from a path.
    ///
    /// # Errors
    ///
    /// As [`PdfDocument::open`], with the file's own I/O failure reported the
    /// same way.
    pub fn open_path(path: impl AsRef<std::path::Path>, limits: PdfLimits) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|error| PdfError::Malformed(error.to_string()))?;
        Self::open(&bytes, limits)
    }

    /// Opens a PDF from memory.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Malformed`] for a file that is not a PDF,
    /// [`PdfError::Encrypted`] for one that is, and
    /// [`crate::error::PdfError::LimitExceeded`] when
    /// the input size, the inflated size of its object streams
    /// ([`LimitKind::ObjectStreamBytes`](crate::error::LimitKind::ObjectStreamBytes))
    /// or the page count is over budget.
    ///
    /// Active content — an opening action, document JavaScript, embedded
    /// files, annotations that launch or submit — is not run and not refused:
    /// it is recorded in [`PdfDocument::report`] under `pdf.active.*`, so a
    /// caller that hands the file on knows what it carries.
    pub fn open(bytes: &[u8], limits: PdfLimits) -> Result<Self> {
        if bytes.len() > limits.max_input_bytes {
            return Err(limits.exceeded(crate::error::LimitKind::InputBytes, bytes.len() as u64));
        }
        // `lopdf` inflates every object and cross-reference stream while it
        // loads, so their budget is checked on the raw bytes first, and handed to
        // the loader as its per-stream ceiling for whatever the scan could not
        // read (waiver `PDF-OBJSTM-BOMB`, closed).
        crate::preload::check_object_streams(bytes, &limits)?;
        let options = lopdf::LoadOptions {
            max_decompressed_size: Some(crate::preload::object_stream_budget(&limits)),
            ..lopdf::LoadOptions::default()
        };
        let document = lopdf::Document::load_mem_with_options(bytes, options)
            .map_err(|error| PdfError::from_lopdf(&error))?;
        if document.is_encrypted() {
            return Err(PdfError::Encrypted);
        }
        let pages = document.get_pages().len();
        if pages > limits.max_pages {
            return Err(limits.exceeded(crate::error::LimitKind::Pages, pages as u64));
        }
        let mut report = crate::report::ReadReport::new();
        for (id, detail) in active_content(&document) {
            report.record_ignored(id, detail);
        }
        Ok(Self {
            document,
            source: bytes.to_vec(),
            limits,
            images: ImageCache::new(limits.max_cached_image_bytes),
            forms: FormCache::new(limits.max_cached_form_bytes),
            fonts: FontCache::new(),
            report,
            content_spent: std::cell::Cell::new(0),
        })
    }

    /// Decompressed page-content bytes read so far, every page read counted.
    ///
    /// What [`PdfLimits::max_total_content_bytes`] is checked against: a page
    /// read twice is inflated twice, and is charged twice.
    #[must_use]
    pub fn content_bytes_read(&self) -> u64 {
        self.content_spent.get()
    }

    /// The bytes the document was opened from.
    #[must_use]
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// A rasterizer for this document, under the `raster` feature.
    ///
    /// The convenience a caller converting a document wants: the bytes are
    /// already here, and a rasterizer parses the file once for as many pages as
    /// the caller asks for.
    ///
    /// # Errors
    ///
    /// As [`crate::raster::Rasterizer::new`].
    #[cfg(feature = "raster")]
    pub fn rasterizer(&self) -> Result<crate::raster::Rasterizer> {
        crate::raster::Rasterizer::new(&self.source, self.limits)
    }

    /// How many pages the document has.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.document.get_pages().len()
    }

    /// The resource budget in force.
    #[must_use]
    pub fn limits(&self) -> PdfLimits {
        self.limits
    }

    /// Bytes of decoded pictures this document is holding on to (Q-26).
    ///
    /// What a caller tuning [`PdfLimits::max_cached_image_bytes`] needs: a number
    /// equal to the ceiling means the cache is full and further pictures are
    /// being decoded per draw again, which is the state the reader was in before
    /// there was a cache.
    #[must_use]
    pub fn cached_image_bytes(&self) -> usize {
        self.images.bytes()
    }

    /// Decompressed content bytes of the form XObjects this document holds, at
    /// most [`PdfLimits::max_cached_form_bytes`].
    #[must_use]
    pub fn cached_form_bytes(&self) -> usize {
        self.forms.held_bytes.get()
    }

    /// How many distinct pictures this document has decoded, refusals included.
    ///
    /// The number to compare against the number of picture draws: equal means
    /// every draw was served from the cache.
    #[must_use]
    pub fn distinct_images(&self) -> usize {
        self.images.distinct_images()
    }

    /// What could not be carried out, across every page read so far.
    #[must_use]
    pub fn report(&self) -> &crate::report::ReadReport {
        &self.report
    }

    /// Reads one page, 1-based.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Missing`] when the page has no usable geometry, and a
    /// limit error when a budget is hit.
    pub fn page(&mut self, number: usize) -> Result<PdfPage> {
        let id = *self
            .document
            .get_pages()
            .get(&u32::try_from(number).unwrap_or(0))
            .ok_or_else(|| PdfError::Missing(format!("page {number}")))?;
        let page = self.read_page(id, number)?;
        self.report.merge(&page.report);
        Ok(page)
    }

    /// Reads every page, bounded by `PdfLimits::max_pages`.
    ///
    /// # Errors
    ///
    /// As [`PdfDocument::page`].
    pub fn pages(&mut self) -> Result<Vec<PdfPage>> {
        let ids: Vec<lopdf::ObjectId> = self.document.page_iter().collect();
        let mut out = Vec::with_capacity(ids.len());
        for (index, id) in ids.into_iter().enumerate() {
            out.push(self.read_page(id, index.saturating_add(1))?);
        }
        for page in &out {
            self.report.merge(&page.report);
        }
        Ok(out)
    }

    /// Reads one page and stamps it with its own 1-based number.
    ///
    /// The number is a parameter rather than something `read_page` works out,
    /// because lopdf hands out object ids and this crate hands out *page*
    /// numbers, and every message that names a page — a loss record, a recovery,
    /// a page range — has to name the page the caller asked for. A page that
    /// reports itself as page 0 is a page nobody can find in a document.
    fn read_page(&self, id: lopdf::ObjectId, number: usize) -> Result<PdfPage> {
        let dictionary = self
            .document
            .get_dictionary(id)
            .map_err(|error| PdfError::from_lopdf(&error))?;
        let inherited = self.inherited(id);
        let geometry = geometry_of(dictionary, &inherited)?;
        let resources = PageResources::new(
            &self.document,
            &self.images,
            &self.forms,
            &self.fonts,
            &inherited,
            self.limits,
        );

        let content_bytes = self
            .document
            .get_page_content_with_limit(id, self.limits.max_content_bytes)
            .map_err(|error| PdfError::from_lopdf(&error))?;
        // The page budget bounds one page; this bounds the document. The bytes
        // are charged even when they put the total over, so every later read
        // fails too instead of a smaller page slipping under the ceiling.
        let spent = self
            .content_spent
            .get()
            .saturating_add(u64::try_from(content_bytes.len()).unwrap_or(u64::MAX));
        self.content_spent.set(spent);
        if spent > self.limits.max_total_content_bytes {
            return Err(self
                .limits
                .exceeded(crate::error::LimitKind::TotalContentBytes, spent));
        }
        let (cleaned, inlines) = crate::inline::extract(&content_bytes);
        let operations = lopdf::content::Content::decode(&cleaned)
            .map_err(|error| PdfError::from_lopdf(&error))?
            .operations;
        let content = crate::content::interpret_with_inlines(
            &operations,
            &inlines,
            &resources,
            geometry,
            self.limits,
            crate::FLATTEN_TOLERANCE_PT,
        )?;
        let mut report = crate::report::ReadReport::new();
        for ignored in &content.ignored {
            report.record_ignored(ignored.id, &ignored.detail);
        }
        for (id, detail) in resources.notes() {
            report.record_ignored(id, detail);
        }
        for (id, detail) in self.fonts.take_notes() {
            report.record_ignored(&id, &detail);
        }
        if self.images.saw_mask_cycle() {
            // A picture whose `/SMask` points at itself, or a cycle of them. The
            // picture still draws; it draws without the transparency that cycle
            // claimed (AUD-12).
            report.record_ignored(
                "pdf.image.smask-cycle",
                "a soft mask names itself or another mask that names it; the mask was not read",
            );
        }
        report.set_unmapped(content.unmapped_glyphs);
        report.set_estimated_widths(content.estimated_widths);
        for item in &content.items {
            if let Item::Image(placed) = item {
                if let Some(missing) = &placed.missing {
                    report.record_image_missing(missing);
                }
            }
        }
        Ok(PdfPage {
            number,
            geometry,
            content,
            report,
        })
    }

    /// The attributes a page inherits from its ancestors, nearest first.
    ///
    /// `/Resources`, `/MediaBox`, `/CropBox` and `/Rotate` walk down the page
    /// tree until a page sets them, so a document that puts its `MediaBox` on the
    /// page tree and only its `CropBox` on the page needs both halves. The walk
    /// follows `/Parent` *without* dereferencing it first: resolving it lands on
    /// the parent's dictionary, and a match against `Object::Reference` there
    /// never succeeds, so the walk stops after one page and the inherited
    /// attributes are invisible.
    fn inherited(&self, id: lopdf::ObjectId) -> BTreeMap<Vec<u8>, Object> {
        let mut out: BTreeMap<Vec<u8>, Object> = BTreeMap::new();
        let mut current = id;
        let mut guard = 0u32;
        while let Ok(dictionary) = self.document.get_dictionary(current) {
            for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
                if let Ok(value) = dictionary.get(key) {
                    out.entry(key.to_vec()).or_insert_with(|| value.clone());
                }
            }
            match dictionary.get(b"Parent") {
                Ok(value) => match value.as_reference() {
                    Ok(parent) => current = parent,
                    Err(_) => break,
                },
                Err(_) => break,
            }
            guard = guard.saturating_add(1);
            if guard > 64 {
                // A parent cycle in a damaged file: stop rather than loop.
                break;
            }
        }
        out
    }
}

/// How many annotations [`active_content`] looks at, across every page.
///
/// The scan is a report, not a defence, and a file of a million annotations is
/// not allowed to make opening it a million dictionary lookups. Past the cap
/// the report says the scan stopped.
const MAX_ANNOTATIONS_SCANNED: usize = 10_000;

/// Action types (ISO 32000-1 §12.6.4) that reach outside the page: run code,
/// start a program, open another file, or send or fetch data.
const RISKY_ACTIONS: [&str; 5] = ["JavaScript", "Launch", "GoToR", "ImportData", "SubmitForm"];

/// What the document would *do* if a viewer let it, as `(report id, detail)`.
///
/// Information only: this reader runs no action and opens no attachment, so
/// nothing here changes what a page reads as. But a converter that hands the
/// original file on, or a person deciding whether to open it, should learn that
/// it carries an opening action, document-level JavaScript, embedded files or
/// annotations that launch, submit or fetch — and the report is where every
/// other fact about the file goes.
///
/// Bounded: references are followed through `lopdf`'s own dereference limit,
/// and at most [`MAX_ANNOTATIONS_SCANNED`] annotations are inspected.
fn active_content(document: &lopdf::Document) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    if let Ok(catalog) = document.catalog() {
        // An `/OpenAction` that is a destination, or a `/GoTo`, only scrolls;
        // the ones worth a line are the ones that leave the page.
        if let Some(kind) = catalog
            .get(b"OpenAction")
            .ok()
            .and_then(|value| dict_of(document, value))
            .and_then(risky_action)
        {
            out.push(("pdf.active.open-action", open_action_detail(kind)));
        }
        if catalog.get(b"AA").is_ok() {
            out.push((
                "pdf.active.additional-actions",
                "the document catalog carries additional actions (/AA); they were not run",
            ));
        }
        if let Some(names) = catalog
            .get(b"Names")
            .ok()
            .and_then(|value| dict_of(document, value))
        {
            if names.get(b"JavaScript").is_ok() {
                out.push((
                    "pdf.active.javascript",
                    "the document carries document-level JavaScript; it was not run",
                ));
            }
            if names.get(b"EmbeddedFiles").is_ok() {
                out.push((
                    "pdf.active.embedded-files",
                    "the document carries embedded files; they were not read",
                ));
            }
        }
    }

    let mut scanned = 0usize;
    'pages: for page in document.page_iter() {
        let Some(annotations) = document
            .get_dictionary(page)
            .ok()
            .and_then(|dictionary| dictionary.get(b"Annots").ok())
            .and_then(|value| deref(document, value))
            .and_then(|value| value.as_array().ok())
        else {
            continue;
        };
        for annotation in annotations {
            if scanned >= MAX_ANNOTATIONS_SCANNED {
                out.push((
                    "pdf.active.scan-capped",
                    "annotations past the first 10 000 were not checked for actions",
                ));
                break 'pages;
            }
            scanned = scanned.saturating_add(1);
            let Some(kind) = dict_of(document, annotation)
                .and_then(|dictionary| dictionary.get(b"A").ok())
                .and_then(|value| dict_of(document, value))
                .and_then(risky_action)
            else {
                continue;
            };
            out.push(("pdf.active.annotation", annotation_detail(kind)));
        }
    }
    out
}

/// `object`, with references followed through `lopdf`'s bounded dereference.
fn deref<'a>(document: &'a lopdf::Document, object: &'a Object) -> Option<&'a Object> {
    document.dereference(object).ok().map(|(_, value)| value)
}

/// `object` as a dictionary, references followed.
fn dict_of<'a>(
    document: &'a lopdf::Document,
    object: &'a Object,
) -> Option<&'a lopdf::Dictionary> {
    deref(document, object).and_then(|value| value.as_dict().ok())
}

/// The action's `/S`, when it is one of [`RISKY_ACTIONS`].
fn risky_action(action: &lopdf::Dictionary) -> Option<&'static str> {
    let kind = action.get(b"S").ok()?.as_name().ok()?;
    RISKY_ACTIONS
        .iter()
        .find(|risky| risky.as_bytes() == kind)
        .copied()
}

/// The report line for a risky `/OpenAction`, by its type.
fn open_action_detail(kind: &str) -> &'static str {
    match kind {
        "JavaScript" => "the document runs JavaScript when opened (/OpenAction); it was not run",
        "Launch" => "the document launches a program when opened (/OpenAction); it was not run",
        "GoToR" => "the document opens another file when opened (/OpenAction); it was not run",
        "ImportData" => "the document imports data when opened (/OpenAction); it was not run",
        _ => "the document submits a form when opened (/OpenAction); it was not run",
    }
}

/// The report line for an annotation's risky `/A`, by its type.
fn annotation_detail(kind: &str) -> &'static str {
    match kind {
        "JavaScript" => "an annotation runs JavaScript (/S /JavaScript); it was not run",
        "Launch" => "an annotation launches a program (/S /Launch); it was not run",
        "GoToR" => "an annotation opens another file (/S /GoToR); it was not run",
        "ImportData" => "an annotation imports data (/S /ImportData); it was not run",
        _ => "an annotation submits a form (/S /SubmitForm); it was not run",
    }
}

/// One read page.
#[derive(Clone, Debug)]
pub struct PdfPage {
    /// The 1-based page number, filled in by the document.
    pub number: usize,
    /// Size and rotation.
    pub geometry: PageGeometry,
    /// The placed items.
    pub content: Content,
    /// What this page lost.
    pub report: crate::report::ReadReport,
}

/// What a page offers a converter, and what it costs to get more.
///
/// Three states, because the two failures are not the same failure: a page with
/// no glyphs at all is a **scan** and a picture of it is worth reading with a
/// model, while a page full of glyphs this reader could not map is a **font**
/// problem — a picture of it shows the text perfectly well, so the model helps
/// there too, but nothing about the PDF is wrong and nothing about the font
/// should be reported as if it were.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLayer {
    /// The page has at least one glyph this reader could map to a character.
    Readable,
    /// The page draws no text at all: a scan, or a picture.
    Absent,
    /// The page draws text and none of it could be mapped — an embedded font
    /// with no usable `ToUnicode`, a broken `CMap`.
    Unreadable,
}

impl PdfPage {
    /// What this page's text layer amounts to (`STAGE-8-TASK.md` §5, C7).
    ///
    /// "Readable" means a character came out, not that the page reads well: a
    /// page of mojibake that mapped to *something* is `Readable`, and the report
    /// is where a caller learns to distrust it. Glyphs the producer drew
    /// invisibly (a text layer under a scanned image, which is how a producer
    /// makes a scan searchable) do **not** count — they are real text to a
    /// search index and absent to a converter, and pretending otherwise would
    /// make every OCR-enabled scan come back "already has text".
    #[must_use]
    pub fn text_layer(&self) -> TextLayer {
        let mut glyphs = 0usize;
        let mut mapped = 0usize;
        for item in &self.content.items {
            let Item::Glyph(glyph) = item else {
                continue;
            };
            if !glyph.render_mode.paints() {
                continue;
            }
            glyphs = glyphs.saturating_add(1);
            if glyph.mapped {
                mapped = mapped.saturating_add(1);
            }
        }
        match (glyphs, mapped) {
            (0, _) => TextLayer::Absent,
            (_, 0) => TextLayer::Unreadable,
            _ => TextLayer::Readable,
        }
    }

    /// Whether the page draws anything at all.
    ///
    /// A blank page has no text layer either, and sending it to a vision model
    /// would spend a call to be told there is nothing there. The difference
    /// matters because "we could not read this" and "there is nothing here" are
    /// different facts about a document.
    #[must_use]
    pub fn has_ink(&self) -> bool {
        self.content.items.iter().any(|item| match item {
            // A glyph and a picture are both ink even when the reader could not
            // decode the picture: an undecodable scan is exactly the case where a
            // model is the only way left to see the page.
            Item::Glyph(_) | Item::Image(_) => true,
            Item::Vector(vector) => !vector.subpaths.is_empty(),
        })
    }

    /// The text of the page, in drawing order, with items joined by spaces.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut last_baseline = f64::NAN;
        for item in &self.content.items {
            let Item::Glyph(glyph) = item else {
                continue;
            };
            if !glyph.mapped {
                // A character the reader had to substitute is reported, not
                // silently inserted into the text of a document.
                continue;
            }
            if let Some(previous) = previous_baseline(&self.content, glyph) {
                if (previous - glyph.y).abs() > glyph.size.max(1.0) {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            let _ = last_baseline;
            last_baseline = glyph.y;
            out.push_str(&glyph.text);
        }
        out
    }

    /// The items, for a converter.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.content.items
    }
}

fn previous_baseline(content: &Content, glyph: &crate::content::Glyph) -> Option<f64> {
    let mut seen = false;
    for item in &content.items {
        let Item::Glyph(previous) = item else {
            continue;
        };
        if std::ptr::eq(previous, glyph) {
            return seen.then_some(f64::NAN);
        }
        seen = true;
    }
    None
}

/// The page's size and rotation, inheriting what it does not set.
fn geometry_of(
    dictionary: &lopdf::Dictionary,
    inherited: &BTreeMap<Vec<u8>, Object>,
) -> Result<PageGeometry> {
    // Every entry below is already a direct object by the time it arrives;
    // there is nothing left to resolve through a `lopdf::Document` stub.
    // (AUD-92: a former `Box::leak(resolver_stub())` here was unused and
    // failed LeakSanitizer under `fuzz_pdf`.)
    let get = |key: &[u8]| -> Option<Object> {
        dictionary
            .get(key)
            .ok()
            .cloned()
            .or_else(|| inherited.get(key).cloned())
    };
    let media = get(b"MediaBox").ok_or_else(|| PdfError::Missing("MediaBox".to_owned()))?;
    let items = match &media {
        Object::Array(items) => items.clone(),
        _ => return Err(PdfError::Missing("MediaBox is not an array".to_owned())),
    };
    let mut numbers = [0.0f64; 4];
    for (index, slot) in numbers.iter_mut().enumerate() {
        *slot = items.get(index).and_then(number_of_object).unwrap_or(0.0);
    }
    let width = (numbers[2] - numbers[0]).abs();
    let height = (numbers[3] - numbers[1]).abs();
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(PdfError::Missing(format!(
            "MediaBox is degenerate ({numbers:?})"
        )));
    }
    // A `/CropBox` smaller than the media box is what gets *shown*, and a
    // consumer placing content on the page has to know that.
    if let Some(Object::Array(items)) = get(b"CropBox") {
        let mut crop_numbers = [0.0f64; 4];
        for (index, slot) in crop_numbers.iter_mut().enumerate() {
            *slot = items.get(index).and_then(number_of_object).unwrap_or(0.0);
        }
        let crop_width = (crop_numbers[2] - crop_numbers[0]).abs();
        let crop_height = (crop_numbers[3] - crop_numbers[1]).abs();
        if crop_width > 0.0 && crop_height > 0.0 {
            return Ok(PageGeometry {
                width: crop_width,
                height: crop_height,
                rotation: rotation_of(get(b"Rotate").as_ref()),
            });
        }
    }
    Ok(PageGeometry {
        width,
        height,
        rotation: rotation_of(get(b"Rotate").as_ref()),
    })
}

/// `/Rotate` normalised to a right angle, as the specification requires.
fn rotation_of(object: Option<&Object>) -> i32 {
    let raw = object.and_then(number_of_object).unwrap_or(0.0).round() as i64;
    raw.rem_euclid(360) as i32
}

fn number_of_object(object: &Object) -> Option<f64> {
    match object {
        // A PDF number is a length, a colour component or a rotation; all of
        // them fit an `f64` exactly, and `i64` beyond 2^53 is not a coordinate.
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

/// A page's (or a form's) fonts and XObjects, resolved through the document.
///
/// A form XObject carries **its own** resource dictionary, and a form that
/// declares none inherits the page that invoked it (ISO 32000-1 §8.10.2), so the
/// two are the same type with a parent link rather than two types.
struct PageResources<'a> {
    document: &'a lopdf::Document,
    /// The document's picture cache: a form's pictures are the document's.
    cache: &'a ImageCache,
    /// The document's form cache: a form drawn many times is decoded once.
    form_cache: &'a FormCache,
    /// The document's font cache: a font object is decoded once (AUD-13).
    font_cache: &'a FontCache,
    fonts: BTreeMap<String, PdfFont>,
    images: BTreeMap<String, lopdf::ObjectId>,
    forms: BTreeMap<String, lopdf::ObjectId>,
    limits: PdfLimits,
    /// The resources a form falls back on, when the form declares none itself.
    parent: Option<&'a PageResources<'a>>,
    /// What reading this page's fonts cost; carried out of the loader so the
    /// report can be written once the page's content has been interpreted.
    notes: Vec<(String, String)>,
}

impl<'a> PageResources<'a> {
    fn new(
        document: &'a lopdf::Document,
        cache: &'a ImageCache,
        form_cache: &'a FormCache,
        font_cache: &'a FontCache,
        inherited: &BTreeMap<Vec<u8>, Object>,
        limits: PdfLimits,
    ) -> Self {
        Self::load(
            document, cache, form_cache, font_cache, inherited, limits, None,
        )
    }

    /// Builds the resource set of a form, falling back on `parent`.
    fn for_form(
        document: &'a lopdf::Document,
        cache: &'a ImageCache,
        form_cache: &'a FormCache,
        font_cache: &'a FontCache,
        inherited: &BTreeMap<Vec<u8>, Object>,
        limits: PdfLimits,
        parent: &'a PageResources<'a>,
    ) -> Self {
        Self::load(
            document,
            cache,
            form_cache,
            font_cache,
            inherited,
            limits,
            Some(parent),
        )
    }

    fn load(
        document: &'a lopdf::Document,
        cache: &'a ImageCache,
        form_cache: &'a FormCache,
        font_cache: &'a FontCache,
        inherited: &BTreeMap<Vec<u8>, Object>,
        limits: PdfLimits,
        parent: Option<&'a PageResources<'a>>,
    ) -> Self {
        let resolve = resolver(document);
        let mut fonts = BTreeMap::new();
        let mut images = BTreeMap::new();
        let mut forms = BTreeMap::new();
        let mut notes: Vec<(String, String)> = Vec::new();
        let Some(resources) = inherited
            .get(b"Resources".as_slice())
            .and_then(|value| resolve.get(value))
            .and_then(|value| value.as_dict().ok())
        else {
            return Self {
                document,
                cache,
                form_cache,
                font_cache,
                fonts,
                images,
                forms,
                limits,
                parent,
                notes,
            };
        };

        // `/Font` and `/XObject` are independent: a page whose only resource is a
        // picture has no `/Font` at all, and a reader that stops looking when
        // the font table is missing never finds the picture. That is the normal
        // case for a scanned document.
        if let Some(fonts_object) = resources.get(b"Font").ok().cloned() {
            for (name, value) in dictionary_entries(resolve, &fonts_object) {
                let Some(id) = value.as_reference().ok() else {
                    continue;
                };
                let Some(dictionary) = document
                    .get_object(id)
                    .ok()
                    .and_then(|object| object.as_dict().ok())
                else {
                    continue;
                };
                // Through the document's font cache: the same object under two
                // names, or on two pages, is decoded once, and `max_fonts` is
                // checked at insert (AUD-13).
                let font = font_cache.get_or_build(id, &name, dictionary, document, &limits);
                notes.extend(font.notes.iter().cloned());
                fonts.insert(name, font);
            }
        }

        if let Some(xobjects) = resources.get(b"XObject").ok().cloned() {
            for (name, value) in dictionary_entries(resolve, &xobjects) {
                let Ok(id) = value.as_reference() else {
                    continue;
                };
                // An XObject is a picture **or** a form, and the two live in one
                // table. A reader that only recognises the first half silently
                // drops every drawing that arrives through the second.
                let subtype = document
                    .get_object(id)
                    .ok()
                    .and_then(|object| object.as_stream().ok())
                    .and_then(|stream| stream.dict.get(b"Subtype").ok())
                    .and_then(|value| value.as_name().ok());
                match subtype {
                    Some(b"Form") => {
                        forms.insert(name, id);
                    }
                    // An XObject with no `/Subtype` is an image by the
                    // specification's default, and treating it as anything else
                    // would lose a picture on a technicality.
                    Some(b"Image") | None => {
                        images.insert(name, id);
                    }
                    Some(_) => {}
                }
            }
        }

        Self {
            document,
            cache,
            form_cache,
            font_cache,
            fonts,
            images,
            forms,
            limits,
            parent,
            notes,
        }
    }

    /// What reading this page's fonts cost, as `(report id, detail)`.
    fn notes(&self) -> &[(String, String)] {
        &self.notes
    }
}

/// The entries of a resource dictionary, dereferencing it first.
///
/// `/Font` and `/XObject` are **usually indirect**: Word writes
/// `/Resources<</Font 17 0 R>>`, and so does every producer that shares one font
/// table between pages. Reading the entries off the raw object therefore finds
/// nothing, and the page comes back with no text and no pictures — silently,
/// because a font that is not in the table is reported as missing and the text
/// that needed it is simply not drawn.
fn dictionary_entries(
    resolve: crate::fonts::Resolver<'_>,
    object: &Object,
) -> Vec<(String, Object)> {
    let Some(dictionary) = resolve.get(object).and_then(|value| value.as_dict().ok()) else {
        return Vec::new();
    };
    dictionary
        .iter()
        .map(|(name, value)| (String::from_utf8_lossy(name).into_owned(), value.clone()))
        .collect()
}

impl ResourceProvider for PageResources<'_> {
    fn font(&self, name: &str) -> Option<&PdfFont> {
        // A form's own name first, then the page it was invoked from: a form that
        // declares three fonts and uses a fourth is legal, and the fourth is the
        // page's.
        self.fonts
            .get(name)
            .or_else(|| self.parent.and_then(|parent| parent.font(name)))
    }

    fn image(&self, name: &str) -> Result<Option<(Encoded, u32, u32)>> {
        let Some(id) = self.images.get(name).copied() else {
            return match self.parent {
                Some(parent) => parent.image(name),
                None => Ok(None),
            };
        };
        // Through the document's cache: the same picture drawn on the next page
        // is decoded once for the document, not once per draw (Q-26).
        match self.cache.decode(id, self.document, &self.limits) {
            Ok(image) => Ok(Some(image)),
            Err(reject) => Err(PdfError::Refused(reject.to_string())),
        }
    }

    fn has_xobject(&self, name: &str) -> bool {
        self.images.contains_key(name)
            || self.forms.contains_key(name)
            || self.parent.is_some_and(|parent| parent.has_xobject(name))
    }

    fn is_form(&self, name: &str) -> bool {
        self.forms.contains_key(name)
    }

    fn form(&self, name: &str) -> Option<crate::content::Form<'_>> {
        let id = *self.forms.get(name)?;
        // Through the document's form cache: a form drawn ten thousand times is
        // inflated once, not once per `Do` (AUD-13).
        let decoded = self
            .form_cache
            .get_or_decode(id, self.document, &self.limits)?;
        // A form with no `/Resources` of its own inherits the invoking page's, so
        // an empty dictionary is treated the same way: a producer that writes
        // `/Resources <<>>` meant "nothing extra", not "nothing at all".
        let inherited: BTreeMap<Vec<u8>, Object> = match &decoded.resources {
            Some(resources) => {
                let mut map = BTreeMap::new();
                map.insert(b"Resources".to_vec(), resources.clone());
                map
            }
            None => BTreeMap::new(),
        };
        let child = PageResources::for_form(
            self.document,
            self.cache,
            self.form_cache,
            self.font_cache,
            &inherited,
            self.limits,
            self,
        );
        Some(crate::content::Form {
            operations: Rc::clone(&decoded.operations),
            inlines: Rc::clone(&decoded.inlines),
            matrix: decoded.matrix,
            resources: Rc::new(child),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{active_content, rotation_of, PdfDocument, MAX_ANNOTATIONS_SCANNED};
    use crate::error::{LimitKind, PdfError, PdfLimits};
    use lopdf::{dictionary, Document, Object, Stream};

    /// A document of `pages` pages drawing `content`, each page carrying
    /// `annotation` when one is given, with `catalog_entries` added to the
    /// catalog.
    fn document_with(
        pages: usize,
        content: &[u8],
        annotation: Option<&lopdf::Dictionary>,
        catalog_entries: Vec<(&'static str, Object)>,
    ) -> Document {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..pages {
            let content_id = document.add_object(Stream::new(dictionary! {}, content.to_vec()));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Contents" => content_id,
            };
            if let Some(annotation) = annotation {
                let annotation_id = document.add_object(annotation.clone());
                page.set("Annots", vec![Object::Reference(annotation_id)]);
            }
            kids.push(Object::Reference(document.add_object(page)));
        }
        let count = i64::try_from(kids.len()).expect("page count");
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
        for (key, value) in catalog_entries {
            catalog.set(key, value);
        }
        let catalog_id = document.add_object(catalog);
        document.trailer.set("Root", catalog_id);
        document
    }

    fn bytes_of(mut document: Document) -> Vec<u8> {
        let mut out = Vec::new();
        document.save_to(&mut out).expect("save");
        out
    }

    fn action(kind: &str) -> lopdf::Dictionary {
        dictionary! {
            "Type" => "Action",
            "S" => kind,
            "JS" => Object::string_literal("app.alert(1)"),
        }
    }

    fn ids(found: &[(&'static str, &'static str)]) -> Vec<&'static str> {
        found.iter().map(|(id, _)| *id).collect()
    }

    #[test]
    fn a_plain_document_reports_no_active_content() {
        let document = document_with(2, b"0 0 m 10 10 l S", None, Vec::new());
        assert!(active_content(&document).is_empty());
    }

    #[test]
    fn catalog_actions_scripts_and_attachments_are_reported() {
        let names = dictionary! {
            "JavaScript" => dictionary! { "Names" => Vec::<Object>::new() },
            "EmbeddedFiles" => dictionary! { "Names" => Vec::<Object>::new() },
        };
        let document = document_with(
            1,
            b"",
            None,
            vec![
                ("OpenAction", Object::Dictionary(action("JavaScript"))),
                ("AA", Object::Dictionary(dictionary! {})),
                ("Names", Object::Dictionary(names)),
            ],
        );
        let found = ids(&active_content(&document));
        for id in [
            "pdf.active.open-action",
            "pdf.active.additional-actions",
            "pdf.active.javascript",
            "pdf.active.embedded-files",
        ] {
            assert!(found.contains(&id), "{id} missing from {found:?}");
        }
    }

    #[test]
    fn an_open_action_that_only_scrolls_is_not_reported() {
        let document = document_with(
            1,
            b"",
            None,
            vec![("OpenAction", Object::Dictionary(action("GoTo")))],
        );
        assert!(active_content(&document).is_empty());
    }

    #[test]
    fn annotations_that_leave_the_page_are_reported_and_links_are_not() {
        for kind in ["JavaScript", "Launch", "GoToR", "ImportData", "SubmitForm"] {
            let annotation = dictionary! {
                "Type" => "Annot",
                "Subtype" => "Link",
                "A" => action(kind),
            };
            let document = document_with(1, b"", Some(&annotation), Vec::new());
            let found = active_content(&document);
            assert_eq!(ids(&found), ["pdf.active.annotation"], "{kind}");
            assert!(found[0].1.contains(kind), "{kind}: {}", found[0].1);
        }
        let link = dictionary! { "Type" => "Annot", "Subtype" => "Link", "A" => action("URI") };
        let document = document_with(1, b"", Some(&link), Vec::new());
        assert!(active_content(&document).is_empty());
    }

    #[test]
    fn the_annotation_scan_is_capped() {
        let annotation = dictionary! { "Type" => "Annot", "Subtype" => "Text" };
        let mut document = document_with(1, b"", None, Vec::new());
        let annotation_id = document.add_object(annotation);
        let many = vec![Object::Reference(annotation_id); MAX_ANNOTATIONS_SCANNED + 5];
        let page = document.page_iter().next().expect("one page");
        document
            .get_dictionary_mut(page)
            .expect("page dictionary")
            .set("Annots", many);
        let found = ids(&active_content(&document));
        assert_eq!(found, ["pdf.active.scan-capped"]);
    }

    #[test]
    fn open_records_active_content_without_refusing_the_file() {
        let annotation = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "A" => action("Launch"),
        };
        let bytes = bytes_of(document_with(
            2,
            b"0 0 m 10 10 l S",
            Some(&annotation),
            vec![("OpenAction", Object::Dictionary(action("JavaScript")))],
        ));
        let mut pdf = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
        let count_of = |id: &str| {
            pdf.report()
                .losses()
                .iter()
                .find(|loss| loss.id == id)
                .map_or(0, |loss| loss.count)
        };
        assert_eq!(count_of("pdf.active.open-action"), 1);
        assert_eq!(count_of("pdf.active.annotation"), 2);
        // Information only: the pages still read.
        assert_eq!(pdf.pages().expect("pages").len(), 2);
    }

    #[test]
    fn the_document_content_budget_spans_pages() {
        // Three pages of 15 bytes, which lopdf hands back with a newline after
        // each stream: 16 per page, so a 40-byte document budget lets two
        // through.
        let content = b"0 0 m 10 10 l S";
        let bytes = bytes_of(document_with(3, content, None, Vec::new()));
        let limits = PdfLimits {
            max_total_content_bytes: 40,
            ..PdfLimits::default()
        };
        let mut pdf = PdfDocument::open(&bytes, limits).expect("open");
        pdf.page(1).expect("first page");
        pdf.page(2).expect("second page");
        assert_eq!(pdf.content_bytes_read(), 32);
        let error = pdf.page(3).expect_err("third page is over the document budget");
        assert!(
            matches!(
                error,
                PdfError::LimitExceeded {
                    kind: LimitKind::TotalContentBytes,
                    limit: 40,
                    actual: 48,
                }
            ),
            "{error}"
        );
        // Charged even on refusal: a smaller read does not slip back under.
        pdf.page(1).expect_err("the budget stays spent");
    }

    #[test]
    fn the_default_document_budget_reads_an_ordinary_document() {
        let bytes = bytes_of(document_with(4, b"0 0 m 10 10 l S", None, Vec::new()));
        let mut pdf = PdfDocument::open(&bytes, PdfLimits::default()).expect("open");
        assert_eq!(pdf.pages().expect("pages").len(), 4);
        assert_eq!(pdf.content_bytes_read(), 64);
    }

    #[test]
    fn rotation_is_normalised_to_a_right_angle() {
        assert_eq!(rotation_of(Some(&Object::Integer(0))), 0);
        assert_eq!(rotation_of(Some(&Object::Integer(90))), 90);
        assert_eq!(rotation_of(Some(&Object::Integer(360))), 0);
        assert_eq!(rotation_of(Some(&Object::Integer(-90))), 270);
        assert_eq!(rotation_of(Some(&Object::Integer(450))), 90);
        assert_eq!(rotation_of(Some(&Object::Real(180.0))), 180);
        assert_eq!(rotation_of(None), 0);
    }
}
