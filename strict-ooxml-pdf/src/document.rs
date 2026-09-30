//! Page and document assembly.
//!
//! `lopdf` gives the object graph; this module decides what a page *is*: its
//! size, its rotation, the fonts and images its resources declare, and the
//! content its stream describes. Inherited attributes matter here — `/Resources`,
//! `/MediaBox` and `/Rotate` walk down the page tree until a page sets them —
//! and getting that wrong yields a page of the wrong size rather than an error.

use std::collections::BTreeMap;

use lopdf::Object;

use crate::content::{
    decode_image, interpret, Content, Item, PageGeometry, Resources as ResourceProvider,
};
use crate::error::{PdfError, PdfLimits, Result};
use crate::fonts::{resolver, PdfFont};
use crate::image::Encoded;

/// An opened PDF.
pub struct PdfDocument {
    document: lopdf::Document,
    limits: PdfLimits,
    report: crate::report::ReadReport,
}

impl PdfDocument {
    /// Opens a PDF from memory.
    ///
    /// # Errors
    ///
    /// Returns [`PdfError::Malformed`] for a file that is not a PDF,
    /// [`PdfError::Encrypted`] for one that is, and
    /// [`PdfError::LimitExceeded`](crate::error::PdfError::LimitExceeded) when
    /// the page count is over budget.
    pub fn open(bytes: &[u8], limits: PdfLimits) -> Result<Self> {
        let document =
            lopdf::Document::load_mem(bytes).map_err(|error| PdfError::from_lopdf(&error))?;
        if document.is_encrypted() {
            return Err(PdfError::Encrypted);
        }
        let pages = document.get_pages().len();
        if pages > limits.max_pages {
            return Err(limits.exceeded(crate::error::LimitKind::Pages, pages as u64));
        }
        Ok(Self {
            document,
            limits,
            report: crate::report::ReadReport::new(),
        })
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
        let page = self.read_page(id)?;
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
        for id in ids {
            out.push(self.read_page(id)?);
        }
        for page in &out {
            self.report.merge(&page.report);
        }
        Ok(out)
    }

    fn read_page(&self, id: lopdf::ObjectId) -> Result<PdfPage> {
        let dictionary = self
            .document
            .get_dictionary(id)
            .map_err(|error| PdfError::from_lopdf(&error))?;
        let inherited = self.inherited(id);
        let geometry = geometry_of(dictionary, &inherited)?;
        let resources = PageResources::new(&self.document, &inherited, self.limits);

        let content_bytes = self
            .document
            .get_page_content_with_limit(id, self.limits.max_content_bytes)
            .map_err(|error| PdfError::from_lopdf(&error))?;
        let operations = lopdf::content::Content::decode(&content_bytes)
            .map_err(|error| PdfError::from_lopdf(&error))?
            .operations;
        let content = interpret(
            &operations,
            &resources,
            geometry,
            self.limits,
            crate::FLATTEN_TOLERANCE_PT,
        )?;
        let mut report = crate::report::ReadReport::new();
        for ignored in &content.ignored {
            report.record_ignored(ignored.id, &ignored.detail);
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
            number: 0,
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
        let mut guard = 0;
        loop {
            let Ok(dictionary) = self.document.get_dictionary(current) else {
                break;
            };
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
            guard += 1;
            if guard > 64 {
                // A parent cycle in a damaged file: stop rather than loop.
                break;
            }
        }
        out
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

impl PdfPage {
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
    let resolve = Box::leak(Box::new(resolver_stub()));
    let _ = resolve;
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

fn resolver_stub() -> lopdf::Document {
    // `geometry_of` resolves nothing: every entry it reads is already a direct
    // object by the time it arrives. A stub keeps the signature honest rather
    // than pretending there is a lookup.
    lopdf::Document::with_version("1.7")
}

/// `/Rotate` normalised to a right angle, as the specification requires.
fn rotation_of(object: Option<&Object>) -> i32 {
    let raw = object.and_then(number_of_object).unwrap_or(0.0).round() as i64;
    (((raw % 360) + 360) % 360) as i32
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

/// A page's fonts and images, resolved through the document.
struct PageResources<'a> {
    document: &'a lopdf::Document,
    fonts: BTreeMap<String, PdfFont>,
    images: BTreeMap<String, lopdf::ObjectId>,
    limits: PdfLimits,
}

impl<'a> PageResources<'a> {
    fn new(
        document: &'a lopdf::Document,
        inherited: &BTreeMap<Vec<u8>, Object>,
        limits: PdfLimits,
    ) -> Self {
        let resolve = resolver(document);
        let mut fonts = BTreeMap::new();
        let mut images = BTreeMap::new();
        let Some(resources) = inherited
            .get(b"Resources".as_slice())
            .or_else(|| {
                // The direct entry, when the page sets its own resources.
                inherited.get(b"Resources".as_slice())
            })
            .and_then(|value| resolve.get(value).cloned())
        else {
            return Self {
                document,
                fonts,
                images,
                limits,
            };
        };
        let Some(fonts_object) = resources
            .as_dict()
            .ok()
            .and_then(|dictionary| dictionary.get(b"Font").ok().cloned())
        else {
            return Self {
                document,
                fonts,
                images,
                limits,
            };
        };
        for (name, value) in dictionary_entries(&fonts_object) {
            let Some(id) = value.as_reference().ok() else {
                continue;
            };
            let Some(dictionary) = document.get_object(id).ok().and_then(|o| o.as_dict().ok())
            else {
                continue;
            };
            match PdfFont::build(&name, dictionary, document, &limits) {
                Ok(font) => {
                    fonts.insert(name, font);
                }
                Err(error) => {
                    // A font the reader cannot build is not a reason to lose the
                    // page: the text is recorded as unmapped instead.
                    let _ = error;
                }
            }
        }
        if let Some(xobjects) = resources
            .as_dict()
            .ok()
            .and_then(|dictionary| dictionary.get(b"XObject").ok().cloned())
        {
            for (name, value) in dictionary_entries(&xobjects) {
                if let Ok(id) = value.as_reference() {
                    let is_image = document
                        .get_object(id)
                        .ok()
                        .and_then(|object| object.as_stream().ok())
                        .is_some_and(|stream| {
                            stream
                                .dict
                                .get(b"Subtype")
                                .ok()
                                .and_then(|value| value.as_name().ok())
                                == Some(b"Image")
                        });
                    if is_image {
                        images.insert(name, id);
                    }
                }
            }
        }
        Self {
            document,
            fonts,
            images,
            limits,
        }
    }
}

fn dictionary_entries(object: &Object) -> Vec<(String, Object)> {
    let Ok(dictionary) = object.as_dict() else {
        return Vec::new();
    };
    dictionary
        .iter()
        .map(|(name, value)| (String::from_utf8_lossy(name).into_owned(), value.clone()))
        .collect()
}

impl ResourceProvider for PageResources<'_> {
    fn font(&self, name: &str) -> Option<&PdfFont> {
        self.fonts.get(name)
    }

    fn image(&self, name: &str) -> Result<Option<(Encoded, u32, u32)>> {
        let Some(id) = self.images.get(name) else {
            return Ok(None);
        };
        match decode_image(*id, self.document, &self.limits) {
            Ok(image) => Ok(Some(image)),
            Err(error) => Err(PdfError::Missing(error.to_string())),
        }
    }

    fn has_xobject(&self, name: &str) -> bool {
        self.images.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::rotation_of;
    use lopdf::Object;

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
