//! Image XObjects: what can be carried out of a PDF, and what cannot.
//!
//! A PDF stores an image as *compressed samples plus a colour space*, and the two
//! filters that matter are `/DCTDecode` (JPEG, which is the codec itself) and
//! `/FlateDecode` (raw samples). Everything else — JPEG 2000, CCITT fax, JBIG2 —
//! is a codec this crate does not carry, and a document using one is reported
//! rather than approximated.
//!
//! What comes out is deliberately the same [`Encoded`] shape the PDF writer of
//! stage 8B takes, so a PDF → WML converter and a PDF → PDF rewrite share one
//! image representation instead of two.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use lopdf::Object;

use crate::error::{LimitKind, PdfLimits};
use crate::fonts::{name_of, number, resolver, Resolver};

/// A decoded image.
#[derive(Clone, Debug, PartialEq)]
pub enum Encoded {
    /// JPEG bytes, carried through untouched.
    Jpeg {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The original bytes.
        data: Arc<Vec<u8>>,
        /// The soft mask, when the image has an alpha channel.
        alpha: Option<Box<Encoded>>,
    },
    /// Raw samples, 8 bits per component.
    Raw {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Interleaved components, 8 bits each.
        samples: Arc<Vec<u8>>,
        /// Components per pixel.
        components: u8,
        /// The alpha channel, when the image has one.
        alpha: Option<Arc<Vec<u8>>>,
    },
}

impl Encoded {
    /// The size the image will be drawn at, from its own dimensions.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        match self {
            Encoded::Jpeg { width, height, .. } | Encoded::Raw { width, height, .. } => {
                (*width, *height)
            }
        }
    }

    /// A short label for a report line.
    #[must_use]
    pub fn describe(&self) -> String {
        let (width, height) = self.pixel_size();
        match self {
            Encoded::Jpeg { .. } => format!("jpeg {width}x{height}"),
            Encoded::Raw { components, .. } => format!("raw {width}x{height}x{components}"),
        }
    }

    /// The JPEG bytes, when the image is a JPEG.
    #[must_use]
    pub fn jpeg_bytes(&self) -> Option<&[u8]> {
        match self {
            Encoded::Jpeg { data, .. } => Some(data.as_slice()),
            Encoded::Raw { .. } => None,
        }
    }

    /// The raw samples and their component count, when the image has any.
    #[must_use]
    pub fn raw_samples(&self) -> Option<(&[u8], u8)> {
        match self {
            Encoded::Raw {
                samples,
                components,
                ..
            } => Some((samples.as_slice(), *components)),
            Encoded::Jpeg { .. } => None,
        }
    }

    /// The alpha channel, when the image has one.
    #[must_use]
    pub fn alpha_channel(&self) -> Option<Vec<u8>> {
        match self {
            Encoded::Raw { alpha, .. } => alpha.as_ref().map(|mask| mask.as_ref().clone()),
            Encoded::Jpeg { alpha, .. } => alpha.as_ref().and_then(|mask| match mask.as_ref() {
                Encoded::Raw { samples, .. } => Some(samples.as_ref().clone()),
                Encoded::Jpeg { .. } => None,
            }),
        }
    }

    /// The image as PNG bytes, which is what a `.docx` media part needs.
    ///
    /// A JPEG is returned as it is: re-encoding it would need a JPEG decoder
    /// this crate does not carry, and the bytes are already a valid image.
    ///
    /// # Errors
    ///
    /// Returns `png::EncodingError` when the samples cannot be laid out.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let Some((samples, components)) = self.raw_samples() else {
            return Ok(self.jpeg_bytes().unwrap_or_default().to_vec());
        };
        let (width, height) = self.pixel_size();
        let mut out = Vec::new();
        {
            let color = match components {
                1 => png::ColorType::Grayscale,
                3 => png::ColorType::Rgb,
                _ => png::ColorType::Rgba,
            };
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(samples)?;
        }
        Ok(out)
    }
}

/// Why an image could not be carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// The filter is a codec this crate does not carry.
    UnsupportedFilter(&'static str),
    /// The samples are 16-bit, a palette, an image mask, or otherwise not laid
    /// out as 8-bit components, and converting them is a decision not made here.
    UnsupportedLayout(&'static str),
    /// The dictionary is missing what an image needs.
    Incomplete(&'static str),
    /// The file itself is broken: the stream is there and does not decode, or it
    /// decodes to less than the dictionary promised.
    ///
    /// Separate from [`Reject::Incomplete`] because the two send a reader of the
    /// report to different places. "The dictionary is missing `Width`" means the
    /// producer wrote an image object we cannot read; "the samples are
    /// truncated" means **this file is damaged**, and the first version of this
    /// reader reported a broken stream as a missing dictionary — a wrong reason
    /// for a real loss, which is worse than no reason at all.
    Broken(&'static str),
    /// The samples exceed the budget.
    TooLarge,
}

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFilter(name) => {
                write!(f, "image filter {name} is not carried by this reader")
            }
            Self::UnsupportedLayout(what) => {
                write!(f, "image layout ({what}) is not carried by this reader")
            }
            Self::Incomplete(what) => write!(f, "image dictionary is missing {what}"),
            Self::Broken(what) => write!(f, "{what}"),
            Self::TooLarge => f.write_str("image exceeds the decoding budget"),
        }
    }
}

/// The budget an image decode respects, named for the report.
#[must_use]
pub const fn image_limit_kind() -> LimitKind {
    LimitKind::ImageBytes
}

/// How many bytes of decoded pictures one image's payload is worth.
///
/// What the cache is charged for, and what a caller decides it can afford: the
/// inflated samples, not the compressed stream the document carries.
fn payload_bytes(encoded: &Encoded) -> usize {
    match encoded {
        Encoded::Jpeg { data, alpha, .. } => {
            data.len() + alpha.as_ref().map_or(0, |mask| payload_bytes(mask))
        }
        Encoded::Raw { samples, alpha, .. } => {
            samples.len() + alpha.as_ref().map_or(0, |mask| mask.len())
        }
    }
}

/// Picture XObjects already decoded in this document, by object id.
///
/// A `Do` decodes the picture it draws, and a document that draws the same logo
/// on every page — or the same scanned figure on every spread — decodes it once
/// per draw. `Балашова` (139 MiB, 160 pages) draws 102 902 pictures, and before
/// this cache every one of them was an inflate of a stream the reader had
/// already inflated, on a page that was holding the result anyway.
///
/// **On the document, not on the page.** The second page of a spread draws what
/// the first drew, and a per-page cache would decode it twice; the pages of a
/// document are also read one after another, so a per-page cache is 160 maps
/// instead of one. A hit is a hash lookup and a refcount: the page's own
/// `PlacedImage` already holds an `Arc` to the same bytes.
///
/// **Bounded, because it outlives the page.** A caller that reads one page at a
/// time and drops it would otherwise keep every picture the document has, which
/// is the whole document in memory and not the page it asked for.
/// `PdfLimits::max_cached_image_bytes` is the ceiling; past it a picture is
/// decoded per draw again, which is what always happened. Nothing is evicted: an
/// entry nobody draws again costs what it cost, and a policy that guessed wrong
/// would cost more than it saves.
///
/// A refusal is cached too, and for free — a broken stream re-inflated on every
/// draw is a real cost, and the refusal is a fact about the object rather than
/// about the draw. The caller still records the loss for each draw, because the
/// number of draws is what a report is about.
///
/// The map is never iterated, so its order cannot reach any output.
pub(crate) struct ImageCache {
    entries: RefCell<HashMap<lopdf::ObjectId, CachedImage>>,
    /// What the entries are worth, so the ceiling is about bytes and not about
    /// the number of pictures — a document of 10 000 tiny icons is not a
    /// document of 10 000 large photographs.
    bytes: Cell<usize>,
    limit: usize,
    /// The ids being decoded right now; an id that reappears is a `/SMask` cycle.
    in_progress: RefCell<BTreeSet<lopdf::ObjectId>>,
    /// Whether a cycle was refused, for the page's report.
    cycle: Cell<bool>,
}

struct CachedImage {
    result: Result<(Encoded, u32, u32), Reject>,
}

impl ImageCache {
    /// A cache that will hold up to `limit` bytes of decoded pictures.
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            entries: RefCell::new(HashMap::new()),
            bytes: Cell::new(0),
            limit,
            in_progress: RefCell::new(BTreeSet::new()),
            cycle: Cell::new(false),
        }
    }

    /// The decoded picture, decoded at most once per document.
    ///
    /// # Errors
    ///
    /// Returns a [`Reject`] describing what the image is that this reader will
    /// not carry — and the same one every time, because the answer is a property
    /// of the object.
    pub(crate) fn decode(
        &self,
        id: lopdf::ObjectId,
        document: &lopdf::Document,
        limits: &PdfLimits,
    ) -> Result<(Encoded, u32, u32), Reject> {
        if let Some(hit) = self.entries.borrow().get(&id) {
            return hit.result.clone();
        }
        // An image whose `/SMask` points at itself, or a cycle A -> B -> A,
        // made this recurse until the stack ended. The set of ids currently being
        // decoded is what says so: an id already in it is a cycle, and the mask it
        // names is simply absent (AUD-12).
        if !self.in_progress.borrow_mut().insert(id) {
            self.cycle.set(true);
            return Err(Reject::Incomplete("SMask"));
        }
        let result = decode_inner(id, document, limits, &|mask_id| {
            self.decode_mask(mask_id, document, limits).ok()
        });
        self.in_progress.borrow_mut().remove(&id);
        let bytes = match &result {
            Ok((encoded, _, _)) => payload_bytes(encoded),
            Err(_) => 0,
        };
        let mut entries = self.entries.borrow_mut();
        if self.bytes.get() + bytes <= self.limit {
            self.bytes.set(self.bytes.get() + bytes);
            entries.insert(
                id,
                CachedImage {
                    result: result.clone(),
                },
            );
        }
        result
    }

    /// Decodes a soft mask, and only ever a soft mask.
    ///
    /// ISO 32000-1 8.9.2.4: an `/SMask` is a grayscale image and an image has
    /// one alpha channel, of which its `/SMask` *is* that channel. A mask with a
    /// mask of its own is not a thing, so a mask is decoded with `mask_of` that
    /// never fires - which also makes the A -> B -> A cycle unreachable from the
    /// mask side at all, leaving the set in [`decode`](Self::decode) as the second
    /// line of defence rather than the only one.
    fn decode_mask(
        &self,
        id: lopdf::ObjectId,
        document: &lopdf::Document,
        limits: &PdfLimits,
    ) -> Result<(Encoded, u32, u32), Reject> {
        // A mask that names a mask is a cycle, whether it names itself or the
        // picture two steps back. ISO 32000-1 8.9.2.4 gives an image one alpha
        // channel and its `/SMask` *is* that channel, so a mask has nothing to
        // point at; a document that writes one anyway is asking for a loop, and
        // the answer is a mask with no mask of its own and a line in the report.
        if let Ok(Ok(stream)) = document.get_object(id).map(lopdf::Object::as_stream) {
            if stream.dict.get(b"SMask").is_ok() {
                self.cycle.set(true);
            }
        }
        decode_inner(id, document, limits, &|_mask_id| None)
    }

    /// Whether any decode of this cache hit a `/SMask` cycle.
    pub(crate) fn saw_mask_cycle(&self) -> bool {
        self.cycle.get()
    }

    /// How many bytes of decoded pictures are held.
    ///
    /// Named for a caller that wants to know what the cache is worth, and for a
    /// test that wants to prove a second `Do` decoded nothing.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes.get()
    }

    /// How many distinct pictures are held, refusals included.
    pub(crate) fn distinct_images(&self) -> usize {
        self.entries.borrow().len()
    }
}

/// Decodes the image XObject at `id`.
///
/// # Errors
///
/// Returns a [`Reject`] describing what the image is that this reader will not
/// carry.
pub fn decode_from(
    id: lopdf::ObjectId,
    document: &lopdf::Document,
    limits: &PdfLimits,
) -> Result<(Encoded, u32, u32), Reject> {
    // Without a cache, a soft mask is decoded by decoding it: the public entry
    // point is the one place that has no cache to ask.
    decode_inner(id, document, limits, &|mask_id| {
        decode_from(mask_id, document, limits).ok()
    })
}

/// The decode itself, with the soft mask fetched through `mask_of`.
///
/// A soft mask is an image in its own right, and the caller that knows about a
/// cache is the one that should be asked for it — a mask shared by a hundred
/// draws of the same logo is a hundred inflates of one stream.
fn decode_inner(
    id: lopdf::ObjectId,
    document: &lopdf::Document,
    limits: &PdfLimits,
    mask_of: &dyn Fn(lopdf::ObjectId) -> Option<(Encoded, u32, u32)>,
) -> Result<(Encoded, u32, u32), Reject> {
    let resolve = resolver(document);
    let object = document
        .get_object(id)
        .map_err(|_| Reject::Incomplete("XObject"))?;
    let stream = object
        .as_stream()
        .map_err(|_| Reject::Incomplete("XObject stream"))?;
    let dictionary = &stream.dict;

    let width = number(dictionary, b"Width")
        .filter(|value| *value > 0.0)
        .ok_or(Reject::Incomplete("Width"))? as u32;
    let height = number(dictionary, b"Height")
        .filter(|value| *value > 0.0)
        .ok_or(Reject::Incomplete("Height"))? as u32;

    // An image mask is a 1-bit stencil, not a picture: it is only ever a soft
    // mask or a fill colour, and decoding it as pixels would produce a page of
    // black or white boxes.
    if dictionary
        .get(b"ImageMask")
        .is_ok_and(|value| value.as_bool().unwrap_or(false))
    {
        return Err(Reject::UnsupportedLayout("image mask"));
    }
    let bits = number(dictionary, b"BitsPerComponent").unwrap_or(8.0) as u32;
    if bits != 8 {
        return Err(Reject::UnsupportedLayout("bits per component"));
    }

    let filters = filter_names(dictionary, resolve);
    let components = components_for(dictionary, &filters, resolve);
    let first = filters.first().map(Vec::as_slice);

    let mut encoded = match first {
        Some(b"DCTDecode") => {
            // The filter *is* the codec, so the bytes go in untouched.
            let bytes = stream.content.clone();
            let (jpeg_width, jpeg_height) = jpeg_size(&bytes).unwrap_or((width, height));
            Encoded::Jpeg {
                width: jpeg_width,
                height: jpeg_height,
                data: Arc::new(bytes),
                alpha: None,
            }
        }
        None | Some(b"FlateDecode") => {
            // The filter is carried; a stream that will not inflate is a broken
            // file, and saying "this filter is not carried" for it is a wrong
            // reason for a real loss — the kind of message that sends somebody to
            // look for a codec instead of at the file. It is also not a *missing
            // dictionary*, which is what the same message used to say.
            let samples = stream.decompressed_content().map_err(|_| match first {
                Some(_) => Reject::Broken("flate samples could not be inflated"),
                None => Reject::Broken("samples are not a decodable stream"),
            })?;
            let expected = (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(components as usize);
            if samples.len() < expected {
                return Err(Reject::Broken("samples are truncated"));
            }
            if samples.len() > limits.max_image_bytes {
                return Err(Reject::TooLarge);
            }
            Encoded::Raw {
                width,
                height,
                samples: Arc::new(samples),
                components,
                alpha: None,
            }
        }
        Some(other) => {
            // The filter name comes from the file, so the reason has to own it.
            let leaked: &'static str =
                Box::leak(String::from_utf8_lossy(other).into_owned().into_boxed_str());
            return Err(Reject::UnsupportedFilter(leaked));
        }
    };

    // A soft mask is an image in its own right; carrying it is what keeps a
    // PNG's transparency alive through the conversion.
    if let Ok(mask_id) = dictionary.get(b"SMask").and_then(Object::as_reference) {
        if let Some((mask, _, _)) = mask_of(mask_id) {
            match (&mut encoded, &mask) {
                (Encoded::Jpeg { alpha: slot, .. }, _) => {
                    *slot = Some(Box::new(mask));
                }
                (
                    Encoded::Raw { alpha, .. },
                    Encoded::Raw {
                        samples: mask_samples,
                        ..
                    },
                ) => {
                    *alpha = Some(Arc::clone(mask_samples));
                }
                _ => {}
            }
        }
    }
    Ok((encoded, width, height))
}

/// The filter chain of a stream, as names.
fn filter_names(dictionary: &lopdf::Dictionary, resolve: Resolver<'_>) -> Vec<Vec<u8>> {
    let Ok(filters) = dictionary.get(b"Filter") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match resolve.get(filters) {
        Some(Object::Name(name)) => out.push(name.clone()),
        Some(Object::Array(items)) => {
            for item in items {
                if let Some(name) = resolve.get(item).and_then(|value| value.as_name().ok()) {
                    out.push(name.to_vec());
                }
            }
        }
        _ => {}
    }
    out
}

/// The filter chain of a stream, as text, for a report.
#[must_use]
pub fn declared_filters(dictionary: &lopdf::Dictionary, resolve: Resolver<'_>) -> Vec<String> {
    filter_names(dictionary, resolve)
        .into_iter()
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect()
}

/// How many components per pixel the samples carry.
fn components_for(
    dictionary: &lopdf::Dictionary,
    filters: &[Vec<u8>],
    resolve: Resolver<'_>,
) -> u8 {
    let space = name_of(dictionary, b"ColorSpace", resolve);
    if filters.first().is_some_and(|name| name == b"DCTDecode") {
        // A JPEG's components are in its own headers; three is the assumption
        // every producer makes and the only one a reader can check.
        return match space.as_deref() {
            Some(b"DeviceGray") => 1,
            Some(b"DeviceCMYK") => 4,
            _ => 3,
        };
    }
    match space.as_deref() {
        Some(b"DeviceGray" | b"CalGray" | b"G") => 1,
        Some(b"DeviceCMYK" | b"CMYK") => 4,
        _ => 3,
    }
}

/// A JPEG's dimensions, read from its SOF marker.
///
/// The compressed data is never touched, so a truncated JPEG still yields a size
/// and the reader can place it.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            if width == 0 || height == 0 {
                return None;
            }
            return Some((u32::from(width), u32::from(height)));
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]);
        if length < 2 {
            return None;
        }
        index += 2 + usize::from(length);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Encoded;

    /// A JPEG header followed by nothing: enough for the size reader.
    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&64u16.to_be_bytes());
        bytes.extend_from_slice(&32u16.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    #[test]
    fn a_jpeg_reports_its_own_size_and_keeps_its_bytes() {
        let bytes = jpeg();
        let image = Encoded::Jpeg {
            width: 32,
            height: 64,
            data: std::sync::Arc::new(bytes.clone()),
            alpha: None,
        };
        assert_eq!(image.pixel_size(), (32, 64));
        assert_eq!(image.jpeg_bytes(), Some(bytes.as_slice()));
        assert!(image.raw_samples().is_none());
        assert!(image.describe().starts_with("jpeg 32x64"));
        // A JPEG is already an image file; the PNG path hands it back untouched.
        assert_eq!(image.to_png().expect("bytes"), bytes);
    }

    #[test]
    fn raw_samples_come_back_with_their_component_count() {
        let image = Encoded::Raw {
            width: 2,
            height: 1,
            samples: std::sync::Arc::new(vec![255, 0, 0, 0, 255, 0]),
            components: 3,
            alpha: None,
        };
        assert_eq!(image.pixel_size(), (2, 1));
        assert_eq!(image.raw_samples().map(|(_, c)| c), Some(3));
        assert!(image.alpha_channel().is_none());
        let png = image.to_png().expect("a 2x1 RGB png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn an_alpha_channel_is_readable() {
        let image = Encoded::Raw {
            width: 2,
            height: 1,
            samples: std::sync::Arc::new(vec![0, 0, 0, 255, 255, 255]),
            components: 3,
            alpha: Some(std::sync::Arc::new(vec![255, 0])),
        };
        assert_eq!(image.alpha_channel(), Some(vec![255, 0]));
    }
}
