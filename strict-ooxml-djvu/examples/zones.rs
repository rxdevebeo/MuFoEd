//! Emit native word-zone rectangles as JSON for the zone oracle driver (SC-D5).
//!
//! Rectangles are in points from the top-left (the public `Rect` convention);
//! the driver converts them to the pixel, bottom-left system `djvutxt` uses.
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, TextLayer, TextZone, ZoneKind};

fn json_string(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn collect_words(zone: &TextZone, out: &mut Vec<String>) {
    if zone.kind == ZoneKind::Word {
        out.push(format!(
            "{{\"x\":{:.4},\"y\":{:.4},\"w\":{:.4},\"h\":{:.4},\"text\":{}}}",
            zone.rect.x,
            zone.rect.y,
            zone.rect.width,
            zone.rect.height,
            json_string(&zone.text)
        ));
    }
    for child in &zone.children {
        collect_words(child, out);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("requires a DjVu path")?;
    let bytes = std::fs::read(&path)?;
    let document = DjvuDocument::open(&bytes, DjvuLimits::default())?;
    let name = std::path::Path::new(&path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut pages = Vec::new();
    for page in document.pages() {
        let mut words = Vec::new();
        if let TextLayer::Readable(root) = &page.text {
            collect_words(root, &mut words);
        }
        pages.push(format!(
            "{{\"page\":{},\"width\":{},\"height\":{},\"dpi\":{},\"rotation\":{},\"words\":[{}]}}",
            page.number,
            page.info.width,
            page.info.height,
            page.info.dpi,
            page.info.rotation,
            words.join(",")
        ));
    }
    println!(
        "{{\"file\":{},\"pages\":[{}]}}",
        json_string(&name),
        pages.join(",")
    );
    Ok(())
}
