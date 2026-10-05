//! Decode a single page for comparison with an external raster oracle.
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("requires input, one-based page and output PNG".into());
    }
    let doc = DjvuDocument::open(&std::fs::read(&args[0])?, DjvuLimits::default())?;
    let number = args[1].parse::<usize>()?;
    let mut page = doc
        .pages()
        .get(number.checked_sub(1).ok_or("page zero")?)
        .ok_or("page out of range")?
        .clone();
    if args.get(3).is_some_and(|a| a == "--foreground") {
        page.chunks.retain(|c| !matches!(&c.id, b"BG44" | b"BGjp"));
    }
    std::fs::write(&args[2], page.image_png(doc.limits)?)?;
    Ok(())
}
