//! Directory and include resolution. No filesystem or network access is implicit.
use crate::{
    bounded, container, decompress, invalid, nav, text, xmp, Chunk, DjvuError, DjvuLimits,
    TextLayer,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
/// Page geometry from INFO. Text coordinates remain unrotated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageInfo {
    /// Unrotated width in pixels.
    pub width: u16,
    /// Unrotated height in pixels.
    pub height: u16,
    /// Pixels per inch, clamped to the format's supported range.
    pub dpi: u16,
    /// Display rotation in degrees, counterclockwise.
    pub rotation: u16,
    /// Gamma multiplied by ten.
    pub gamma: u8,
    /// Format version.
    pub version: u16,
}
impl PageInfo {
    /// Unrotated page width and height in points.
    pub fn points(self) -> (f64, f64) {
        let s = 72.0 / f64::from(self.dpi);
        (f64::from(self.width) * s, f64::from(self.height) * s)
    }
}
/// A page in directory order, including resolved shared chunks.
#[derive(Debug, Clone)]
pub struct DjvuPage {
    /// One-based document page number.
    pub number: usize,
    /// Component identifier (empty for a single-page file).
    pub id: String,
    /// Page title supplied by DIRM, when present.
    pub title: String,
    /// INFO fields.
    pub info: PageInfo,
    /// Native text or its explicit absence/damage state.
    pub text: TextLayer,
    /// Resolvable `maparea` annotations (external URLs / page links).
    pub annotations: Vec<crate::Annotation>,
    /// Flattened chunks in include order.
    pub chunks: Vec<Chunk>,
    /// Reader observations. These must be carried into conversion reports.
    pub warnings: Vec<String>,
}
impl DjvuPage {
    /// Whether any image-bearing chunk exists; decoding may still be unsupported.
    pub fn has_image(&self) -> bool {
        self.chunks.iter().any(|c| {
            matches!(
                &c.id,
                b"Sjbz" | b"Smmr" | b"BG44" | b"BGjp" | b"PM44" | b"BM44"
            )
        })
    }
}
/// Parsed document. Source bytes are shared with chunk payloads.
#[derive(Debug, Clone)]
pub struct DjvuDocument {
    /// Container headers in file order, including resolved indirect components.
    pub roots: Vec<Chunk>,
    pages: Vec<DjvuPage>,
    /// Budgets used while opening this document.
    pub limits: DjvuLimits,
    /// Document metadata from `(metadata …)` / `(xmp …)`.
    pub metadata: crate::DjvuMetadata,
    /// The `NAVM` outline as a pre-order forest. Empty when the file has none.
    pub outline: Vec<crate::NavItem>,
    /// Document-level observations, including unsupported navigation.
    pub warnings: Vec<String>,
}
#[derive(Debug)]
struct Entry {
    id: String,
    name: String,
    title: String,
    offset: usize,
    size: usize,
    kind: u8,
}
impl DjvuDocument {
    /// Open a single-page or bundled file. Indirect files require an explicit resolver.
    pub fn open(bytes: &[u8], limits: DjvuLimits) -> Result<Self, DjvuError> {
        Self::open_with_resolver(bytes, limits, |id| {
            Err(DjvuError::Component(format!(
                "{id}: supply an indirect component resolver"
            )))
        })
    }
    /// Open with a component provider. IDs are validated before calling the provider.
    /// The provider controls access; the reader never fetches URLs or follows paths.
    pub fn open_with_resolver<F>(
        bytes: &[u8],
        limits: DjvuLimits,
        mut resolver: F,
    ) -> Result<Self, DjvuError>
    where
        F: FnMut(&str) -> Result<Vec<u8>, DjvuError>,
    {
        bounded(bytes.len(), limits.max_file_bytes, "max_file_bytes")?;
        let mut count = 0;
        let root = container::parse(&Arc::from(bytes), limits, &mut count)?;
        let mut roots = vec![root.clone()];
        let mut components = BTreeMap::new();
        let mut page_roots = Vec::new();
        let mut outline = Vec::new();
        let mut warnings = Vec::new();
        match root.form_type.as_ref() {
            Some(b"DJVU" | b"PM44" | b"BM44") => {
                page_roots.push((String::new(), String::new(), root.clone()));
            }
            Some(b"DJVM") => {
                let dir = root
                    .children()
                    .first()
                    .filter(|c| c.id == *b"DIRM")
                    .ok_or_else(|| invalid("DJVM must start with DIRM"))?;
                let (bundled, entries) = directory(dir.data(), limits)?;
                let pages = entries.iter().filter(|e| e.kind == 1).count();
                bounded(pages, limits.max_pages, "max_pages")?;
                let mut total = bytes.len();
                for entry in entries {
                    let form = if bundled {
                        let c = root
                            .children()
                            .iter()
                            .find(|c| c.offset == entry.offset && c.id == *b"FORM")
                            .ok_or_else(|| {
                                invalid(format!(
                                    "DIRM offset for {} is not a component FORM",
                                    entry.id
                                ))
                            })?;
                        if entry.size != c.length + 8 {
                            return Err(invalid(format!("DIRM size mismatch for {}", entry.id)));
                        }
                        c.clone()
                    } else {
                        let data = resolver(&entry.name)?;
                        total = total
                            .checked_add(data.len())
                            .ok_or(DjvuError::Limit("max_file_bytes"))?;
                        bounded(total, limits.max_file_bytes, "max_file_bytes")?;
                        let c = container::parse(&Arc::from(data), limits, &mut count)?;
                        roots.push(c.clone());
                        c
                    };
                    let expected = match entry.kind {
                        1 => *b"DJVU",
                        0 | 3 => *b"DJVI",
                        2 => *b"THUM",
                        _ => return Err(invalid("unknown DIRM component kind")),
                    };
                    if form.form_type != Some(expected) {
                        return Err(invalid(format!(
                            "DIRM component type mismatch for {}",
                            entry.id
                        )));
                    }
                    if entry.kind == 1 {
                        page_roots.push((entry.id.clone(), entry.title, form.clone()));
                    }
                    if components.insert(entry.id, form).is_some() {
                        return Err(invalid("duplicate DIRM id"));
                    }
                }
            }
            _ => return Err(invalid("document root must be DJVU or DJVM")),
        }
        // SC-D9 L3: the `NAVM` outline is a BZZ-wrapped pre-order forest that
        // sits directly under the root form (after `DIRM` in a `DJVM`).
        if let Some(navm) = root.children().iter().find(|c| c.id == *b"NAVM") {
            match decompress(navm.data(), limits.max_text_bytes, limits) {
                Ok(decoded) => match nav::parse(&decoded, limits) {
                    Ok(items) => outline = items,
                    Err(e @ DjvuError::Limit(_)) => return Err(e),
                    Err(e) => warnings.push(format!("NAVM outline could not be read: {e}")),
                },
                Err(e) => warnings.push(format!("NAVM outline is not BZZ-decodable: {e}")),
            }
        }
        bounded(page_roots.len(), limits.max_pages, "max_pages")?;
        if page_roots.is_empty() {
            return Err(invalid("document has no pages"));
        }
        let mut pages = Vec::new();
        let mut metadata = crate::DjvuMetadata::default();
        let mut expanded = 0;
        for (id, title, form) in page_roots {
            let legacy = matches!(form.form_type.as_ref(), Some(b"PM44" | b"BM44"));
            let info = if legacy {
                let c = form
                    .children()
                    .first()
                    .filter(|c| Some(c.id) == form.form_type)
                    .ok_or_else(|| invalid("legacy FORM must start with matching IW44 chunk"))?;
                let data = c.data();
                if data.len() < 9 || data[0] != 0 {
                    return Err(invalid("invalid legacy IW44 header"));
                }
                let width = u16::from_be_bytes([data[4], data[5]]);
                let height = u16::from_be_bytes([data[6], data[7]]);
                if width == 0 || height == 0 {
                    return Err(invalid("zero legacy IW44 dimensions"));
                }
                PageInfo {
                    width,
                    height,
                    dpi: 100,
                    rotation: 0,
                    gamma: 22,
                    version: 24,
                }
            } else {
                page_info(
                    form.children()
                        .first()
                        .filter(|c| c.id == *b"INFO")
                        .ok_or_else(|| invalid("DJVU must start with INFO"))?
                        .data(),
                )?
            };
            let mut chunks = Vec::new();
            flatten(
                &form,
                &components,
                &mut BTreeSet::new(),
                &mut chunks,
                0,
                limits,
                &mut expanded,
            )?;
            let mut raw = Vec::new();
            let mut present = false;
            let mut ant = Vec::new();
            let mut page_warnings = Vec::new();
            if legacy {
                page_warnings.push("legacy IW44 has no INFO; 100 dpi page size is assumed".into());
            }
            let mut dict_seen = false;
            let mut unknown = 0;
            for c in &chunks {
                match &c.id {
                    b"TXTa" | b"TXTz" => {
                        present = true;
                        let remaining = limits.max_text_bytes.saturating_sub(raw.len());
                        let decoded = if c.id == *b"TXTz" {
                            decompress(c.data(), remaining, limits)?
                        } else {
                            bounded(c.data().len(), remaining, "max_text_bytes")?;
                            c.data().to_vec()
                        };
                        raw.extend_from_slice(&decoded);
                    }
                    b"Djbz" => {
                        if dict_seen {
                            page_warnings.push("multiple JB2 dictionaries are unsupported".into());
                        }
                        dict_seen = true;
                    }
                    b"INFO" | b"Sjbz" | b"BG44" | b"FG44" | b"Smmr" | b"BGjp" | b"FGjp"
                    | b"FGbz" | b"PM44" | b"BM44" => {}
                    b"ANTa" | b"ANTz" => {
                        let remaining = limits.max_text_bytes.saturating_sub(ant.len());
                        let decoded = if c.id == *b"ANTz" {
                            decompress(c.data(), remaining, limits)?
                        } else {
                            bounded(c.data().len(), remaining, "max_text_bytes")?;
                            c.data().to_vec()
                        };
                        ant.extend_from_slice(&decoded);
                    }
                    b"METa" | b"METz" => page_warnings.push(format!(
                        "{} metadata is not mapped",
                        String::from_utf8_lossy(&c.id)
                    )),
                    _ => unknown += 1,
                }
            }
            if unknown > 0 {
                page_warnings.push(format!("{unknown} unknown chunks skipped"));
            }
            // SC-D14: native zone coordinates are unrotated; verified against
            // djvutxt on synthetic 90/180/270 pages, so a rotated page is no
            // longer an untested assumption and carries no warning.
            let annotations = crate::annotations::parse(&ant, info);
            let page_metadata = crate::annotations::metadata(&ant);
            for (key, value) in page_metadata.entries {
                if !metadata
                    .entries
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&key))
                {
                    metadata.entries.push((key, value));
                }
            }
            if metadata.xmp.is_none() {
                metadata.xmp = page_metadata.xmp;
            }
            let layer = if present {
                match text::parse(&raw, info, limits) {
                    Ok(layer) => layer,
                    Err(e @ DjvuError::Limit(_)) => return Err(e),
                    Err(e) => TextLayer::Unusable {
                        text: String::new(),
                        reason: e.to_string(),
                    },
                }
            } else {
                TextLayer::Empty
            };
            pages.push(DjvuPage {
                number: pages.len() + 1,
                id,
                title,
                info,
                text: layer,
                annotations,
                chunks,
                warnings: page_warnings,
            });
        }
        // SC-D9 L4: an XMP packet fills the core properties a native
        // `(metadata …)` block did not state. Native keys win; XMP is the fallback.
        if let Some(packet) = metadata.xmp.clone() {
            for (key, value) in xmp::entries(&packet) {
                if !metadata
                    .entries
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&key))
                {
                    metadata.entries.push((key, value));
                }
            }
        }
        Ok(Self {
            roots,
            pages,
            limits,
            metadata,
            outline,
            warnings,
        })
    }
    /// Pages in authoritative DIRM order.
    pub fn pages(&self) -> &[DjvuPage] {
        &self.pages
    }
    /// Stable container dump; offsets are local to each component file.
    pub fn dump(&self) -> String {
        fn walk(c: &Chunk, out: &mut String) {
            use std::fmt::Write as _;
            let kind = c
                .form_type
                .map(|id| format!(":{}", String::from_utf8_lossy(&id)))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "{}{}{} [{}] @{}",
                "  ".repeat(c.depth),
                String::from_utf8_lossy(&c.id),
                kind,
                c.length,
                c.offset
            );
            for child in &c.children {
                walk(child, out);
            }
        }
        let mut out = String::new();
        for root in &self.roots {
            walk(root, &mut out);
        }
        out
    }
}
fn safe_id(id: &str) -> Result<(), DjvuError> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id
            .bytes()
            .any(|b| b < 32 || matches!(b, b'/' | b'\\' | b':' | 127))
    {
        return Err(DjvuError::Component(format!("unsafe component id {id:?}")));
    }
    Ok(())
}
fn page_info(bytes: &[u8]) -> Result<PageInfo, DjvuError> {
    if bytes.len() < 5 {
        return Err(invalid("short INFO"));
    }
    let word = |p| u16::from_be_bytes([bytes[p], bytes[p + 1]]);
    let width = word(0);
    let height = word(2);
    if width == 0 || height == 0 {
        return Err(invalid("zero INFO dimensions"));
    }
    let version = u16::from(bytes[4]) | u16::from(*bytes.get(5).unwrap_or(&0)) << 8;
    if version >= 50 {
        return Err(invalid("unsupported INFO version >=50"));
    }
    // INFO is the exceptional little-endian field in an otherwise BE container.
    let dpi = if bytes.len() >= 8 {
        u16::from_le_bytes([bytes[6], bytes[7]]).clamp(25, 6000)
    } else {
        300
    };
    let gamma = bytes.get(8).copied().unwrap_or(22).clamp(3, 50);
    let rotation = match bytes.get(9).copied().unwrap_or(1) & 7 {
        6 => 90,
        2 => 180,
        5 => 270,
        _ => 0,
    };
    Ok(PageInfo {
        width,
        height,
        dpi,
        rotation,
        gamma,
        version,
    })
}
fn directory(bytes: &[u8], limits: DjvuLimits) -> Result<(bool, Vec<Entry>), DjvuError> {
    if bytes.len() < 3 {
        return Err(invalid("short DIRM"));
    }
    if bytes[0] & 0x7f > 1 {
        return Err(invalid("unsupported DIRM version"));
    }
    let bundled = bytes[0] & 0x80 != 0;
    let count = usize::from(u16::from_be_bytes([bytes[1], bytes[2]]));
    bounded(count, limits.max_component_files, "max_component_files")?;
    let base = 3 + if bundled { 4 * count } else { 0 };
    let rest = bytes
        .get(base..)
        .ok_or_else(|| invalid("truncated DIRM offsets"))?;
    let decoded = decompress(rest, limits.max_text_bytes, limits)?;
    if decoded.len() < 4 * count {
        return Err(invalid("truncated DIRM sizes/flags"));
    }
    let mut pos = 4 * count;
    fn string(data: &[u8], pos: &mut usize) -> Result<String, DjvuError> {
        let tail = data.get(*pos..).ok_or_else(|| invalid("DIRM strings"))?;
        let n = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated DIRM string"))?;
        let s = std::str::from_utf8(&tail[..n])
            .map_err(|_| invalid("DIRM string is not UTF-8"))?
            .to_owned();
        *pos += n + 1;
        Ok(s)
    }
    let mut entries = Vec::new();
    for i in 0..count {
        let flags = decoded[3 * count + i];
        let id = string(&decoded, &mut pos)?;
        safe_id(&id)?;
        let name = if flags & 0x80 != 0 {
            string(&decoded, &mut pos)?
        } else {
            id.clone()
        };
        safe_id(&name)?;
        let title = if flags & 0x40 != 0 {
            string(&decoded, &mut pos)?
        } else {
            id.clone()
        };
        let size = decoded[i * 3..i * 3 + 3]
            .iter()
            .fold(0usize, |v, &b| (v << 8) | usize::from(b));
        let offset = if bundled {
            u32::from_be_bytes(
                bytes[3 + i * 4..7 + i * 4]
                    .try_into()
                    .map_err(|_| invalid("DIRM offset"))?,
            ) as usize
        } else {
            0
        };
        entries.push(Entry {
            id,
            name,
            title,
            size,
            offset,
            kind: flags & 0x3f,
        });
    }
    if pos != decoded.len() {
        return Err(invalid("trailing DIRM data"));
    }
    Ok((bundled, entries))
}
fn flatten(
    form: &Chunk,
    components: &BTreeMap<String, Chunk>,
    active: &mut BTreeSet<String>,
    out: &mut Vec<Chunk>,
    depth: usize,
    limits: DjvuLimits,
    work: &mut usize,
) -> Result<(), DjvuError> {
    bounded(depth, limits.max_zone_depth, "max_zone_depth")?;
    for c in form.children() {
        *work += 1;
        bounded(*work, limits.max_chunks, "max_chunks (expanded includes)")?;
        if c.id == *b"INCL" {
            let id = std::str::from_utf8(c.data()).map_err(|_| invalid("INCL is not ASCII"))?;
            safe_id(id)?;
            if !active.insert(id.to_owned()) {
                return Err(invalid("cyclic INCL"));
            }
            let target = components
                .get(id)
                .ok_or_else(|| DjvuError::Component(format!("unknown INCL {id}")))?;
            if target.form_type != Some(*b"DJVI") {
                return Err(invalid("INCL must name DJVI"));
            }
            flatten(target, components, active, out, depth + 1, limits, work)?;
            active.remove(id);
        } else {
            if form.form_type == Some(*b"DJVI") && matches!(&c.id, b"INFO" | b"TXTa" | b"TXTz") {
                return Err(invalid("INFO/text in DJVI"));
            }
            if c.id == *b"FORM" {
                return Err(invalid("nested FORM in component"));
            }
            out.push(c.clone());
        }
    }
    Ok(())
}
