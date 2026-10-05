//! Text preservation score for synthetic and corpus checks.
//!
//! The comparison is a Unicode scalar longest common subsequence. The only
//! normalization is `CRLF` to `LF`. Spaces stay. A zero `want` length has no
//! recall, and a zero `have` length has no precision.

#![allow(clippy::cast_precision_loss)]

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::PdfDocument;
use strict_ooxml_wml::model::block::Block;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::Document;

/// Matched scalars and the two lengths, after newline normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextScore {
    /// Length of the longest common subsequence.
    pub matched: usize,
    /// Scalars in the expected text.
    pub want_len: usize,
    /// Scalars in the actual text.
    pub have_len: usize,
}

impl TextScore {
    /// `matched / want_len`. `None` when `want` is empty: that is not a pass.
    pub(crate) fn recall(self) -> Option<f64> {
        if self.want_len == 0 {
            None
        } else {
            Some(self.matched as f64 / self.want_len as f64)
        }
    }

    /// `matched / have_len`. `None` when `have` is empty.
    pub(crate) fn precision(self) -> Option<f64> {
        if self.have_len == 0 {
            None
        } else {
            Some(self.matched as f64 / self.have_len as f64)
        }
    }
}

/// The instrument the audit found: the search index, not the number of matches.
///
/// `abc` against `Z` returns 1. Kept so a test can show the old answer and the
/// replacement side by side.
pub(crate) fn flawed_recall(want: &str, have: &str) -> f64 {
    let want: Vec<char> = want.chars().filter(|ch| !ch.is_whitespace()).collect();
    let have: Vec<char> = have.chars().filter(|ch| !ch.is_whitespace()).collect();
    if want.is_empty() {
        return 1.0;
    }
    let mut index = 0;
    for ch in &have {
        while index < want.len() && want[index] != *ch {
            index += 1;
        }
        if index < want.len() {
            index += 1;
        }
    }
    index as f64 / want.len() as f64
}

/// `CRLF` becomes `LF`. A bare `CR` is left as it is.
pub(crate) fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Score `want` against `have`.
pub(crate) fn score(want: &str, have: &str) -> TextScore {
    let want: Vec<char> = normalize_newlines(want).chars().collect();
    let have: Vec<char> = normalize_newlines(have).chars().collect();
    TextScore {
        matched: lcs_len(&want, &have),
        want_len: want.len(),
        have_len: have.len(),
    }
}

fn lcs_len(want: &[char], have: &[char]) -> usize {
    if want.is_empty() || have.is_empty() {
        return 0;
    }
    let mut previous = vec![0_usize; have.len() + 1];
    let mut current = vec![0_usize; have.len() + 1];
    for left in want {
        for (index, right) in have.iter().enumerate() {
            current[index + 1] = if left == right {
                previous[index] + 1
            } else {
                current[index].max(previous[index + 1])
            };
        }
        std::mem::swap(&mut previous, &mut current);
        current.fill(0);
    }
    previous[have.len()]
}

/// Mapped glyph text in drawing order.
///
/// Nothing is inserted between glyphs. `PdfPage::text` puts a space between
/// every glyph, and that joiner is not a character the document stores. A
/// space counts only when the glyph's own text is a space.
pub(crate) fn pdf_reading_text(
    document: &mut PdfDocument,
) -> Result<String, strict_ooxml_pdf::PdfError> {
    let mut out = String::new();
    for page in document.pages()? {
        for item in page.items() {
            let Item::Glyph(glyph) = item else {
                continue;
            };
            if glyph.mapped {
                out.push_str(&glyph.text);
            }
        }
    }
    Ok(out)
}

/// Body text, including tables and nested blocks. Headers are not included.
pub(crate) fn body_text(document: &Document) -> String {
    let mut out = String::new();
    push_blocks(&mut out, &document.body.blocks);
    out
}

/// Header and footer text, separate from the body.
pub(crate) fn header_footer_text(document: &Document) -> String {
    let mut out = String::new();
    for part in &document.headers_footers {
        push_blocks(&mut out, &part.blocks);
    }
    out
}

fn push_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                push_inlines(out, &paragraph.inlines);
                out.push('\n');
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        push_blocks(out, &cell.blocks);
                    }
                }
            }
            Block::SdtBlock(container) => push_blocks(out, &container.blocks),
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
}

fn push_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                for content in &run.content {
                    if let RunContent::Text(node) = content {
                        out.push_str(&node.text);
                    }
                }
            }
            Inline::Hyperlink(link) => push_inlines(out, &link.inlines),
            Inline::Field(field) => push_inlines(out, &field.inlines),
            Inline::SdtInline(container) => push_inlines(out, &container.inlines),
            _ => {}
        }
    }
}
