//! Audit of the default [`ResourceLimits`] against the corpora
//! (`REWORK-CORE-LIMITS.md` §3).
//!
//! Opens every `.docx` in `tests/samples/` (versioned) and `tests/docx/`
//! (local) permissively and checks that **no** limit is exceeded, while
//! recording the maximum value observed for each structural limit so the
//! report can state the headroom.

use std::path::{Path, PathBuf};

use strict_ooxml_core::error::StrictError;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::normalize::NoopNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};

/// Collects `.docx` files from a corpus directory (sorted).
fn docx_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    files
}

/// Max XML element depth and max text-node length, by a byte scan.
fn scan_xml(bytes: &[u8]) -> (u32, usize) {
    let mut depth: u32 = 0;
    let mut max_depth: u32 = 0;
    let mut max_text: usize = 0;
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'<' {
            let start = i;
            let mut j = i + 1;
            while j < n && bytes[j] != b'>' {
                j += 1;
            }
            if j >= n {
                break;
            }
            let inner = &bytes[start + 1..j];
            if inner.first() == Some(&b'/') {
                depth = depth.saturating_sub(1);
            } else if matches!(inner.first(), Some(b'!' | b'?')) {
                // declaration / comment / CDATA
            } else {
                depth += 1;
                max_depth = max_depth.max(depth);
                if inner.last() == Some(&b'/') {
                    depth = depth.saturating_sub(1);
                }
            }
            i = j + 1;
        } else {
            let start = i;
            while i < n && bytes[i] != b'<' {
                i += 1;
            }
            max_text = max_text.max(i - start);
        }
    }
    (max_depth, max_text)
}

#[test]
fn corpus_stays_within_default_limits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // This audit is about resource limits, not conformance (AUD-23 /
    // ADR-0016): `Permissive` without a normalizer now refuses Transitional
    // and Mixed content, so a `Noop` normalizer is installed to open every
    // corpus file regardless of family without changing a single byte.
    let permissive = OpenOptions::default()
        .conformance(ConformancePolicy::Permissive)
        .normalization(NoopNormalizer);

    let mut max_compressed_input: u64 = 0;
    let mut max_single_uncompressed: u64 = 0;
    let mut max_total_uncompressed: u64 = 0;
    let mut max_parts: usize = 0;
    let mut max_xml_depth: u32 = 0;
    let mut max_text_len: usize = 0;
    let mut checked = 0u32;

    for dir in [root.join("tests/samples"), root.join("tests/docx")] {
        for path in docx_files(&dir) {
            let file_size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
            max_compressed_input = max_compressed_input.max(file_size);

            match Package::open_path(&path, &permissive) {
                Ok(package) => {
                    let parts: Vec<_> = package.parts().collect();
                    max_parts = max_parts.max(parts.len());
                    let total: u64 = parts.iter().map(|part| part.uncompressed_size).sum();
                    max_total_uncompressed = max_total_uncompressed.max(total);
                    for part in &parts {
                        max_single_uncompressed =
                            max_single_uncompressed.max(part.uncompressed_size);
                    }
                    if let Ok(main) = package.main_document_part() {
                        let main = main.clone();
                        if let Ok(bytes) = package.read_part(&main) {
                            let (depth, text) = scan_xml(&bytes);
                            max_xml_depth = max_xml_depth.max(depth);
                            max_text_len = max_text_len.max(text);
                        }
                    }
                    checked += 1;
                }
                Err(error @ StrictError::LimitExceeded { .. }) => {
                    panic!("{}: limit exceeded under defaults: {error}", path.display());
                }
                Err(_) => {
                    // Non-limit failure (for example a damaged local fixture);
                    // the corpus tests assert those separately.
                }
            }
        }
    }

    let limits = ResourceLimits::default();
    eprintln!("limits-audit ({checked} files opened):");
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_compressed_input", limits.max_compressed_input, max_compressed_input
    );
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_single_uncompressed", limits.max_single_uncompressed, max_single_uncompressed
    );
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_total_uncompressed", limits.max_total_uncompressed, max_total_uncompressed
    );
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_parts", limits.max_parts, max_parts
    );
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_xml_depth", limits.max_xml_depth, max_xml_depth
    );
    eprintln!(
        "  {:<26} default={:<12} observed={}",
        "max_text_len", limits.max_text_len, max_text_len
    );

    assert!(checked > 0, "no corpus files were opened");
    assert!(max_compressed_input <= limits.max_compressed_input);
    assert!(max_single_uncompressed <= limits.max_single_uncompressed);
    assert!(max_total_uncompressed <= limits.max_total_uncompressed);
    assert!(max_parts <= limits.max_parts);
    assert!(u64::from(max_xml_depth) <= u64::from(limits.max_xml_depth));
    assert!(max_text_len <= limits.max_text_len);
}
