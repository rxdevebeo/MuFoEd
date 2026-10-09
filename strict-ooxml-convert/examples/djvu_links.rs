//! Print how many hyperlinks a DjVu file converts to, for the SC-D9 oracle.
//!
//! The ant driver compares this count with `djvused`'s `maparea` total. It reads
//! the converted model, not the written package; a separate writer test proves
//! the writer emits the external relationship.
#![allow(clippy::doc_markdown)]
use strict_ooxml_convert::{convert_djvu, DjvuOptions};
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
use strict_ooxml_wml::model::{block::Block, inline::Inline};

fn count_blocks(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => count_inlines(&paragraph.inlines),
            Block::Table(table) => table
                .rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .map(|cell| count_blocks(&cell.blocks))
                .sum(),
            Block::SdtBlock(sdt) => count_blocks(&sdt.blocks) + count_inlines(&sdt.inlines),
            _ => 0,
        })
        .sum()
}

fn count_inlines(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Hyperlink(link) => 1 + count_inlines(&link.inlines),
            Inline::Field(field) => count_inlines(&field.inlines),
            Inline::SdtInline(sdt) => count_blocks(&sdt.blocks) + count_inlines(&sdt.inlines),
            Inline::Directional(dir) => count_inlines(&dir.inlines),
            _ => 0,
        })
        .sum()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("requires a DjVu path")?;
    let bytes = std::fs::read(&path)?;
    let mut reader = DjvuDocument::open(&bytes, DjvuLimits::default())?;
    let converted = convert_djvu(&mut reader, &DjvuOptions::default().embed_images(false))?;
    println!("{}", count_blocks(&converted.document.body.blocks));
    Ok(())
}
