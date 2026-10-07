//! Bounded embedded image collection; no rasterization or external fetching.
use crate::{RtfError, RtfLimits};
use std::sync::Arc;
use strict_ooxml_core::{error::SourceLocation, part::PartId};
use strict_ooxml_wml::model::drawing::{
    BlipRef, DocPr, Drawing, DrawingKind, Extent, Graphic, InlineDrawing, MediaItem, MediaKind,
    Picture,
};
use strict_ooxml_wml::model::Emu;

#[derive(Default)]
pub(super) struct PictureBuilder {
    pub kind: Option<MediaKind>,
    pub bytes: Vec<u8>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub width_goal: Option<i32>,
    pub height_goal: Option<i32>,
    pub scale_x: Option<i32>,
    pub scale_y: Option<i32>,
    nibble: Option<u8>,
}
impl PictureBuilder {
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        limits: &RtfLimits,
        total: usize,
        offset: usize,
    ) -> Result<(), RtfError> {
        if self.nibble.is_some() {
            return Err(RtfError::Malformed {
                offset,
                reason: "binary picture starts after odd hex nibble",
            });
        }
        let size = self.bytes.len() + bytes.len();
        for (name, observed, max) in [
            ("media_part_bytes", size, limits.media_part_bytes),
            ("media_total_bytes", total + size, limits.media_total_bytes),
        ] {
            if observed > max {
                return Err(RtfError::LimitExceeded {
                    offset,
                    limit: name,
                    observed,
                });
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub(super) fn hex(
        &mut self,
        byte: u8,
        limits: &RtfLimits,
        total: usize,
        offset: usize,
    ) -> Result<(), RtfError> {
        if byte.is_ascii_whitespace() {
            return Ok(());
        }
        let value = (byte as char)
            .to_digit(16)
            .and_then(|n| u8::try_from(n).ok())
            .ok_or(RtfError::Malformed {
                offset,
                reason: "invalid picture hex data",
            })?;
        if let Some(high) = self.nibble.take() {
            self.push(&[high * 16 + value], limits, total, offset)?;
        } else {
            self.nibble = Some(value);
        }
        Ok(())
    }
    pub(super) fn build(
        self,
        index: usize,
        offset: usize,
    ) -> Result<(Drawing, MediaItem, Vec<u8>), RtfError> {
        if self.nibble.is_some() {
            return Err(RtfError::Malformed {
                offset,
                reason: "odd picture hex data",
            });
        }
        let (kind, extension, content_type) = match self.kind {
            Some(MediaKind::Png) => (MediaKind::Png, "png", "image/png"),
            Some(MediaKind::Jpeg) => (MediaKind::Jpeg, "jpg", "image/jpeg"),
            Some(MediaKind::Wmf) => (MediaKind::Wmf, "wmf", "image/x-wmf"),
            Some(MediaKind::Emf) => (MediaKind::Emf, "emf", "image/x-emf"),
            _ => {
                return Err(RtfError::Malformed {
                    offset,
                    reason: "missing supported picture format",
                })
            }
        };
        if !signature_matches(kind, &self.bytes) {
            return Err(RtfError::Malformed {
                offset,
                reason: "picture signature does not match declared format",
            });
        }
        let dimension =
            |goal: Option<i32>, pixels: Option<i32>, scale: Option<i32>| -> Result<Emu, RtfError> {
                let base = if let Some(goal) = goal {
                    i64::from(goal) * 635
                } else {
                    i64::from(pixels.ok_or(RtfError::Malformed {
                        offset,
                        reason: "picture has no dimensions",
                    })?) * 9525
                };
                let scaled = base
                    .checked_mul(i64::from(scale.unwrap_or(100)))
                    .map(|v| v / 100)
                    .filter(|v| *v > 0)
                    .ok_or(RtfError::Malformed {
                        offset,
                        reason: "invalid picture extent",
                    })?;
                Ok(Emu(scaled))
            };
        let extent = Extent {
            cx: dimension(self.width_goal, self.width, self.scale_x)?,
            cy: dimension(self.height_goal, self.height, self.scale_y)?,
        };
        let part = PartId::new(format!("/word/media/rtf-image-{index}.{extension}"));
        let location = SourceLocation::unknown();
        let name = Arc::<str>::from(format!("RTF image {index}"));
        let drawing = Drawing {
            kind: DrawingKind::Inline(InlineDrawing {
                extent: Some(extent),
                effect_extent: None,
                doc_pr: Some(DocPr {
                    id: u32::try_from(index).ok(),
                    name: Some(name.clone()),
                    descr: None,
                    title: None,
                }),
                dist_top: None,
                dist_bottom: None,
                dist_left: None,
                dist_right: None,
                graphic_uri: None,
                graphic: Box::new(Graphic::Picture(Picture {
                    name: Some(name),
                    descr: None,
                    blip: Some(BlipRef {
                        embed: None,
                        link: None,
                        resolved: Some(part.clone()),
                        location: location.clone(),
                    }),
                    extent: Some(extent),
                    src_rect: None,
                    xfrm: None,
                })),
                location: location.clone(),
            }),
            location,
        };
        Ok((
            drawing,
            MediaItem {
                part,
                content_type: Some(Arc::from(content_type)),
                kind,
            },
            self.bytes,
        ))
    }
}

// Container signatures only; drawing records and raster validity belong to
// the decoder. Preserve all payload bytes, including trailing JPEG data.
fn signature_matches(kind: MediaKind, bytes: &[u8]) -> bool {
    match kind {
        MediaKind::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        MediaKind::Jpeg => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        MediaKind::Wmf => {
            let offset = if bytes.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) {
                22
            } else {
                0
            };
            bytes.get(offset..).is_some_and(|b| {
                b.len() >= 18
                    && (b.starts_with(&[1, 0, 9, 0]) || b.starts_with(&[2, 0, 9, 0]))
                    && matches!(b.get(4..6), Some([0, 1 | 3]))
            })
        }
        MediaKind::Emf => {
            bytes.len() >= 88
                && bytes.starts_with(&[1, 0, 0, 0])
                && bytes.get(40..44) == Some(b" EMF")
        }
        _ => false,
    }
}
