//! Decode a single page for comparison with an external raster oracle.
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("requires input, one-based page and output PNG".into());
    }
    let mut limits = DjvuLimits::default();
    let mut foreground = false;
    let mut at = 3;
    while at < args.len() {
        match args[at].as_str() {
            "--foreground" => foreground = true,
            "--max-ink-pixels" => {
                at += 1;
                limits.max_ink_pixels = args
                    .get(at)
                    .ok_or("--max-ink-pixels requires a value")?
                    .parse()?;
            }
            other => return Err(format!("unknown option {other}").into()),
        }
        at += 1;
    }
    let doc = DjvuDocument::open(&std::fs::read(&args[0])?, limits)?;
    let number = args[1].parse::<usize>()?;
    let mut page = doc
        .pages()
        .get(number.checked_sub(1).ok_or("page zero")?)
        .ok_or("page out of range")?
        .clone();
    if foreground {
        page.chunks.retain(|c| !matches!(&c.id, b"BG44" | b"BGjp"));
    }
    std::fs::write(&args[2], page.image_png(doc.limits)?)?;
    Ok(())
}
