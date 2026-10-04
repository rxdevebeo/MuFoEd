//! Scan a directory of foreign `.docx` for open / parse / write problems.
//!
//! ```text
//! cargo +1.92.0 run -p strict-ooxml --features write,svg --example corpus_scan -- testdata/CC0
//! cargo +1.92.0 run -p strict-ooxml --features write,svg --example corpus_scan -- testdata/CC0 --render
//! ```
//!
//! Same spirit as `strict-ooxml-pdf/examples/corpus_report.rs`: a manual instrument
//! over files we do not own. Permanent guards belong in tests once a defect is
//! reduced to a fixture.

#![allow(clippy::print_stdout, clippy::print_stderr, missing_docs)]
#![allow(
    clippy::cast_precision_loss,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use strict_ooxml_core::normalize::TransitionalNormalizer;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{verify_no_silent_loss, write_package, WriteOptions};

fn usage() -> ! {
    eprintln!(
        "usage: cargo run -p strict-ooxml --features write[,svg] --example corpus_scan -- <dir> [--render]"
    );
    std::process::exit(2);
}

fn corpus(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[derive(Debug, Default)]
struct Finding {
    stage: &'static str,
    detail: String,
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn scan_one(path: &Path, render: bool) -> Result<Vec<Finding>, String> {
    let name = file_label(path);
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scan_bytes(&name, &bytes, render)
    }));
    match result {
        Ok(Ok(findings)) => Ok(findings),
        Ok(Err(detail)) => Ok(vec![Finding {
            stage: "error",
            detail,
        }]),
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic".to_owned()
            };
            Ok(vec![Finding {
                stage: "panic",
                detail: msg,
            }])
        }
    }
}

#[allow(clippy::too_many_lines)] // open → parse → write → reopen → gen2 fixed-point
fn scan_bytes(name: &str, bytes: &[u8], render: bool) -> Result<Vec<Finding>, String> {
    let mut findings = Vec::new();
    let normalizer = Arc::new(TransitionalNormalizer::new());
    let options = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer.clone());

    let package = Package::open_reader(bytes, &options).map_err(|e| format!("open: {e}"))?;

    for part in package.parts() {
        if let Err(error) = package.read_part(&part.id) {
            findings.push(Finding {
                stage: "read_part",
                detail: format!("{}: {error}", part.id),
            });
        }
    }

    let document = match parse_document(&package, &ParseOptions::default()) {
        Ok(document) => document,
        Err(error) => {
            findings.push(Finding {
                stage: "parse",
                detail: error.to_string(),
            });
            return Ok(findings);
        }
    };

    if render {
        #[cfg(feature = "svg")]
        {
            match strict_ooxml_render_svg::render_with_media(
                &document,
                &strict_ooxml_render_svg::RenderOptions::default()
                    .pages(strict_ooxml_render_svg::PageSelection::Range { start: 1, end: 1 }),
                Some(&package),
            ) {
                Ok(_) => {}
                Err(error) => findings.push(Finding {
                    stage: "render_svg",
                    detail: error.to_string(),
                }),
            }
        }
        #[cfg(not(feature = "svg"))]
        {
            findings.push(Finding {
                stage: "render_svg",
                detail: "rebuild with --features write,svg".to_owned(),
            });
        }
    }

    let written = match write_package(&document, Some(&package), &WriteOptions::default()) {
        Ok(output) => {
            if let Err(error) = verify_no_silent_loss(&output.report) {
                findings.push(Finding {
                    stage: "silent_loss",
                    detail: error,
                });
            }
            output.bytes
        }
        Err(error) => {
            findings.push(Finding {
                stage: "write",
                detail: error.to_string(),
            });
            return Ok(findings);
        }
    };

    let normalizer2 = Arc::new(TransitionalNormalizer::new());
    let options2 = OpenOptions::default()
        .conformance(ConformancePolicy::Normalize)
        .shared_normalization(normalizer2.clone());
    let reopened = match Package::open_reader(&written[..], &options2) {
        Ok(package) => package,
        Err(error) => {
            findings.push(Finding {
                stage: "reopen_written",
                detail: error.to_string(),
            });
            return Ok(findings);
        }
    };

    // Touch every part so the second-pass report covers the whole package.
    for part in reopened.parts() {
        let _ = reopened.read_part(&part.id);
    }
    let report = normalizer2.report();
    if !report.is_noop() {
        findings.push(Finding {
            stage: "second_normalize",
            detail: format!("written package still needs normalization:\n{report}"),
        });
    }

    let reparsed = match parse_document(&reopened, &ParseOptions::default()) {
        Ok(document) => document,
        Err(error) => {
            findings.push(Finding {
                stage: "reparse_written",
                detail: error.to_string(),
            });
            return Ok(findings);
        }
    };

    let second = match write_package(&reparsed, Some(&reopened), &WriteOptions::default()) {
        Ok(output) => output.bytes,
        Err(error) => {
            findings.push(Finding {
                stage: "write_gen2",
                detail: error.to_string(),
            });
            return Ok(findings);
        }
    };

    if written.len() != second.len() || written != second {
        let parts = differing_parts(&written, &second);
        findings.push(Finding {
            stage: "fixed_point",
            detail: format!(
                "{} vs {} bytes; differing parts: {parts:?}",
                written.len(),
                second.len()
            ),
        });
    }

    // Page / block counts for "nothing disappears" style checks.
    let blocks_in = document.body.blocks.len();
    let blocks_out = reparsed.body.blocks.len();
    if blocks_out < blocks_in {
        findings.push(Finding {
            stage: "blocks_shrink",
            detail: format!("{blocks_in} -> {blocks_out} body blocks ({name})"),
        });
    }

    Ok(findings)
}

fn differing_parts(a: &[u8], b: &[u8]) -> Vec<String> {
    let open = |bytes: &[u8]| {
        Package::open_reader(
            bytes,
            &OpenOptions::default().conformance(ConformancePolicy::Permissive),
        )
        .ok()
    };
    let (Some(pa), Some(pb)) = (open(a), open(b)) else {
        return vec!["<package open failed>".to_owned()];
    };
    let mut names = BTreeMap::<String, (Option<usize>, Option<usize>)>::new();
    for part in pa.parts() {
        let len = pa.read_part(&part.id).ok().map(|b| b.len());
        names.insert(part.id.as_str().to_owned(), (len, None));
    }
    for part in pb.parts() {
        let len = pb.read_part(&part.id).ok().map(|b| b.len());
        names
            .entry(part.id.as_str().to_owned())
            .and_modify(|entry| entry.1 = len)
            .or_insert((None, len));
    }
    names
        .into_iter()
        .filter_map(|(name, (a, b))| {
            if a == b {
                let (Some(la), Some(_)) = (a, b) else {
                    return None;
                };
                let ba = pa.read_part(&PartId::new(name.as_str())).ok()?;
                let bb = pb.read_part(&PartId::new(name.as_str())).ok()?;
                if ba == bb {
                    None
                } else {
                    Some(format!("{name}: content differs ({la} bytes)"))
                }
            } else {
                Some(format!("{name}: {a:?} -> {b:?}"))
            }
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let render = args.iter().any(|a| a == "--render");
    args.retain(|a| a != "--render");
    let Some(dir) = args.first().map(PathBuf::from) else {
        usage();
    };
    if !dir.is_dir() {
        eprintln!("not a directory: {}", dir.display());
        std::process::exit(2);
    }

    let files = corpus(&dir);
    eprintln!("scanning {} .docx under {}", files.len(), dir.display());

    let summary: Arc<Mutex<BTreeMap<&'static str, usize>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let mut problem_files = 0usize;
    let started = Instant::now();

    for (index, path) in files.iter().enumerate() {
        let name = file_label(path);
        let file_started = Instant::now();
        let findings = scan_one(path, render).unwrap_or_else(|e| {
            vec![Finding {
                stage: "scan",
                detail: e,
            }]
        });
        let ms = file_started.elapsed().as_millis();
        if findings.is_empty() {
            println!("OK\t{name}\t{ms}ms");
        } else {
            problem_files += 1;
            for finding in &findings {
                *summary
                    .lock()
                    .expect("summary")
                    .entry(finding.stage)
                    .or_default() += 1;
                let detail = finding.detail.replace('\n', " | ");
                println!("FAIL\t{name}\t{}\t{detail}\t{ms}ms", finding.stage);
            }
        }
        if (index + 1) % 10 == 0 {
            eprintln!("… {}/{}", index + 1, files.len());
        }
    }

    eprintln!();
    eprintln!(
        "done: {} files, {} with findings, {:.1}s",
        files.len(),
        problem_files,
        started.elapsed().as_secs_f64()
    );
    let summary = summary.lock().expect("summary");
    if !summary.is_empty() {
        eprintln!("by stage:");
        for (stage, count) in summary.iter() {
            eprintln!("  {stage}: {count}");
        }
    }
}
