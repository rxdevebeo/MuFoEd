use crate::{HtmlOptions, HtmlResolver};
use base64::Engine as _;
use strict_ooxml_core::{error::SourceLocation, part::PartId};
use strict_ooxml_wml::model::{
    BlipRef, DocPr, Drawing, DrawingKind, Emu, Extent, Graphic, InlineDrawing, MediaKind, Picture,
};

pub(crate) fn load(
    src: &str,
    options: &HtmlOptions,
    resolver: Option<&dyn HtmlResolver>,
    remaining: usize,
) -> Result<Vec<u8>, String> {
    let bytes = if let Some(data) = src.strip_prefix("data:") {
        let (header, data) = data.split_once(',').ok_or("invalid data URL")?;
        if !header.to_ascii_lowercase().starts_with("image/") {
            return Err("data URL is not an image".into());
        }
        if header.ends_with(";base64") {
            if data.len() > remaining.saturating_mul(4) / 3 + 8 {
                return Err("image byte budget exceeded".into());
            }
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| e.to_string())?
        } else {
            let mut out = Vec::new();
            let mut i = 0;
            while i < data.len() {
                if out.len() > remaining {
                    return Err("image byte budget exceeded".into());
                }
                if data.as_bytes()[i] == b'%' {
                    let v = data.get(i + 1..i + 3).ok_or("invalid percent escape")?;
                    out.push(u8::from_str_radix(v, 16).map_err(|e| e.to_string())?);
                    i += 3;
                } else {
                    out.push(data.as_bytes()[i]);
                    i += 1;
                }
            }
            out
        }
    } else {
        let reference = url::Url::parse(src).or_else(|_| {
            options
                .base_url
                .as_ref()
                .ok_or(url::ParseError::RelativeUrlWithoutBase)
                .and_then(|base| url::Url::parse(base)?.join(src))
        });
        let source = reference.as_ref().map_or(src, url::Url::as_str);
        resolver
            .ok_or("external image has no resolver")?
            .resolve(source, remaining)?
    };
    if bytes.len() > remaining {
        return Err("image byte budget exceeded".into());
    }
    Ok(bytes)
}

pub(crate) fn format(bytes: &[u8]) -> Option<(MediaKind, &'static str, &'static str, u32, u32)> {
    let four = |at: usize| -> Option<[u8; 4]> { bytes.get(at..at + 4)?.try_into().ok() };
    let two = |at: usize| -> Option<[u8; 2]> { bytes.get(at..at + 2)?.try_into().ok() };
    let (kind, mime, ext, w, h) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (
            MediaKind::Png,
            "image/png",
            "png",
            u32::from_be_bytes(four(16)?),
            u32::from_be_bytes(four(20)?),
        )
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        (
            MediaKind::Gif,
            "image/gif",
            "gif",
            u32::from(u16::from_le_bytes(two(6)?)),
            u32::from(u16::from_le_bytes(two(8)?)),
        )
    } else if bytes.starts_with(b"BM") {
        (
            MediaKind::Bmp,
            "image/bmp",
            "bmp",
            i32::from_le_bytes(four(18)?).unsigned_abs(),
            i32::from_le_bytes(four(22)?).unsigned_abs(),
        )
    } else if bytes.starts_with(&[255, 216, 255]) {
        let mut at = 2;
        let mut size = None;
        while at + 4 <= bytes.len() {
            if bytes[at] != 255 {
                break;
            }
            let marker = bytes[at + 1];
            if marker == 255 {
                at += 1;
                continue;
            }
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
                at += 2;
                continue;
            }
            let len = usize::from(u16::from_be_bytes(
                bytes.get(at + 2..at + 4)?.try_into().ok()?,
            ));
            if len < 2 || at + 2 + len > bytes.len() {
                break;
            }
            if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
                size = Some((
                    u32::from(u16::from_be_bytes(
                        bytes.get(at + 7..at + 9)?.try_into().ok()?,
                    )),
                    u32::from(u16::from_be_bytes(
                        bytes.get(at + 5..at + 7)?.try_into().ok()?,
                    )),
                ));
                break;
            }
            at += 2 + len;
        }
        let (w, h) = size?;
        (MediaKind::Jpeg, "image/jpeg", "jpeg", w, h)
    } else {
        return None;
    };
    (w > 0 && h > 0).then_some((kind, mime, ext, w, h))
}

pub(crate) fn drawing(part: PartId, width: i64, height: i64, id: u32, alt: &str) -> Drawing {
    let extent = Extent {
        cx: Emu(width),
        cy: Emu(height),
    };
    let location = SourceLocation::unknown();
    Drawing {
        kind: DrawingKind::Inline(InlineDrawing {
            extent: Some(extent),
            effect_extent: None,
            doc_pr: Some(DocPr {
                hidden: None,
                id: Some(id),
                name: Some(format!("HTML image {id}").into()),
                descr: Some(alt.into()),
                title: None,
            }),
            dist_top: Some(0),
            dist_bottom: Some(0),
            dist_left: Some(0),
            dist_right: Some(0),
            graphic_uri: Some("http://purl.oclc.org/ooxml/drawingml/picture".into()),
            graphic: Box::new(Graphic::Picture(Picture {
                name: Some(alt.into()),
                descr: Some(alt.into()),
                nv_id: Some(id),
                bw_mode: None,
                blip: Some(BlipRef {
                    embed: None,
                    link: None,
                    resolved: Some(part),
                    cstate: None,
                    location: location.clone(),
                }),
                extent: Some(extent),
                src_rect: None,
                xfrm: None,
                markup: strict_ooxml_wml::model::drawing::PictureMarkup::default(),
            })),
            location: location.clone(),
        }),
        location,
    }
}
