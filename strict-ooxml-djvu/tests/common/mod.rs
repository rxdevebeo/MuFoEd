#![allow(
    unexpected_cfgs,
    dead_code,
    clippy::cast_possible_truncation,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_sign_loss
)]
use strict_ooxml_djvu::ZoneKind;
pub(crate) fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}
pub(crate) fn form(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    for c in chunks {
        if body.len() % 2 == 1 {
            body.push(0);
        }
        body.extend_from_slice(c);
    }
    chunk(b"FORM", &body)
}
pub(crate) fn file(form: &[u8]) -> Vec<u8> {
    let mut out = b"AT&T".to_vec();
    out.extend_from_slice(form);
    out
}
pub(crate) fn info(w: u16, h: u16, dpi: u16) -> Vec<u8> {
    let mut out = w.to_be_bytes().to_vec();
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&[26, 0]);
    out.extend_from_slice(&dpi.to_le_bytes());
    out.extend_from_slice(&[22, 1]);
    out
}
pub(crate) fn single(chunks: &[Vec<u8>]) -> Vec<u8> {
    file(&form(b"DJVU", chunks))
}
pub(crate) fn u24(n: usize) -> [u8; 3] {
    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
}
pub(crate) fn zone(
    kind: ZoneKind,
    coords: [i16; 5],
    length: usize,
    children: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = vec![kind as u8];
    for v in coords {
        out.extend_from_slice(&((i32::from(v) + 0x8000) as u16).to_be_bytes());
    }
    out.extend_from_slice(&u24(length));
    out.extend_from_slice(&u24(children.len()));
    for c in children {
        out.extend_from_slice(c);
    }
    out
}
pub(crate) fn text(text: &str) -> Vec<u8> {
    let line = zone(ZoneKind::Line, [0, 0, 80, 10, 0], text.len(), &[]);
    let paragraph = zone(
        ZoneKind::Paragraph,
        [10, 10, 80, 10, 0],
        text.len(),
        &[line],
    );
    let page = zone(
        ZoneKind::Page,
        [0, 0, 100, 100, 0],
        text.len(),
        &[paragraph],
    );
    let mut out = u24(text.len()).to_vec();
    out.extend_from_slice(text.as_bytes());
    out.push(1);
    out.extend_from_slice(&page);
    out
}
pub(crate) fn text_file(value: &str, compressed: bool) -> Vec<u8> {
    let txt = text(value);
    let txt = if compressed {
        chunk(b"TXTz", &djvu_bzz::bzz_encode(&txt))
    } else {
        chunk(b"TXTa", &txt)
    };
    single(&[chunk(b"INFO", &info(100, 100, 72)), txt])
}
/// A plain annotation chunk holding `expression` as its UTF-8 S-expression.
pub(crate) fn ant(expression: &str) -> Vec<u8> {
    chunk(b"ANTa", expression.as_bytes())
}
#[cfg(feature = "djvu-image")]
pub(crate) fn scan() -> Vec<u8> {
    let mut bm = djvu_bitmap::Bitmap::new(100, 100);
    for y in 10..15 {
        for x in 10..40 {
            bm.set_black(x, y);
        }
    }
    let data = djvu_jb2::encode::encode_jb2_lossless(&bm);
    single(&[chunk(b"INFO", &info(100, 100, 72)), chunk(b"Sjbz", &data)])
}
pub(crate) fn directory(entries: &[(&str, u8, usize, usize)], bundled: bool) -> Vec<u8> {
    let mut payload = vec![if bundled { 0x81 } else { 1 }];
    payload.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    if bundled {
        for (_, _, offset, _) in entries {
            payload.extend_from_slice(&(*offset as u32).to_be_bytes());
        }
    }
    let mut raw = Vec::new();
    for (_, _, _, size) in entries {
        raw.extend_from_slice(&u24(*size));
    }
    for (_, kind, _, _) in entries {
        raw.push(*kind);
    }
    for (id, _, _, _) in entries {
        raw.extend_from_slice(id.as_bytes());
        raw.push(0);
    }
    payload.extend_from_slice(&djvu_bzz::bzz_encode(&raw));
    chunk(b"DIRM", &payload)
}
pub(crate) fn bundled(pages: &[Vec<u8>]) -> Vec<u8> {
    bundled_with(pages, &[])
}

/// A bundled `DJVM` with extra root-level chunks (for example `NAVM`) placed
/// after `DIRM` and before the pages.
pub(crate) fn bundled_with(pages: &[Vec<u8>], extra: &[Vec<u8>]) -> Vec<u8> {
    let names: Vec<_> = (0..pages.len()).map(|i| format!("p{i}.djvu")).collect();
    // BZZ excludes offsets; changing the offset table cannot change DIRM size.
    let dummy: Vec<_> = pages
        .iter()
        .enumerate()
        .map(|(i, p)| (names[i].as_str(), 1, 0, p.len()))
        .collect();
    let dir = directory(&dummy, true);
    let mut offset = 16 + dir.len();
    if offset % 2 == 1 {
        offset += 1;
    }
    for chunk in extra {
        offset += chunk.len();
        if offset % 2 == 1 {
            offset += 1;
        }
    }
    let mut entries = Vec::new();
    for (i, p) in pages.iter().enumerate() {
        entries.push((names[i].as_str(), 1, offset, p.len()));
        offset += p.len();
        if offset % 2 == 1 {
            offset += 1;
        }
    }
    let mut children = vec![directory(&entries, true)];
    children.extend_from_slice(extra);
    children.extend_from_slice(pages);
    file(&form(b"DJVM", &children))
}

/// A decoded `NAVM` payload: `BE16` entry count, then each node as `DjVuLibre`
/// writes it (`LE16` child count, `BE16` title length, title, `BE24` url length,
/// url) in pre-order.
pub(crate) fn navm_entries(entries: &[(u16, &str, &str)]) -> Vec<u8> {
    let mut out = (entries.len() as u16).to_be_bytes().to_vec();
    for (count, title, url) in entries {
        out.push((*count & 0xff) as u8);
        out.push((*count >> 8) as u8);
        out.extend_from_slice(&(title.len() as u16).to_be_bytes());
        out.extend_from_slice(title.as_bytes());
        out.extend_from_slice(&u24(url.len()));
        out.extend_from_slice(url.as_bytes());
    }
    out
}

/// A BZZ-encoded `NAVM` outline chunk.
pub(crate) fn navm(entries: &[(u16, &str, &str)]) -> Vec<u8> {
    chunk(b"NAVM", &djvu_bzz::bzz_encode(&navm_entries(entries)))
}
