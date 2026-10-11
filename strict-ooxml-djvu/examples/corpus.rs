//! Bounded corpus inventory. Takes a directory and emits one TSV row per file.
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, TextLayer};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("requires a corpus directory")?;
    let out = std::env::args_os().nth(2).map(std::path::PathBuf::from);
    if let Some(p) = &out {
        std::fs::create_dir_all(p)?;
    }
    let mut paths = std::fs::read_dir(root)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    println!("file\tpages\treadable\tempty\tunusable\tchars\tresult");
    for path in paths {
        if path
            .extension()
            .is_none_or(|s| !s.eq_ignore_ascii_case("djvu"))
        {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        match DjvuDocument::open(&bytes, DjvuLimits::default()) {
            Ok(doc) => {
                let (mut readable, mut empty, mut unusable, mut chars) = (0, 0, 0, 0);
                let mut text = String::new();
                for page in doc.pages() {
                    match &page.text {
                        TextLayer::Readable(z) => {
                            readable += 1;
                            chars += z.text.chars().count();
                            text.push_str(&z.text);
                        }
                        TextLayer::Empty => empty += 1,
                        TextLayer::Unusable { text: raw, .. } => {
                            unusable += 1;
                            text.push_str(raw);
                        }
                    }
                    text.push('\u{c}');
                }
                println!(
                    "{}\t{}\t{readable}\t{empty}\t{unusable}\t{chars}\tOK",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    doc.pages().len()
                );
                if let Some(out) = &out {
                    std::fs::write(
                        out.join(path.file_name().unwrap_or_default())
                            .with_extension("txt"),
                        text,
                    )?;
                }
            }
            Err(e) => println!(
                "{}\t0\t0\t0\t0\t0\tERROR: {e}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        }
    }
    Ok(())
}
