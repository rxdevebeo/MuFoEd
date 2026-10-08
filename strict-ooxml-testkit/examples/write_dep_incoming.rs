//! Writes dependency-provocation fixtures into `strict-ooxml-core/tests/docx-incoming/`.
//!
//! These files are gitignored (local quarantine, same as the rest of
//! `docx-incoming/`). Regenerate after changing the cases:
//!
//! ```text
//! cargo +1.92.0 run -p strict-ooxml-testkit --example write_dep_incoming
//! ```
//!
//! Plan tasks: AUD-95…AUD-99 in `REWORK-AUDIT-2026-10.md`; index in
//! `docs/corpus-incoming.md` §«Зависимости».

#![allow(
    missing_docs,
    clippy::format_push_string,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

use std::fs;
use std::path::PathBuf;

use strict_ooxml_testkit::{DocxBuilder, PdfBuilder};

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/docx-incoming")
}

fn write(name: &str, bytes: &[u8]) {
    let path = out_dir().join(name);
    fs::write(&path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
}

fn main() {
    let dir = out_dir();
    fs::create_dir_all(&dir).expect("docx-incoming");

    // --- quick-xml / our XmlReader wrapper (RUSTSEC-2026-0194/0195, #977/#980) ---

    let mut many_attrs = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p"#,
    );
    for i in 0..600 {
        many_attrs.push_str(&format!(r#" w:a{i}="{i}""#));
    }
    many_attrs.push_str("><w:r><w:t>many-attrs</w:t></w:r></w:p></w:body></w:document>");
    write(
        "dep-quickxml-many-attrs.docx",
        &DocxBuilder::strict()
            .document_bytes(many_attrs.into_bytes())
            .build(),
    );

    let mut xmlns_bomb =
        String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document"#);
    for i in 0..40 {
        xmlns_bomb.push_str(&format!(r#" xmlns:p{i}="urn:dep:{i}""#));
    }
    xmlns_bomb.push_str(
        r#" xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:r><w:t>xmlns</w:t></w:r></w:p></w:body></w:document>"#,
    );
    write(
        "dep-quickxml-xmlns-bomb.docx",
        &DocxBuilder::strict()
            .document_bytes(xmlns_bomb.into_bytes())
            .build(),
    );

    let depth = 40u32;
    let mut deep = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body>"#,
    );
    for i in 0..depth {
        deep.push_str(&format!(
            r#"<w:p xmlns:n{i}="urn:n{i}"><w:r><w:t>d{i}</w:t></w:r>"#
        ));
        // nest via customXml-like wrappers that stay well-formed WML-ish XML
        deep.push_str(&format!(r#"<w:customXml xmlns:n{i}="urn:n{i}">"#));
    }
    for _ in 0..depth {
        deep.push_str("</w:customXml>");
    }
    for _ in 0..depth {
        deep.push_str("</w:p>");
    }
    deep.push_str("</w:body></w:document>");
    write(
        "dep-quickxml-deep-ns.docx",
        &DocxBuilder::strict()
            .document_bytes(deep.into_bytes())
            .build(),
    );

    write(
        "dep-quickxml-doctype.docx",
        &DocxBuilder::strict()
            .document_bytes(
                br#"<?xml version="1.0"?><!DOCTYPE w:document [ <!ENTITY xxe SYSTEM "file:///etc/passwd"> ]><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:r><w:t>&xxe;</w:t></w:r></w:p></w:body></w:document>"#.as_slice(),
            )
            .build(),
    );

    write(
        "dep-quickxml-custom-entity.docx",
        &DocxBuilder::strict()
            .document_bytes(
                br#"<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:r><w:t>&bogus;</w:t></w:r></w:p></w:body></w:document>"#.as_slice(),
            )
            .build(),
    );

    write(
        "dep-quickxml-dup-attr.docx",
        &DocxBuilder::strict()
            .document_bytes(
                br#"<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p w:val="1" w:val="2"><w:r><w:t>dup</w:t></w:r></w:p></w:body></w:document>"#.as_slice(),
            )
            .build(),
    );

    // --- hayro (AUD-95…AUD-97): PDF fixtures next to the OOXML quarantine ---

    // Dictionary-level absurd JBIG2 image (hayro#1259 family / AUD-95).
    let mut jbig2 = PdfBuilder::new().media_box([0, 0, 40, 40]);
    let image = jbig2.stream(
        "/Type /XObject /Subtype /Image /Width 4294967295 /Height 2 \
         /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode",
        b"XXXX",
        false,
    );
    jbig2.page_with(
        b"q 20 0 0 20 5 5 cm /Im0 Do Q",
        &format!("<< /XObject << /Im0 {image} 0 R >> >>"),
    );
    write("dep-hayro-jbig2-absurd.pdf", &jbig2.build());

    // Self-referencing tiling pattern (AUD-88 residual / AUD-96 nesting family).
    let tiling = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R \
/Resources << /Pattern << /P1 5 0 R >> >> >> endobj\n\
4 0 obj << /Length 30 >> stream\n\
/Pattern cs /P1 scn 0 0 40 40 re f\n\
endstream endobj\n\
5 0 obj << /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 \
/BBox [0 0 10 10] /XStep 10 /YStep 10 /Length 28 >> stream\n\
/Pattern cs /P1 scn 0 0 10 10 re f\n\
endstream endobj\n\
trailer << /Root 1 0 R >>\n\
%%EOF\n";
    write("dep-hayro-tiling-self.pdf", tiling);

    // Absurd inline image width (AUD-88 / AUD-95 sibling).
    let mut inline = PdfBuilder::new().media_box([0, 0, 40, 40]);
    inline.page(
        b"q 20 0 0 20 5 5 cm BI /W 4294967295 /H 2 /BPC 8 /CS /G ID \0\xff\xff\0 EI Q \
1 0 0 rg 0 0 4 4 re f",
    );
    write("dep-hayro-inline-absurd.pdf", &inline.build());

    // Page-tree Kids cycle (AUD-88 / AUD-96).
    let kids_cycle = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >> endobj\n\
4 0 obj << /Type /Pages /Parent 3 0 R /Kids [3 0 R] /Count 1 >> endobj\n\
trailer << /Root 1 0 R >>\n\
%%EOF\n";
    write("dep-hayro-kids-cycle.pdf", kids_cycle);

    // Deep literal dictionary nesting in the trailer (AUD-96; sibling of pdq stack abort).
    let nest = 2_000usize;
    let mut deep_pdf = String::from("%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    deep_pdf.push_str("2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    deep_pdf.push_str(
        "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R >> endobj\n",
    );
    deep_pdf.push_str("4 0 obj << /Length 0 >> stream\nendstream endobj\n");
    deep_pdf.push_str("trailer << /Root 1 0 R /Info ");
    for _ in 0..nest {
        deep_pdf.push_str("<< /X ");
    }
    deep_pdf.push_str("null");
    for _ in 0..nest {
        deep_pdf.push_str(" >>");
    }
    deep_pdf.push_str(" >>\n%%EOF\n");
    write("dep-hayro-deep-dict.pdf", deep_pdf.as_bytes());

    // Huge CID /W range (AUD-88 residual).
    let mut cid = PdfBuilder::new().media_box([0, 0, 100, 40]);
    let desc = cid.object(
        "<< /Type /FontDescriptor /FontName /Helvetica /Flags 32 \
         /FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 >>",
    );
    let descendant = cid.object(format!(
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Helvetica \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor {desc} 0 R /W [0 4294967295 500] >>"
    ));
    let font = cid.object(format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Helvetica /Encoding /Identity-H \
         /DescendantFonts [{descendant} 0 R] >>"
    ));
    cid.page_with(
        b"BT /F1 12 Tf 10 10 Td <0041> Tj ET",
        &format!("<< /Font << /F1 {font} 0 R >> >>"),
    );
    write("dep-hayro-cid-huge-w.pdf", &cid.build());

    println!("done → {}", dir.display());
}
