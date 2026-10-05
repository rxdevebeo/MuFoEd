//! UTF-8 text ranges and the 17-byte delta-encoded zone tree.
use crate::{bounded, invalid, DjvuError, DjvuLimits, PageInfo};
/// Text-zone types, ordered from containers to characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ZoneKind {
    /// Entire page.
    Page = 1,
    /// Text column.
    Column = 2,
    /// Region inside a column.
    Region = 3,
    /// Paragraph.
    Paragraph = 4,
    /// Visual line.
    Line = 5,
    /// Word.
    Word = 6,
    /// Character.
    Character = 7,
}
/// Rectangle in points from the top-left, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}
/// A native text zone, without invented glyph metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct TextZone {
    /// Native zone kind.
    pub kind: ZoneKind,
    /// Rectangle in the unrotated page coordinate system.
    pub rect: Rect,
    /// Exact UTF-8 range supplied by the file.
    pub text: String,
    /// Native child order, which is reading order.
    pub children: Vec<Self>,
}
/// Missing text and damaged text are different facts.
#[derive(Debug, Clone, PartialEq)]
pub enum TextLayer {
    /// A tree containing paragraph or line zones.
    Readable(TextZone),
    /// No TXTa/TXTz chunk exists.
    Empty,
    /// A chunk exists but cannot provide structured text. OCR does not overwrite it.
    Unusable {
        /// Valid UTF-8 text retained when the tree is absent or invalid.
        text: String,
        /// Why structure could not be read.
        reason: String,
    },
}
#[derive(Clone, Copy)]
struct Raw {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    start: i64,
    len: usize,
}
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], DjvuError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| invalid("text range overflow"))?;
        let bytes = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| invalid("truncated text zone"))?;
        self.pos = end;
        Ok(bytes)
    }
    fn unsigned(&mut self, n: usize) -> Result<usize, DjvuError> {
        Ok(self
            .take(n)?
            .iter()
            .fold(0, |v, &b| (v << 8) | usize::from(b)))
    }
    fn biased(&mut self) -> Result<i64, DjvuError> {
        Ok(self.unsigned(2)? as i64 - 0x8000)
    }
}
pub(crate) fn parse(
    bytes: &[u8],
    info: PageInfo,
    limits: DjvuLimits,
) -> Result<TextLayer, DjvuError> {
    bounded(bytes.len(), limits.max_text_bytes, "max_text_bytes")?;
    let mut c = Cursor { bytes, pos: 0 };
    let size = c.unsigned(3)?;
    let text = std::str::from_utf8(c.take(size)?)
        .map_err(|_| invalid("text is not UTF-8"))?
        .to_owned();
    if c.pos == bytes.len() {
        return Ok(TextLayer::Unusable {
            text,
            reason: "text has no zone tree".into(),
        });
    }
    if c.unsigned(1)? != 1 {
        return Ok(TextLayer::Unusable {
            text,
            reason: "unsupported zone version".into(),
        });
    }
    let mut count = 0;
    let mut copied = 0;
    match zone(
        &mut c,
        &text,
        info,
        limits,
        None,
        None,
        0,
        &mut count,
        &mut copied,
    ) {
        Ok((root, _)) if c.pos == bytes.len() && has_lines(&root) => Ok(TextLayer::Readable(root)),
        Ok(_) => Ok(TextLayer::Unusable {
            text,
            reason: "no line/paragraph zones or trailing zone bytes".into(),
        }),
        Err(e @ DjvuError::Limit(_)) => Err(e),
        Err(e) => Ok(TextLayer::Unusable {
            text,
            reason: e.to_string(),
        }),
    }
}
fn has_lines(z: &TextZone) -> bool {
    matches!(z.kind, ZoneKind::Paragraph | ZoneKind::Line) || z.children.iter().any(has_lines)
}
#[allow(clippy::too_many_arguments)]
fn zone(
    c: &mut Cursor<'_>,
    text: &str,
    info: PageInfo,
    limits: DjvuLimits,
    parent: Option<Raw>,
    prev: Option<Raw>,
    depth: usize,
    count: &mut usize,
    copied: &mut usize,
) -> Result<(TextZone, Raw), DjvuError> {
    bounded(depth, limits.max_zone_depth, "max_zone_depth")?;
    *count += 1;
    bounded(*count, limits.max_zones, "max_zones")?;
    let kind = match c.unsigned(1)? {
        1 => ZoneKind::Page,
        2 => ZoneKind::Column,
        3 => ZoneKind::Region,
        4 => ZoneKind::Paragraph,
        5 => ZoneKind::Line,
        6 => ZoneKind::Word,
        7 => ZoneKind::Character,
        _ => return Err(invalid("unknown text zone type")),
    };
    let (mut x, mut y, w, h, mut start) = (
        c.biased()?,
        c.biased()?,
        c.biased()?,
        c.biased()?,
        c.biased()?,
    );
    let len = c.unsigned(3)?;
    let children = c.unsigned(3)?;
    bounded(
        children,
        limits.max_zones.saturating_sub(*count),
        "max_zones",
    )?;
    if w <= 0 || h <= 0 {
        return Err(invalid("empty text-zone rectangle"));
    }
    if let Some(p) = prev {
        if matches!(kind, ZoneKind::Page | ZoneKind::Paragraph | ZoneKind::Line) {
            x += p.x;
            y = p.y - y - h;
        } else {
            x += p.x + p.w;
            y += p.y;
        }
        start += p.start + p.len as i64;
    } else if let Some(p) = parent {
        x += p.x;
        y = p.y + p.h - y - h;
        start += p.start;
    }
    let offset = usize::try_from(start).map_err(|_| invalid("negative zone text offset"))?;
    let end = offset
        .checked_add(len)
        .ok_or_else(|| invalid("zone text overflow"))?;
    let value = text
        .get(offset..end)
        .ok_or_else(|| invalid("zone range exceeds text or splits UTF-8"))?;
    if let Some(p) = parent {
        if start < p.start || end > (p.start as usize).saturating_add(p.len) {
            return Err(invalid("child text range outside parent"));
        }
    }
    *copied = copied
        .checked_add(value.len())
        .ok_or(DjvuError::Limit("max_zone_text_bytes"))?;
    bounded(*copied, limits.max_zone_text_bytes, "max_zone_text_bytes")?;
    let raw = Raw {
        x,
        y,
        w,
        h,
        start,
        len,
    };
    let scale = 72.0 / f64::from(info.dpi);
    let rect = Rect {
        x: x as f64 * scale,
        y: (i64::from(info.height) - y - h) as f64 * scale,
        width: w as f64 * scale,
        height: h as f64 * scale,
    };
    let mut result = TextZone {
        kind,
        rect,
        text: value.to_owned(),
        children: Vec::new(),
    };
    let mut previous = None;
    for _ in 0..children {
        let (child, r) = zone(
            c,
            text,
            info,
            limits,
            Some(raw),
            previous,
            depth + 1,
            count,
            copied,
        )?;
        result.children.push(child);
        previous = Some(r);
    }
    Ok((result, raw))
}
