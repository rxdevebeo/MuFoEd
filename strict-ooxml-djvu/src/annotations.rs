//! Annotation chunk (`ANTa`/`ANTz`) reading: `maparea` rectangles and targets.
//!
//! Only the parts SC-D9 needs are read: `(maparea … )` with a `(rect x y w h)`
//! and either a `(url "…")` or a `(page N)` target. The scan is tolerant — the
//! grammar has many annotation types and producers differ — so anything it does
//! not recognise is simply skipped, never a panic.
use crate::text::Rect;
use crate::PageInfo;

/// Where a `maparea` points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationTarget {
    /// An external URL.
    Url(String),
    /// An in-document reference: `page=N`, `+N`/`-N`, a page id or a title.
    Page(String),
}

/// One resolvable `maparea` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Rectangle in points from the top-left, y down (the [`Rect`] convention).
    pub rect: Rect,
    /// The link target.
    pub target: AnnotationTarget,
}

/// Document metadata from `(metadata (Key "value") …)` and `(xmp "…")`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DjvuMetadata {
    /// Key/value pairs in first-seen order, keys as the file spells them.
    pub entries: Vec<(String, String)>,
    /// The raw XMP packet, when the file carries one.
    pub xmp: Option<String>,
}

impl DjvuMetadata {
    /// The value of `key` (case-insensitive), if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    /// Whether nothing was found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.xmp.is_none()
    }
}

/// Caps work on a hostile chunk; a page cannot carry more annotations usefully.
const MAX_ANNOTATIONS: usize = 200_000;

pub(crate) fn parse(bytes: &[u8], info: PageInfo) -> Vec<Annotation> {
    let text = String::from_utf8_lossy(bytes);
    let scale = 72.0 / f64::from(info.dpi);
    let height = i64::from(info.height);
    let mut out = Vec::new();
    let mut at = 0;
    while out.len() < MAX_ANNOTATIONS {
        let Some(found) = text[at..].find("(maparea") else {
            break;
        };
        let start = at + found;
        let Some(end) = matching_paren(&text, start) else {
            break;
        };
        let region = &text[start..=end];
        if let Some((x, y, w, h)) = find_rect(region) {
            if let Some(target) = find_target(region) {
                out.push(Annotation {
                    rect: Rect {
                        x: x as f64 * scale,
                        y: (height - y - h) as f64 * scale,
                        width: w as f64 * scale,
                        height: h as f64 * scale,
                    },
                    target,
                });
            }
        }
        at = end + 1;
    }
    out
}

/// The index of the `)` matching the `(` at `open`, skipping `"…"` strings.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escape = false;
    let mut index = open;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

/// The four integers of `(rect x y w h)`.
fn find_rect(region: &str) -> Option<(i64, i64, i64, i64)> {
    let at = region.find("(rect")? + "(rect".len();
    let numbers: Vec<i64> = region[at..]
        .replace(')', " ")
        .split_whitespace()
        .take(4)
        .map(str::parse::<i64>)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers.as_slice() {
        [x, y, w, h] if *w > 0 && *h > 0 => Some((*x, *y, *w, *h)),
        _ => None,
    }
}

/// A `(url "…")` first, otherwise a `(page N)`.
fn find_target(region: &str) -> Option<AnnotationTarget> {
    if let Some(at) = region.find("(url") {
        let rest = &region[at..];
        if let Some(open) = rest.find('"') {
            if let Some(href) = quoted(rest, open) {
                if !href.is_empty() {
                    return Some(classify(&href));
                }
            }
        }
    }
    let at = region.find("(page")? + "(page".len();
    let page = region[at..]
        .replace(')', " ")
        .split_whitespace()
        .next()?
        .parse::<usize>()
        .ok()?;
    (page > 0).then(|| AnnotationTarget::Page(format!("page={page}")))
}

/// A `maparea` href is internal when it is a fragment or a page offset.
fn classify(href: &str) -> AnnotationTarget {
    if let Some(reference) = href.strip_prefix('#') {
        AnnotationTarget::Page(reference.to_owned())
    } else if href.starts_with('+') || href.starts_with('-') {
        AnnotationTarget::Page(href.to_owned())
    } else {
        AnnotationTarget::Url(href.to_owned())
    }
}

/// The `"…"` string whose opening quote is at byte index `open`, unescaped.
fn quoted(text: &str, open: usize) -> Option<String> {
    let mut out = String::new();
    let mut chars = text[open + 1..].chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

/// Reads `(metadata (Key "value") …)` and `(xmp "…")` from an annotation chunk.
pub(crate) fn metadata(bytes: &[u8]) -> DjvuMetadata {
    let text = String::from_utf8_lossy(bytes);
    let mut out = DjvuMetadata::default();
    if let Some(at) = text.find("(xmp") {
        let rest = &text[at..];
        if let Some(open) = rest.find('"') {
            out.xmp = quoted(rest, open);
        }
    }
    if let Some(at) = text.find("(metadata") {
        if let Some(end) = matching_paren(&text, at) {
            let region = &text[at..=end];
            // Skip the `(metadata` head so the scan starts at the pairs.
            let mut cursor = "(metadata".len();
            while let Some(offset) = region[cursor..].find('(') {
                let open = cursor + offset;
                let Some(close) = matching_paren(region, open) else {
                    break;
                };
                if let Some((key, value)) = key_value(&region[open + 1..close]) {
                    out.entries.push((key, value));
                }
                cursor = close + 1;
            }
        }
    }
    out
}

/// `Key "value"` to a pair, or `None` for a nested/other expression.
fn key_value(entry: &str) -> Option<(String, String)> {
    let trimmed = entry.trim_start();
    let key_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let key = &trimmed[..key_end];
    if key.is_empty() || key.starts_with('"') || key.contains('(') {
        return None;
    }
    let rest = &trimmed[key_end..];
    let open = rest.find('"')?;
    let value = quoted(rest, open)?;
    Some((key.to_owned(), value))
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn info() -> PageInfo {
        PageInfo {
            width: 100,
            height: 100,
            dpi: 72,
            rotation: 0,
            gamma: 22,
            version: 25,
        }
    }

    #[test]
    fn a_url_maparea_becomes_a_rect_and_target() {
        let bytes = b"(maparea \"\" (rect 10 20 30 40) (url \"https://example.invalid/x\"))";
        let found = parse(bytes, info());
        assert_eq!(found.len(), 1);
        // Bottom-left pixels to top-left points at 72 dpi: y = 100 - 20 - 40.
        assert_eq!(found[0].rect.x, 10.0);
        assert_eq!(found[0].rect.y, 40.0);
        assert_eq!(found[0].rect.width, 30.0);
        assert_eq!(found[0].rect.height, 40.0);
        assert_eq!(
            found[0].target,
            AnnotationTarget::Url("https://example.invalid/x".to_owned())
        );
    }

    #[test]
    fn internal_references_are_page_targets() {
        // The `#page=N` href and the `(page N)` compatibility form both resolve.
        for bytes in [
            b"(maparea (rect 0 0 5 5) (url \"#page=3\"))".as_slice(),
            b"(maparea (rect 0 0 5 5) (page 3))".as_slice(),
        ] {
            let found = parse(bytes, info());
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].target, AnnotationTarget::Page("page=3".to_owned()));
        }
        let offset = parse(b"(maparea (rect 0 0 5 5) (url \"+1\"))", info());
        assert_eq!(offset[0].target, AnnotationTarget::Page("+1".to_owned()));
    }

    #[test]
    fn nested_parens_escapes_and_garbage_never_panic() {
        let bytes = b"(maparea \"a (b)\" (rect 0 0 5 5) (url \"x\\\"y\")) (junk (unbalanced";
        let found = parse(bytes, info());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].target, AnnotationTarget::Url("x\"y".to_owned()));
        // A zero-size rect is not a hotspot.
        assert!(parse(b"(maparea (rect 0 0 0 5) (url \"u\"))", info()).is_empty());
    }

    #[test]
    fn metadata_pairs_and_xmp_are_read() {
        let bytes =
            b"(metadata (Title \"Book\") (Author \"A. Uthor\") (Keywords \"a, b\")) (xmp \"<x/>\")";
        let found = metadata(bytes);
        assert_eq!(found.get("title"), Some("Book"));
        assert_eq!(found.get("AUTHOR"), Some("A. Uthor"));
        assert_eq!(found.get("Keywords"), Some("a, b"));
        assert_eq!(found.xmp.as_deref(), Some("<x/>"));
    }
}
