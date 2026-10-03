//! Inline image placement and media resolution (`STAGE-4-TASK.md` §5.7).

use std::fmt::Write as _;

use strict_ooxml_core::part::PartId;

use crate::layout::{ImageItem, LayoutContext};
use crate::units::emu_to_px;
use crate::MediaMode;

/// Default image size in px when no extent is declared.
const DEFAULT_IMAGE_PX: f64 = 96.0;

/// Builds an [`ImageItem`] for an inline drawing.
///
/// A supported inline raster picture resolves to its media part. Any other
/// *picture-shaped* inline drawing with a declared extent — a chart, a diagram,
/// EMF/WMF, a missing blip — is reserved as a placeholder of that size: Stage 4
/// cannot rasterize it, but reserving the extent keeps pagination aligned with
/// the producer (S4F.6). An inline *shape or group* is not an image at all: it
/// is left to `crate::paint::graphics::inline_items`, which draws its geometry
/// (STAGE-5C-REWORK-1 D3). `wp:anchor` (floating) drawings are out of scope and
/// yield `None`.
pub(crate) fn layout_inline_image(
    ctx: &LayoutContext<'_>,
    drawing: &strict_ooxml_wml::model::Drawing,
    x: f64,
    y: f64,
) -> Option<ImageItem> {
    use strict_ooxml_wml::model::drawing::Graphic;
    let strict_ooxml_wml::model::DrawingKind::Inline(inline) = &drawing.kind else {
        return None;
    };
    match inline.graphic.as_ref() {
        Graphic::Shape(_) | Graphic::Group(_) | Graphic::Other => return None,
        Graphic::None | Graphic::Picture(_) | Graphic::Chart(_) | Graphic::Diagram(_) => {}
    }
    let picture = inline.picture();
    let extent = inline.extent.or_else(|| picture.and_then(|p| p.extent));
    let part = picture
        .and_then(|picture| picture.blip.as_ref())
        .and_then(|blip| blip.resolved.as_ref());
    if part.is_none() && extent.is_none() {
        return None;
    }
    let extent = extent.unwrap_or_default();
    let mut w = emu_to_px(extent.cx.value(), ctx.options.scale);
    let mut h = emu_to_px(extent.cy.value(), ctx.options.scale);
    if w <= 0.0 {
        w = DEFAULT_IMAGE_PX;
    }
    if h <= 0.0 {
        h = DEFAULT_IMAGE_PX;
    }

    let alt = inline
        .doc_pr
        .as_ref()
        .and_then(|doc_pr| doc_pr.descr.clone().or(doc_pr.name.clone()))
        .map_or_else(String::new, |value| value.to_string());

    Some(ImageItem {
        x,
        y,
        w,
        h,
        href: part.and_then(|part| media_href(ctx, part)),
        part: part.cloned(),
        alt,
        transform: None,
    })
}

/// Resolves the `href`/data URI for a media part according to the media mode.
pub(crate) fn media_href(ctx: &LayoutContext<'_>, part: &PartId) -> Option<String> {
    match ctx.media_mode {
        MediaMode::None => None,
        MediaMode::ExternalFiles => Some(media_file_name(part)),
        MediaMode::EmbedDataUri => {
            let media = ctx.media?;
            let bytes = media.read_media(part).ok()?;
            let content_type = ctx
                .document
                .media
                .get(part)
                .and_then(|item| item.content_type.clone())
                .map_or(String::from("application/octet-stream"), |ct| {
                    ct.to_string()
                });
            Some(format!(
                "data:{content_type};base64,{}",
                base64_encode(&bytes)
            ))
        }
    }
}

/// Returns the file name of a media part (`/word/media/image1.png` → `image1.png`).
#[must_use]
pub fn media_file_name(part: &PartId) -> String {
    part.as_str()
        .rsplit('/')
        .next()
        .unwrap_or("media.bin")
        .to_owned()
}

/// Writes an `<image>` (or a placeholder) for a placed image.
pub(crate) fn image_svg(out: &mut String, item: &ImageItem) {
    let title = if item.alt.is_empty() {
        String::new()
    } else {
        format!("<title>{}</title>", crate::paint::escape_text(&item.alt))
    };
    let transform = item.transform.as_deref().map_or_else(String::new, |value| {
        format!(" transform=\"{}\"", crate::paint::escape_attr(value))
    });
    if let Some(href) = &item.href {
        let _ = writeln!(
            out,
            "  <image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"{} xlink:href=\"{}\">{}</image>",
            crate::paint::coord(item.x),
            crate::paint::coord(item.y),
            crate::paint::coord(item.w.max(0.0)),
            crate::paint::coord(item.h.max(0.0)),
            transform,
            crate::paint::escape_attr(href),
            title,
        );
    } else {
        let _ = writeln!(
            out,
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#f2f2f2\" stroke=\"#999999\" stroke-width=\"1\"{}>{}</rect>",
            crate::paint::coord(item.x),
            crate::paint::coord(item.y),
            crate::paint::coord(item.w.max(0.0)),
            crate::paint::coord(item.h.max(0.0)),
            transform,
            title,
        );
    }
}

/// Encodes bytes as standard Base64 with padding (deterministic).
#[must_use]
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        // `& 0x3F` is six bits, so every index below is 0..=63 into a 64-character
        // alphabet: the `usize` is the index type, not a conversion of a number
        // this crate computed from input arithmetic (AUD-09's G-2 audit).
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64_encode, media_file_name};
    use strict_ooxml_core::part::PartId;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn media_file_name_extracts_basename() {
        assert_eq!(
            media_file_name(&PartId::new("/word/media/image1.png")),
            "image1.png"
        );
        assert_eq!(media_file_name(&PartId::new("image2.jpeg")), "image2.jpeg");
    }
}
