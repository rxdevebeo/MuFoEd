//! Fixture builders shared by the reader's integration tests.
//!
//! Each `tests/*.rs` is its own crate, so a helper has to live in a module
//! rather than in one of them - `tests/common/mod.rs` is the only place both can
//! reach.

use std::fmt::Write as _;

/// Builds a one-page PDF from raw objects.
///
/// The xref is assembled and then written, so the offsets are real rather than
/// a placeholder: a fixture whose xref is broken would test the reader's
/// recovery path instead of the thing it exists for.
pub(crate) fn pdf_with(objects: &[&str]) -> Vec<u8> {
    let owned: Vec<String> = objects.iter().map(|object| (*object).to_owned()).collect();
    pdf_from(&owned)
}

/// The same, from owned objects.
pub(crate) fn pdf_from(objects: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut body = String::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        let _ = writeln!(body, "{} 0 obj\n{object}\nendobj\n", index + 1);
    }
    out.extend_from_slice(body.as_bytes());
    let mut table = String::new();
    let _ = writeln!(table, "xref\n0 {}\n0000000000 65535 f ", objects.len() + 1);
    for offset in &offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 99 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n");
    out
}
