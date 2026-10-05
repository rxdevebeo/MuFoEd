//! End-to-end DjVu corpus gate with independent XML extraction from written DOCX.
#![allow(clippy::doc_markdown)]
use strict_ooxml_convert::{convert_djvu, DjvuOptions};
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, TextLayer};
fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(c, '\u{1d}' | '\u{1f}')
                && strict_ooxml_core::xml::escape::is_xml_char(*c)
        })
        .collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("requires corpus directory")?;
    let mut paths = std::fs::read_dir(root)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let (mut files, mut failed) = (0, 0);
    println!("file\tparagraphs\tnative_chars\twritten_chars\tresult");
    for path in paths {
        if path
            .extension()
            .is_none_or(|s| !s.eq_ignore_ascii_case("djvu"))
        {
            continue;
        }
        files += 1;
        let bytes = std::fs::read(&path)?;
        let mut reader = DjvuDocument::open(&bytes, DjvuLimits::default())?;
        let original = reader
            .pages()
            .iter()
            .map(|p| match &p.text {
                TextLayer::Readable(z) => compact(&z.text),
                TextLayer::Unusable { text, .. } => compact(text),
                TextLayer::Empty => String::new(),
            })
            .collect::<String>();
        let converted = convert_djvu(&mut reader, &DjvuOptions::default().embed_images(false))?;
        let written = strict_ooxml_write::write_package(
            &converted.document,
            None,
            &strict_ooxml_write::WriteOptions::default(),
        )?;
        let package = strict_ooxml_core::opc::Package::open_reader(
            written.bytes.as_slice(),
            &strict_ooxml_core::opc::OpenOptions::default(),
        )?;
        let xml = String::from_utf8(
            package.read_part(&strict_ooxml_core::part::PartId::new("/word/document.xml"))?,
        )?;
        let dom = roxmltree::Document::parse(&xml)?;
        let actual = dom
            .descendants()
            .filter(|n| n.tag_name().name() == "t")
            .filter_map(|n| n.text())
            .map(compact)
            .collect::<String>();
        let equal = original == actual;
        if !equal {
            failed += 1;
        }
        println!(
            "{}\t{}\t{}\t{}\t{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            converted.report.paragraphs(),
            original.chars().count(),
            actual.chars().count(),
            if equal { "OK" } else { "FAIL" }
        );
    }
    if files == 0 || failed > 0 {
        return Err(format!("{failed}/{files} conversions lost native text").into());
    }
    Ok(())
}
