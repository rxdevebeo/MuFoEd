//! Inventory native image availability on every page without text.
#![cfg(feature = "djvu-image")]
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, TextLayer};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("requires corpus directory")?;
    let out = std::path::PathBuf::from(
        std::env::args_os()
            .nth(2)
            .ok_or("requires image output directory")?,
    );
    std::fs::create_dir_all(&out)?;
    let mut paths = std::fs::read_dir(root)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    println!("file\tpage\ttext_state\twidth\theight\tresult");
    for path in paths {
        if path
            .extension()
            .is_none_or(|s| !s.eq_ignore_ascii_case("djvu"))
        {
            continue;
        }
        let doc = DjvuDocument::open(&std::fs::read(&path)?, DjvuLimits::default())?;
        for page in doc.pages() {
            if page.number != 1 && matches!(page.text, TextLayer::Readable(_)) {
                continue;
            }
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let result = match page.image(doc.limits) {
                Ok(image) => {
                    std::fs::write(out.join(format!("{name}-{}.png", page.number)), image.bytes)?;
                    if image.has_ink {
                        "OK".into()
                    } else {
                        "BLANK".into()
                    }
                }
                Err(e) => format!("REFUSED: {e}"),
            };
            println!(
                "{}\t{}\t{}\t{}\t{}\t{result}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                page.number,
                if matches!(page.text, TextLayer::Empty) {
                    "empty"
                } else {
                    "native"
                },
                page.info.width,
                page.info.height
            );
        }
    }
    Ok(())
}
