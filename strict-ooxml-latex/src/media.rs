//! Bounded image collection for `\includegraphics`; no rasterization or decode.
//!
//! Container signatures and intrinsic sizes are read directly from each format's
//! header. Anything the importer cannot measure falls back to a one-inch box and
//! is reported.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::drawing::{
    BlipRef, DocPr, Drawing, DrawingKind, Extent, Graphic, InlineDrawing, MediaItem, MediaKind,
    Picture, PictureMarkup, SrcRect, Xfrm,
};
use strict_ooxml_wml::model::Emu;

use crate::LatexError;

/// One English Metric Unit is 1/914400 inch; 96 dpi gives 9525 EMU per pixel.
const EMU_PER_PIXEL: i64 = 9525;
/// One typographic point is 12700 EMU.
const EMU_PER_POINT: i64 = 12700;
/// The fallback extent (one inch) when a container's size is unknown.
const FALLBACK_EMU: i64 = EMU_PER_PIXEL * 96;
/// `a:srcRect` stores thousandths of a percent (`100000` = the whole edge).
const SRCRECT_PER_UNIT: f64 = 100_000.0;

/// A recognized image container and its intrinsic size in pixels.
pub(crate) struct ImageFormat {
    /// Container kind.
    pub kind: MediaKind,
    /// File extension for the media part name.
    pub extension: &'static str,
    /// Content type of the media part.
    pub content_type: &'static str,
    /// Intrinsic pixel size, when the header could be read.
    pub size: Option<(i64, i64)>,
}

/// Sizing and transform options from `\includegraphics[...]`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ImageOptions {
    /// Requested width in EMU.
    pub width_emu: Option<i64>,
    /// Requested height in EMU.
    pub height_emu: Option<i64>,
    /// Uniform scale factor.
    pub scale: Option<f64>,
    /// Rotation in degrees.
    pub angle: Option<f64>,
    /// Crop insets in points, in `left bottom right top` order.
    pub trim: Option<(f64, f64, f64, f64)>,
}

/// Recognizes a supported container and measures its intrinsic size.
#[must_use]
pub(crate) fn sniff(name: &str, bytes: &[u8]) -> Option<ImageFormat> {
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");
    let ext_is = |value: &str| extension.eq_ignore_ascii_case(value);

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || ext_is("png") {
        return Some(ImageFormat {
            kind: MediaKind::Png,
            extension: "png",
            content_type: "image/png",
            size: png_size(bytes),
        });
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || ext_is("jpg") || ext_is("jpeg") {
        return Some(ImageFormat {
            kind: MediaKind::Jpeg,
            extension: "jpg",
            content_type: "image/jpeg",
            size: jpeg_size(bytes),
        });
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") || ext_is("gif") {
        return Some(ImageFormat {
            kind: MediaKind::Gif,
            extension: "gif",
            content_type: "image/gif",
            size: gif_size(bytes),
        });
    }
    if bytes.starts_with(b"BM") || ext_is("bmp") || ext_is("dib") {
        return Some(ImageFormat {
            kind: MediaKind::Bmp,
            extension: "bmp",
            content_type: "image/bmp",
            size: bmp_size(bytes),
        });
    }
    if bytes.starts_with(b"II\x2A\x00")
        || bytes.starts_with(b"MM\x00\x2A")
        || ext_is("tif")
        || ext_is("tiff")
    {
        return Some(ImageFormat {
            kind: MediaKind::Tiff,
            extension: "tiff",
            content_type: "image/tiff",
            size: tiff_size(bytes),
        });
    }
    if is_emf(bytes) || ext_is("emf") {
        return Some(ImageFormat {
            kind: MediaKind::Emf,
            extension: "emf",
            content_type: "image/x-emf",
            size: emf_size(bytes),
        });
    }
    if is_wmf(bytes) || ext_is("wmf") {
        return Some(ImageFormat {
            kind: MediaKind::Wmf,
            extension: "wmf",
            content_type: "image/x-wmf",
            size: wmf_size(bytes),
        });
    }
    None
}

fn is_emf(bytes: &[u8]) -> bool {
    bytes.len() >= 88 && bytes.starts_with(&[1, 0, 0, 0]) && bytes.get(40..44) == Some(b" EMF")
}

fn is_wmf(bytes: &[u8]) -> bool {
    let offset = if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        22
    } else {
        0
    };
    bytes.get(offset..).is_some_and(|body| {
        body.len() >= 18
            && (body.starts_with(&[1, 0, 9, 0]) || body.starts_with(&[2, 0, 9, 0]))
            && matches!(body.get(4..6), Some([0, 1 | 3]))
    })
}

fn png_size(bytes: &[u8]) -> Option<(i64, i64)> {
    let header = bytes.get(16..24)?;
    let width = u32::from_be_bytes(header.get(0..4)?.try_into().ok()?);
    let height = u32::from_be_bytes(header.get(4..8)?.try_into().ok()?);
    Some((i64::from(width), i64::from(height)))
}

fn gif_size(bytes: &[u8]) -> Option<(i64, i64)> {
    let header = bytes.get(6..10)?;
    let width = u16::from_le_bytes(header.get(0..2)?.try_into().ok()?);
    let height = u16::from_le_bytes(header.get(2..4)?.try_into().ok()?);
    Some((i64::from(width), i64::from(height)))
}

fn bmp_size(bytes: &[u8]) -> Option<(i64, i64)> {
    let width = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
    let height = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
    Some((
        i64::from(width.unsigned_abs()),
        i64::from(height.unsigned_abs()),
    ))
}

fn jpeg_size(bytes: &[u8]) -> Option<(i64, i64)> {
    let mut index = 2usize;
    while index + 3 < bytes.len() {
        if bytes.get(index) != Some(&0xFF) {
            index += 1;
            continue;
        }
        let marker = *bytes.get(index + 1)?;
        if marker == 0xFF || marker == 0x00 || (0xD0..=0xD9).contains(&marker) {
            index += 2;
            continue;
        }
        let length = u16::from_be_bytes(bytes.get(index + 2..index + 4)?.try_into().ok()?);
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            let height = u16::from_be_bytes(bytes.get(index + 5..index + 7)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(index + 7..index + 9)?.try_into().ok()?);
            return Some((i64::from(width), i64::from(height)));
        }
        index += 2 + usize::from(length);
    }
    None
}

/// Reads the first IFD's `ImageWidth` (0x0100) and `ImageLength` (0x0101).
fn tiff_size(bytes: &[u8]) -> Option<(i64, i64)> {
    let little = bytes.starts_with(b"II\x2A\x00");
    let read_u16 = |offset: usize| -> Option<u16> {
        let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        })
    };
    let read_u32 = |offset: usize| -> Option<u32> {
        let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    };
    let ifd = usize::try_from(read_u32(4)?).ok()?;
    let count = usize::from(read_u16(ifd)?);
    let mut width = None;
    let mut height = None;
    for entry in 0..count {
        let base = ifd + 2 + entry * 12;
        let tag = read_u16(base)?;
        let value = read_u32(base + 8)?;
        match tag {
            0x0100 => width = Some(i64::from(value)),
            0x0101 => height = Some(i64::from(value)),
            _ => {}
        }
    }
    Some((width?, height?))
}

fn emf_size(bytes: &[u8]) -> Option<(i64, i64)> {
    // `rclFrame` is in hundredths of a millimetre. Widen to `i64` before the
    // subtraction: a crafted header can pair `i32::MIN` with `i32::MAX`, and the
    // difference overflows `i32`.
    let left = i64::from(i32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?));
    let top = i64::from(i32::from_le_bytes(bytes.get(28..32)?.try_into().ok()?));
    let right = i64::from(i32::from_le_bytes(bytes.get(32..36)?.try_into().ok()?));
    let bottom = i64::from(i32::from_le_bytes(bytes.get(36..40)?.try_into().ok()?));
    let width_mm = (right - left) as f64 / 100.0;
    let height_mm = (bottom - top) as f64 / 100.0;
    Some((mm_to_pixels(width_mm), mm_to_pixels(height_mm)))
}

fn wmf_size(bytes: &[u8]) -> Option<(i64, i64)> {
    // The placeable header's bounding box is in twips.
    if !bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        return None;
    }
    let left = i16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    let top = i16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
    let right = i16::from_le_bytes(bytes.get(10..12)?.try_into().ok()?);
    let bottom = i16::from_le_bytes(bytes.get(12..14)?.try_into().ok()?);
    let width = i64::from(i32::from(right) - i32::from(left));
    let height = i64::from(i32::from(bottom) - i32::from(top));
    Some((
        (width * EMU_PER_PIXEL / 1440).max(1),
        (height * EMU_PER_PIXEL / 1440).max(1),
    ))
}

fn mm_to_pixels(mm: f64) -> i64 {
    ((mm / 25.4) * 96.0).round().max(1.0) as i64
}

/// A built image: the drawing node, its media index entry and its bytes.
pub(crate) struct BuiltImage {
    /// Inline drawing to place in the run.
    pub drawing: Drawing,
    /// Index entry naming the media part and content type.
    pub item: MediaItem,
    /// The payload bytes.
    pub bytes: Vec<u8>,
}

/// Builds the drawing for one image, bounding its size against the limits.
pub(crate) fn build(
    index: usize,
    name: &str,
    bytes: Vec<u8>,
    total: usize,
    media_part_bytes: usize,
    media_total_bytes: usize,
    options: ImageOptions,
) -> Result<BuiltImage, LatexError> {
    let format = sniff(name, &bytes).ok_or(LatexError::UnsupportedImage {
        name: name.to_owned(),
    })?;
    let observed = total.saturating_add(bytes.len());
    if bytes.len() > media_part_bytes {
        return Err(LatexError::LimitExceeded {
            limit: "media_part_bytes",
            observed: bytes.len(),
        });
    }
    if observed > media_total_bytes {
        return Err(LatexError::LimitExceeded {
            limit: "media_total_bytes",
            observed,
        });
    }

    let (intrinsic_cx, intrinsic_cy) =
        format.size.map_or((FALLBACK_EMU, FALLBACK_EMU), |(w, h)| {
            (
                w.max(1).saturating_mul(EMU_PER_PIXEL),
                h.max(1).saturating_mul(EMU_PER_PIXEL),
            )
        });
    let scale = options.scale.filter(|s| *s > 0.0).unwrap_or(1.0);
    let scaled_cx = (intrinsic_cx as f64 * scale).round() as i64;
    let scaled_cy = (intrinsic_cy as f64 * scale).round() as i64;
    let (cx, cy) = match (options.width_emu, options.height_emu) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (
            width,
            width.saturating_mul(scaled_cy).max(1) / scaled_cx.max(1),
        ),
        (None, Some(height)) => (
            height.saturating_mul(scaled_cx).max(1) / scaled_cy.max(1),
            height,
        ),
        (None, None) => (scaled_cx, scaled_cy),
    };
    let extent = Extent {
        cx: Emu(cx.max(1)),
        cy: Emu(cy.max(1)),
    };

    let xfrm = options.angle.filter(|a| *a != 0.0).map(|angle| Xfrm {
        offset: None,
        rot: Some((angle * 60_000.0).round() as i32),
        flip_h: false,
        flip_v: false,
    });

    let src_rect = options
        .trim
        .filter(|(l, b, r, t)| *l != 0.0 || *b != 0.0 || *r != 0.0 || *t != 0.0)
        .map(|(left, bottom, right, top)| {
            let edge = |points: f64, basis: i64| -> i32 {
                let inset =
                    (points * EMU_PER_POINT as f64) / basis.max(1) as f64 * SRCRECT_PER_UNIT;
                inset.round().clamp(0.0, SRCRECT_PER_UNIT) as i32
            };
            SrcRect {
                left: edge(left, extent.cx.value()),
                top: edge(top, extent.cy.value()),
                right: edge(right, extent.cx.value()),
                bottom: edge(bottom, extent.cy.value()),
            }
        });

    let part = PartId::new(format!(
        "/word/media/latex-image-{index}.{}",
        format.extension
    ));
    let location = SourceLocation::default();
    let display_name = Arc::<str>::from(format!("LaTeX image {index}"));
    let drawing = Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            doc_pr: Some(DocPr {
                hidden: None,
                id: u32::try_from(index).ok(),
                name: Some(display_name.clone()),
                descr: None,
                title: None,
            }),
            dist_top: None,
            dist_bottom: None,
            dist_left: None,
            dist_right: None,
            graphic_uri: None,
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some(display_name),
                descr: None,
                nv_id: None,
                bw_mode: None,
                blip: Some(BlipRef {
                    embed: None,
                    link: None,
                    resolved: Some(part.clone()),
                    cstate: None,
                    location: location.clone(),
                }),
                extent: Some(extent),
                src_rect,
                xfrm,
                markup: PictureMarkup::default(),
            })),
            location: location.clone(),
        }),
        location,
    };
    Ok(BuiltImage {
        drawing,
        item: MediaItem {
            part,
            content_type: Some(Arc::from(format.content_type)),
            kind: format.kind,
        },
        bytes,
    })
}
