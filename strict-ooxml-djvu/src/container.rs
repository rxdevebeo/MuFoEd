//! Bounded, zero-copy IFF walking with absolute-offset alignment.
use crate::{bounded, invalid, DjvuError, DjvuLimits};
use std::{ops::Range, sync::Arc};
/// One IFF header and its payload. Clones share the source file.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Four ASCII bytes identifying the chunk.
    pub id: [u8; 4],
    /// A FORM's secondary identifier.
    pub form_type: Option<[u8; 4]>,
    /// Header offset in the component file.
    pub offset: usize,
    /// Declared payload bytes, including a FORM's secondary identifier.
    pub length: usize,
    /// Nesting depth, zero for the root.
    pub depth: usize,
    pub(crate) source: Arc<[u8]>,
    pub(crate) range: Range<usize>,
    pub(crate) children: Vec<Chunk>,
}
impl Chunk {
    /// Payload bytes as declared by this header.
    pub fn data(&self) -> &[u8] {
        &self.source[self.range.clone()]
    }
    /// Direct children of a FORM.
    pub fn children(&self) -> &[Self] {
        &self.children
    }
}
pub(crate) fn parse(
    bytes: &Arc<[u8]>,
    limits: DjvuLimits,
    count: &mut usize,
) -> Result<Chunk, DjvuError> {
    bounded(bytes.len(), limits.max_file_bytes, "max_file_bytes")?;
    if !matches!(bytes.get(..4), Some(b"AT&T" | b"SDJV")) {
        return Err(invalid("missing AT&T/SDJV magic"));
    }
    let mut pos = 4;
    let root = chunk(bytes, &mut pos, bytes.len(), 0, limits, count)?;
    if root.id != *b"FORM" {
        return Err(invalid("root must be FORM"));
    }
    if pos < bytes.len() && pos % 2 == 1 && pos + 1 == bytes.len() {
        pos += 1;
    }
    if pos != bytes.len() {
        return Err(invalid("trailing bytes after root FORM"));
    }
    Ok(root)
}
fn chunk(
    source: &Arc<[u8]>,
    pos: &mut usize,
    end: usize,
    depth: usize,
    limits: DjvuLimits,
    count: &mut usize,
) -> Result<Chunk, DjvuError> {
    bounded(depth, limits.max_zone_depth, "max_zone_depth")?;
    while matches!(
        source.get(*pos..pos.saturating_add(4)),
        Some(b"AT&T" | b"SDJV")
    ) {
        *pos += 4;
    }
    let offset = *pos;
    let header = source
        .get(offset..offset.saturating_add(8))
        .filter(|_| offset.saturating_add(8) <= end)
        .ok_or_else(|| invalid("truncated IFF header"))?;
    let id: [u8; 4] = header[..4].try_into().map_err(|_| invalid("IFF id"))?;
    if !id.iter().all(|b| (32..=126).contains(b)) {
        return Err(invalid("non-ASCII IFF identifier"));
    }
    let length =
        u32::from_be_bytes(header[4..8].try_into().map_err(|_| invalid("IFF length"))?) as usize;
    bounded(length, limits.max_chunk_bytes, "max_chunk_bytes")?;
    *count = count.checked_add(1).ok_or(DjvuError::Limit("max_chunks"))?;
    bounded(*count, limits.max_chunks, "max_chunks")?;
    let start = offset + 8;
    let stop = start
        .checked_add(length)
        .filter(|&n| n <= end)
        .ok_or_else(|| invalid("chunk extends beyond enclosing FORM"))?;
    let mut result = Chunk {
        id,
        form_type: None,
        offset,
        length,
        depth,
        source: source.clone(),
        range: start..stop,
        children: Vec::new(),
    };
    if id == *b"FORM" {
        let kind: [u8; 4] = source
            .get(start..start + 4)
            .filter(|_| length >= 4)
            .ok_or_else(|| invalid("FORM has no secondary id"))?
            .try_into()
            .map_err(|_| invalid("FORM type"))?;
        result.form_type = Some(kind);
        let mut inner = start + 4;
        while inner < stop {
            if inner % 2 == 1 {
                inner += 1;
            }
            if inner == stop {
                break;
            }
            result
                .children
                .push(chunk(source, &mut inner, stop, depth + 1, limits, count)?);
        }
        if inner != stop {
            return Err(invalid("IFF alignment crosses FORM boundary"));
        }
    }
    *pos = stop;
    Ok(result)
}
