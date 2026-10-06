//! Image XObjects: what can be carried out of a PDF, and what cannot.
//!
//! A PDF stores an image as *compressed samples plus a colour space*. The filters
//! this crate carries are `/DCTDecode` (JPEG bytes untouched), `/FlateDecode`
//! (inflated samples; 8-bit as-is, 1-bit DeviceGray expanded to 8-bit),
//! `/JPXDecode` (JPEG 2000 → 8-bit [`Encoded::Raw`] via `hayro-jpeg2000`),
//! `/CCITTFaxDecode` (`hayro-ccitt` → 8-bit gray), and `/JBIG2Decode`
//! (`hayro-jbig2` → 8-bit gray). Unknown filters and layouts that are not
//! pictures (image masks, 16-bit, palettes) are reported rather than approximated.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reject {
    /// The filter is a codec this crate does not carry.
    UnsupportedFilter(String),
    /// The samples are a palette, an image mask, 16-bit, or otherwise not laid
    /// out as components this reader will expand, and converting them is a
    /// decision not made here. (1-bit DeviceGray is expanded to 8-bit.)
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

    let filters = filter_names(dictionary, resolve);
    let components = components_for(dictionary, &filters, resolve);
    let first = filters.first().map(Vec::as_slice);
    let bits = number(dictionary, b"BitsPerComponent").unwrap_or(8.0) as u32;
    // Codecs that carry bit depth in the bitstream (or always emit bi-level)
    // must not be refused for a dictionary `/BitsPerComponent` of 1 — that is
    // how every CCITT/JBIG2 scan in the wild is written, and an early reject
    // used to report them as "bits per component" instead of decoding them.
    let codec_carries_depth = matches!(
        first,
        Some(b"JPXDecode" | b"CCITTFaxDecode" | b"JBIG2Decode")
    );
    let expand_one_bit = !codec_carries_depth && bits == 1 && components == 1;
    if !codec_carries_depth && !expand_one_bit && bits != 8 {
        return Err(Reject::UnsupportedLayout("bits per component"));
    }

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
        Some(b"JPXDecode") => decode_jpx_bytes(&stream.content, limits)?,
        Some(b"CCITTFaxDecode") => {
            decode_ccitt_bytes(&stream.content, dictionary, width, height, resolve, limits)?
        }
        Some(b"JBIG2Decode") => {
            decode_jbig2_bytes(&stream.content, dictionary, document, resolve, limits)?
        }
        None | Some(b"FlateDecode") => decode_flate_or_raw(
            stream,
            first.is_some(),
            expand_one_bit,
            width,
            height,
            components,
            limits,
        )?,
        Some(other) => {
            // The filter name comes from the file, so the reason has to own it.
            return Err(Reject::UnsupportedFilter(
                String::from_utf8_lossy(other).into_owned(),
            ));
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

/// Inflate (or take raw) samples for a Flate/unfiltered image XObject.
fn decode_flate_or_raw(
    stream: &lopdf::Stream,
    filtered: bool,
    expand_one_bit: bool,
    width: u32,
    height: u32,
    components: u8,
    limits: &PdfLimits,
) -> Result<Encoded, Reject> {
    // The filter is carried; a stream that will not inflate is a broken file,
    // and saying "this filter is not carried" for it is a wrong reason for a
    // real loss. Bounded inflate (AUD-13): refuse before the allocation.
    let samples = crate::document::bounded_decompress(stream, limits.max_image_bytes).map_err(
        |reject| match (&reject, filtered) {
            (Reject::TooLarge, _) => Reject::TooLarge,
            (_, true) => Reject::Broken("flate samples could not be inflated"),
            (_, false) => Reject::Broken("samples are not a decodable stream"),
        },
    )?;
    let samples = if expand_one_bit {
        expand_gray1_to_eight(&samples, width, height, limits)?
    } else {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(components as usize);
        if samples.len() < expected {
            return Err(Reject::Broken("samples are truncated"));
        }
        if samples.len() > limits.max_image_bytes {
            return Err(Reject::TooLarge);
        }
        samples
    };
    Ok(Encoded::Raw {
        width,
        height,
        samples: Arc::new(samples),
        components,
        alpha: None,
    })
}

/// Owned `/DecodeParms` fields for `/CCITTFaxDecode` (ISO 32000-1 Table 11).
///
/// The bools are the PDF dictionary flags themselves (`EndOfBlock`,
/// `EndOfLine`, `EncodedByteAlign`, `BlackIs1`) — collapsing them into enums
/// would invent a state machine the file does not have.
#[allow(clippy::struct_excessive_bools)]
struct CcittParms {
    k: i32,
    columns: Option<u32>,
    rows: Option<u32>,
    end_of_block: bool,
    end_of_line: bool,
    rows_are_byte_aligned: bool,
    invert_black: bool,
}

impl Default for CcittParms {
    fn default() -> Self {
        Self {
            k: 0,
            columns: None,
            rows: None,
            end_of_block: true,
            end_of_line: false,
            rows_are_byte_aligned: false,
            invert_black: false,
        }
    }
}

fn ccitt_parms(dictionary: &lopdf::Dictionary, resolve: Resolver<'_>) -> CcittParms {
    let Ok(parms_obj) = dictionary.get(b"DecodeParms") else {
        return CcittParms::default();
    };
    let Some(resolved) = resolve.get(parms_obj) else {
        return CcittParms::default();
    };
    let dict = match resolved {
        Object::Dictionary(dict) => dict,
        Object::Array(items) => match items
            .first()
            .and_then(|item| resolve.get(item))
            .and_then(|value| value.as_dict().ok())
        {
            Some(dict) => dict,
            None => return CcittParms::default(),
        },
        _ => return CcittParms::default(),
    };
    CcittParms {
        k: number(dict, b"K").unwrap_or(0.0) as i32,
        columns: number(dict, b"Columns").map(|value| value as u32),
        rows: number(dict, b"Rows").map(|value| value as u32),
        end_of_block: dict
            .get(b"EndOfBlock")
            .ok()
            .and_then(|value| value.as_bool().ok())
            .unwrap_or(true),
        end_of_line: dict
            .get(b"EndOfLine")
            .ok()
            .and_then(|value| value.as_bool().ok())
            .unwrap_or(false),
        rows_are_byte_aligned: dict
            .get(b"EncodedByteAlign")
            .ok()
            .and_then(|value| value.as_bool().ok())
            .unwrap_or(false),
        invert_black: dict
            .get(b"BlackIs1")
            .ok()
            .and_then(|value| value.as_bool().ok())
            .unwrap_or(false),
    }
}

/// Packed 1-bit DeviceGray → 8-bit luma (`0`/`255`), MSB first, rows byte-aligned.
pub(crate) fn expand_gray1_to_eight(
    packed: &[u8],
    width: u32,
    height: u32,
    limits: &PdfLimits,
) -> Result<Vec<u8>, Reject> {
    let row_bytes = (width as usize).div_ceil(8);
    let needed = row_bytes.saturating_mul(height as usize);
    if packed.len() < needed {
        return Err(Reject::Broken("samples are truncated"));
    }
    let out_len = (width as usize).saturating_mul(height as usize);
    if out_len > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }
    let mut out = Vec::with_capacity(out_len);
    for row in 0..height as usize {
        let row_start = row * row_bytes;
        for x in 0..width as usize {
            let byte = packed[row_start + x / 8];
            let bit = (byte >> (7 - (x % 8))) & 1;
            out.push(if bit == 0 { 0 } else { 255 });
        }
    }
    Ok(out)
}

/// Collects 8-bit gray pixels from a CCITT decode.
struct CcittLuma8 {
    output: Vec<u8>,
    decoded_rows: u32,
}

impl hayro_ccitt::Decoder for CcittLuma8 {
    fn push_pixels(&mut self, white: bool, count: u32) {
        let byte = if white { 0xFF } else { 0x00 };
        self.output
            .extend(std::iter::repeat_n(byte, count as usize));
    }

    fn next_line(&mut self) {
        self.decoded_rows += 1;
    }
}

/// Collects 8-bit gray pixels from a JBIG2 decode.
///
/// JBIG2: black = 1; PDF DeviceGray: 0 = black, 1 = white — invert on emit.
struct Jbig2Luma8 {
    output: Vec<u8>,
}

impl hayro_jbig2::Decoder for Jbig2Luma8 {
    fn push_pixel(&mut self, black: bool) {
        self.output.push(if black { 0x00 } else { 0xFF });
    }

    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        let byte = if black { 0x00 } else { 0xFF };
        self.output
            .extend(std::iter::repeat_n(byte, chunk_count as usize * 8));
    }

    fn next_line(&mut self) {}
}

fn ccitt_encoding(k: i32) -> hayro_ccitt::EncodingMode {
    match k {
        ..=-1 => hayro_ccitt::EncodingMode::Group4,
        0 => hayro_ccitt::EncodingMode::Group3_1D,
        _ => hayro_ccitt::EncodingMode::Group3_2D { k: k as u32 },
    }
}

/// `/CCITTFaxDecode` → 8-bit gray ([`Encoded::Raw`]).
fn decode_ccitt_bytes(
    data: &[u8],
    dictionary: &lopdf::Dictionary,
    width: u32,
    height: u32,
    resolve: Resolver<'_>,
    limits: &PdfLimits,
) -> Result<Encoded, Reject> {
    let need = (width as usize).saturating_mul(height as usize);
    if need > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }
    let parms = ccitt_parms(dictionary, resolve);
    let settings = hayro_ccitt::DecodeSettings {
        columns: parms.columns.unwrap_or(width),
        rows: parms.rows.unwrap_or(height),
        end_of_block: parms.end_of_block,
        end_of_line: parms.end_of_line,
        rows_are_byte_aligned: parms.rows_are_byte_aligned,
        encoding: ccitt_encoding(parms.k),
        invert_black: parms.invert_black,
    };

    let mut decoder = CcittLuma8 {
        output: Vec::with_capacity(need),
        decoded_rows: 0,
    };
    let mut context = hayro_ccitt::DecoderContext::new(settings);
    let result = hayro_ccitt::decode(data, &mut decoder, &mut context);
    // Hayro is lenient: a truncated stream may still yield rows. Zero rows is
    // the only case that means "this is not a CCITT image we can read".
    if result.is_err() && decoder.decoded_rows == 0 {
        return Err(Reject::Broken("CCITT fax stream could not be decoded"));
    }
    if decoder.output.len() > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }
    // Prefer dictionary Width×Height when the fax columns differ (common when
    // `/Columns` is omitted and the fax default 1728 was not what we used).
    let samples = if decoder.output.len() >= need {
        decoder.output.truncate(need);
        decoder.output
    } else if !decoder.output.is_empty() {
        // Pad a truncated decode rather than refuse a mostly-readable scan.
        decoder.output.resize(need, 0xFF);
        decoder.output
    } else {
        return Err(Reject::Broken("CCITT fax stream could not be decoded"));
    };
    Ok(Encoded::Raw {
        width,
        height,
        samples: Arc::new(samples),
        components: 1,
        alpha: None,
    })
}

/// `/JBIG2Decode` → 8-bit gray ([`Encoded::Raw`]).
fn decode_jbig2_bytes(
    data: &[u8],
    dictionary: &lopdf::Dictionary,
    document: &lopdf::Document,
    resolve: Resolver<'_>,
    limits: &PdfLimits,
) -> Result<Encoded, Reject> {
    let globals = jbig2_globals(dictionary, document, resolve, limits)?;
    let image = hayro_jbig2::Image::new_embedded(data, globals.as_deref())
        .map_err(|_| Reject::Broken("JBIG2 stream could not be decoded"))?;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(Reject::Broken("JBIG2 stream has empty dimensions"));
    }
    let need = (width as usize).saturating_mul(height as usize);
    if need > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }

    let mut decoder = Jbig2Luma8 {
        output: Vec::with_capacity(need),
    };
    image
        .decode(&mut decoder)
        .map_err(|_| Reject::Broken("JBIG2 stream could not be decoded"))?;
    if decoder.output.len() > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }
    Ok(Encoded::Raw {
        width,
        height,
        samples: Arc::new(decoder.output),
        components: 1,
        alpha: None,
    })
}

fn jbig2_globals(
    dictionary: &lopdf::Dictionary,
    _document: &lopdf::Document,
    resolve: Resolver<'_>,
    limits: &PdfLimits,
) -> Result<Option<Vec<u8>>, Reject> {
    let Ok(value) = dictionary.get(b"JBIG2Globals") else {
        return Ok(None);
    };
    let Some(object) = resolve.get(value) else {
        return Err(Reject::Broken("JBIG2Globals stream is missing"));
    };
    let Ok(stream) = object.as_stream() else {
        return Err(Reject::Broken("JBIG2Globals is not a stream"));
    };
    match crate::document::bounded_decompress(stream, limits.max_image_bytes) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(Reject::TooLarge) => Err(Reject::TooLarge),
        Err(_) => Err(Reject::Broken("JBIG2Globals stream could not be inflated")),
    }
}

/// `/JPXDecode` → 8-bit interleaved samples ([`Encoded::Raw`]).
///
/// Budget is checked from the codestream's own width/height/channels *before*
/// `decode`, so an absurd JPEG 2000 header cannot force a huge allocation past
/// `max_image_bytes`. A broken codestream is [`Reject::Broken`], not
/// [`Reject::UnsupportedFilter`] — the filter *is* carried.
///
/// Public to the crate so inline images (`BI`…`EI` with `/F /JPX`) share one
/// path with XObject streams.
pub(crate) fn decode_jpx_bytes(data: &[u8], limits: &PdfLimits) -> Result<Encoded, Reject> {
    let settings = hayro_jpeg2000::DecodeSettings {
        resolve_palette_indices: false,
        strict: false,
        target_resolution: None,
    };
    let image = hayro_jpeg2000::Image::new(data, &settings)
        .map_err(|_| Reject::Broken("JPEG 2000 stream could not be decoded"))?;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(Reject::Broken("JPEG 2000 stream has empty dimensions"));
    }
    let color_components = image.color_space().num_channels();
    if !matches!(color_components, 1 | 3 | 4) {
        return Err(Reject::UnsupportedLayout("JPX colour space"));
    }
    let has_alpha = image.has_alpha();
    let channels = u64::from(color_components) + u64::from(has_alpha);
    let need = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(channels);
    if need > limits.max_image_bytes as u64 {
        return Err(Reject::TooLarge);
    }
    let mut ctx = hayro_jpeg2000::DecoderContext::default();
    let decoded = image
        .decode(&mut ctx)
        .map_err(|_| Reject::Broken("JPEG 2000 stream could not be decoded"))?;
    let bitmap = decoded.data_u8();
    let (samples, alpha) = if has_alpha {
        let total = usize::from(color_components) + 1;
        let pixels = (width as usize).saturating_mul(height as usize);
        let expected = pixels.saturating_mul(total);
        if bitmap.len() < expected {
            return Err(Reject::Broken("JPEG 2000 samples are truncated"));
        }
        let mut color = Vec::with_capacity(pixels.saturating_mul(usize::from(color_components)));
        let mut mask = Vec::with_capacity(pixels);
        for sample in bitmap.chunks_exact(total) {
            let (a, rgb) = sample
                .split_last()
                .ok_or(Reject::Broken("JPEG 2000 samples are truncated"))?;
            mask.push(*a);
            color.extend_from_slice(rgb);
        }
        (color, Some(Arc::new(mask)))
    } else {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(usize::from(color_components));
        if bitmap.len() < expected {
            return Err(Reject::Broken("JPEG 2000 samples are truncated"));
        }
        (bitmap, None)
    };
    if samples.len() > limits.max_image_bytes {
        return Err(Reject::TooLarge);
    }
    Ok(Encoded::Raw {
        width,
        height,
        samples: Arc::new(samples),
        components: color_components,
        alpha,
    })
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
    use super::{decode_jpx_bytes, Encoded, Reject};
    use crate::PdfLimits;

    /// Lossless 2×2 RGB JP2 (Pillow/OpenJPEG), pixels:
    /// (255,0,0) (0,255,0) / (0,0,255) (255,255,255).
    const JPX_RGB_2X2: &[u8] = &[
        0, 0, 0, 12, 106, 80, 32, 32, 13, 10, 135, 10, 0, 0, 0, 20, 102, 116, 121, 112, 106, 112,
        50, 32, 0, 0, 0, 0, 106, 112, 50, 32, 0, 0, 0, 45, 106, 112, 50, 104, 0, 0, 0, 22, 105,
        104, 100, 114, 0, 0, 0, 2, 0, 0, 0, 2, 0, 3, 7, 7, 0, 0, 0, 0, 0, 15, 99, 111, 108, 114, 1,
        0, 0, 0, 0, 0, 16, 0, 0, 0, 153, 106, 112, 50, 99, 255, 79, 255, 81, 0, 47, 0, 0, 0, 0, 0,
        2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        3, 7, 1, 1, 7, 1, 1, 7, 1, 1, 255, 82, 0, 12, 0, 0, 0, 1, 0, 1, 4, 4, 0, 1, 255, 92, 0, 7,
        64, 64, 72, 72, 80, 255, 100, 0, 37, 0, 1, 67, 114, 101, 97, 116, 101, 100, 32, 98, 121,
        32, 79, 112, 101, 110, 74, 80, 69, 71, 32, 118, 101, 114, 115, 105, 111, 110, 32, 50, 46,
        53, 46, 52, 255, 144, 0, 10, 0, 0, 0, 0, 0, 30, 0, 1, 255, 147, 128, 128, 128, 147, 243, 2,
        0, 223, 207, 192, 4, 0, 167, 224, 2, 0, 255, 217,
    ];

    /// Lossless 3×1 gray JP2: samples 0, 128, 255.
    const JPX_GRAY_3X1: &[u8] = &[
        0, 0, 0, 12, 106, 80, 32, 32, 13, 10, 135, 10, 0, 0, 0, 20, 102, 116, 121, 112, 106, 112,
        50, 32, 0, 0, 0, 0, 106, 112, 50, 32, 0, 0, 0, 45, 106, 112, 50, 104, 0, 0, 0, 22, 105,
        104, 100, 114, 0, 0, 0, 1, 0, 0, 0, 3, 0, 1, 7, 7, 0, 0, 0, 0, 0, 15, 99, 111, 108, 114, 1,
        0, 0, 0, 0, 0, 17, 0, 0, 0, 135, 106, 112, 50, 99, 255, 79, 255, 81, 0, 41, 0, 0, 0, 0, 0,
        3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 7, 1, 1, 255, 82, 0, 12, 0, 0, 0, 1, 0, 0, 4, 4, 0, 1, 255, 92, 0, 4, 64, 64, 255, 100,
        0, 37, 0, 1, 67, 114, 101, 97, 116, 101, 100, 32, 98, 121, 32, 79, 112, 101, 110, 74, 80,
        69, 71, 32, 118, 101, 114, 115, 105, 111, 110, 32, 50, 46, 53, 46, 52, 255, 144, 0, 10, 0,
        0, 0, 0, 0, 21, 0, 1, 255, 147, 223, 128, 32, 7, 36, 97, 19, 255, 217,
    ];

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
    fn unknown_filter_rejection_owns_the_input_name() {
        let mut dictionary = lopdf::Dictionary::new();
        dictionary.set("Width", 1);
        dictionary.set("Height", 1);
        dictionary.set("BitsPerComponent", 8);
        dictionary.set("ColorSpace", lopdf::Object::Name(b"DeviceGray".to_vec()));
        dictionary.set("Filter", lopdf::Object::Name(b"UnknownCodec".to_vec()));
        let mut document = lopdf::Document::new();
        let id = document.add_object(lopdf::Stream::new(dictionary, vec![0]));
        let rejection =
            super::decode_from(id, &document, &PdfLimits::default()).expect_err("unknown codec");
        drop(document);
        assert_eq!(
            rejection,
            Reject::UnsupportedFilter("UnknownCodec".to_owned())
        );
        assert!(rejection.to_string().contains("UnknownCodec"));
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

    #[test]
    fn jpx_rgb_decodes_to_exact_lossless_samples() {
        let encoded = decode_jpx_bytes(JPX_RGB_2X2, &PdfLimits::default()).expect("jpx rgb");
        assert_eq!(encoded.pixel_size(), (2, 2));
        let (samples, components) = encoded.raw_samples().expect("raw");
        assert_eq!(components, 3);
        assert_eq!(samples, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        assert!(encoded.alpha_channel().is_none());
        let png = encoded.to_png().expect("png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn jpx_gray_decodes_to_exact_lossless_samples() {
        let encoded = decode_jpx_bytes(JPX_GRAY_3X1, &PdfLimits::default()).expect("jpx gray");
        assert_eq!(encoded.pixel_size(), (3, 1));
        assert_eq!(encoded.raw_samples(), Some((&[0_u8, 128, 255][..], 1)));
    }

    #[test]
    fn a_broken_jpx_stream_is_broken_not_unsupported() {
        let err =
            decode_jpx_bytes(b"not a jpeg2000 stream", &PdfLimits::default()).expect_err("garbage");
        assert!(
            matches!(err, Reject::Broken(_)),
            "expected Broken, got {err}"
        );
        assert!(
            !matches!(err, Reject::UnsupportedFilter(_)),
            "JPX is carried; must not look like a missing codec: {err}"
        );
    }

    #[test]
    fn jpx_respects_the_image_byte_budget_before_decode() {
        // 2×2×3 = 12 bytes of samples; an 11-byte ceiling must refuse.
        let limits = PdfLimits {
            max_image_bytes: 11,
            ..PdfLimits::default()
        };
        let err = decode_jpx_bytes(JPX_RGB_2X2, &limits).expect_err("budget");
        assert_eq!(err, Reject::TooLarge);
    }

    #[test]
    fn one_bit_gray_expands_msb_first_to_luma8() {
        // 8×2: WWWWBBBB / BBBBWWWW
        let packed = [0b1111_0000_u8, 0b0000_1111];
        let samples =
            super::expand_gray1_to_eight(&packed, 8, 2, &PdfLimits::default()).expect("expand");
        assert_eq!(
            samples,
            [
                255, 255, 255, 255, 0, 0, 0, 0, //
                0, 0, 0, 0, 255, 255, 255, 255
            ]
        );
    }
}
