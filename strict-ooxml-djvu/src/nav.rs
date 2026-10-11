//! `NAVM` outline reading: the `(bookmarks …)` tree, BZZ-decoded.
//!
//! Format: a `BE16` total entry count, then a flat pre-order forest. Each node is
//! a child count, a title length, the UTF-8 title, a URL length and the URL. The
//! DjVu3 specification writes the child count as one `BYTE` and the title length
//! as `BE24`; DjVuLibre (3.5.x) writes the child count as a little-endian `BE16`
//! and the title length as `BE16`. For fewer than 256 children the high count
//! byte is zero, so the `BYTE` + `BE24` read below consumes DjVuLibre's extra byte
//! as the `BE24`'s high byte and both layouts decode identically (verified against
//! `djvused set-outline` / `print-outline`). The parser is bounded and tolerant:
//! a malformed tree yields the nodes read so far, never a panic.
use crate::{invalid, DjvuError, DjvuLimits};

/// One outline entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    /// Bookmark title.
    pub title: String,
    /// Link target: `#page=N`, `#name`, or an external URL.
    pub url: String,
    /// Nested entries.
    pub children: Vec<NavItem>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn byte(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    fn u24(&mut self) -> Option<usize> {
        let raw = self.bytes.get(self.at..self.at + 3)?;
        self.at += 3;
        Some((usize::from(raw[0]) << 16) | (usize::from(raw[1]) << 8) | usize::from(raw[2]))
    }

    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let raw = self.bytes.get(self.at..self.at.checked_add(length)?)?;
        self.at += length;
        Some(raw)
    }

    fn node(&mut self, depth: usize, budget: &mut usize) -> Option<NavItem> {
        if depth > 32 || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let children = self.byte()?;
        let title_len = self.u24()?;
        let title = String::from_utf8_lossy(self.take(title_len)?).into_owned();
        let url_len = self.u24()?;
        let url = String::from_utf8_lossy(self.take(url_len)?).into_owned();
        let mut items = Vec::new();
        for _ in 0..children {
            match self.node(depth + 1, budget) {
                Some(child) => items.push(child),
                None => break,
            }
        }
        Some(NavItem {
            title,
            url,
            children: items,
        })
    }
}

/// Parses a BZZ-decoded `NAVM` payload into an outline forest.
///
/// # Errors
///
/// Returns [`DjvuError::Limit`] when the node budget is exhausted; a truncated
/// tree yields the nodes read so far.
pub(crate) fn parse(data: &[u8], limits: DjvuLimits) -> Result<Vec<NavItem>, DjvuError> {
    if data.len() < 2 {
        return Ok(Vec::new());
    }
    let mut cursor = Cursor { bytes: data, at: 2 };
    let mut budget = limits.max_zones.min(1_000_000);
    let mut roots = Vec::new();
    while cursor.at < data.len() {
        if budget == 0 {
            return Err(invalid("NAVM node budget exhausted"));
        }
        match cursor.node(0, &mut budget) {
            Some(node) => roots.push(node),
            None => break,
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_forest_is_read() {
        // total 2, then two leaf nodes "A" -> "#page=1" and "B" -> "#page=2".
        let mut data = vec![0, 2];
        for (title, url) in [("A", "#page=1"), ("B", "#page=2")] {
            data.push(0);
            data.extend_from_slice(&(title.len() as u32).to_be_bytes()[1..]);
            data.extend_from_slice(title.as_bytes());
            data.extend_from_slice(&(url.len() as u32).to_be_bytes()[1..]);
            data.extend_from_slice(url.as_bytes());
        }
        let items = parse(&data, DjvuLimits::default()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "A");
        assert_eq!(items[1].url, "#page=2");
    }

    #[test]
    fn nesting_is_read_and_truncation_never_panics() {
        // total 2: "One" (one child "One A") then "Two".
        let mut data = vec![0, 2];
        let node = |children: u8, title: &str, url: &str, out: &mut Vec<u8>| {
            out.push(children);
            out.extend_from_slice(&(title.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(title.as_bytes());
            out.extend_from_slice(&(url.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(url.as_bytes());
        };
        node(1, "One", "#page=1", &mut data);
        node(0, "One A", "#page=2", &mut data);
        node(0, "Two", "#page=1", &mut data);
        let items = parse(&data, DjvuLimits::default()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].children[0].title, "One A");
        // A cut in the middle must not panic.
        let _ = parse(&data[..data.len() - 3], DjvuLimits::default());
        let _ = parse(&[0, 9, 1, 0, 0, 200], DjvuLimits::default());
    }

    #[test]
    fn the_djvulibre_layout_is_read_too() {
        // DjVuLibre writes the child count as LE16 and the title length as BE16;
        // the specification's `BYTE` + `BE24` read must consume the same bytes.
        let mut data = vec![0, 2];
        let node = |count: u16, title: &str, url: &str, out: &mut Vec<u8>| {
            out.push((count & 0xff) as u8);
            out.push((count >> 8) as u8);
            out.extend_from_slice(&(title.len() as u16).to_be_bytes());
            out.extend_from_slice(title.as_bytes());
            out.extend_from_slice(&(url.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(url.as_bytes());
        };
        node(1, "Chapter", "#page=1", &mut data);
        node(0, "Section", "#page=2", &mut data);
        let items = parse(&data, DjvuLimits::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Chapter");
        assert_eq!(items[0].children[0].title, "Section");
        assert_eq!(items[0].children[0].url, "#page=2");
    }
}
