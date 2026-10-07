//! TEMPORARY diagnostic (hardening 2026-10-07): the smallest thread stack each
//! pipeline stage needs for twelve nested tables, in this build profile.
//!
//! A stack overflow aborts the process, so each measurement runs in a child
//! process (this test binary, re-entered through `stack_probe_child`) and the
//! parent binary-searches the size from the child's exit status. Results go to
//! the real stderr (not the captured one) as `STACK-PROBE` lines. The test
//! always passes; it is removed once the numbers are in.
#![cfg(all(feature = "write", feature = "pdf"))]

use std::io::{Cursor, Write as _};
use std::process::Command;

use strict_ooxml::{OpenOptions, StrictDocument};
use strict_ooxml_testkit::xml::nested_tables;
use strict_ooxml_testkit::DocxBuilder;

const STAGE: &str = "STRICT_OOXML_STACK_PROBE_STAGE";
const SIZE: &str = "STRICT_OOXML_STACK_PROBE_SIZE";

fn document() -> StrictDocument {
    let bytes = DocxBuilder::strict()
        .body(&nested_tables(12, "<w:p><w:r><w:t>x</w:t></w:r></w:p>"))
        .build();
    StrictDocument::open_reader(Cursor::new(bytes), &OpenOptions::default()).expect("open")
}

#[test]
fn stack_probe_child() {
    let (Ok(stage), Ok(size)) = (std::env::var(STAGE), std::env::var(SIZE)) else {
        return;
    };
    let size: usize = size.parse().expect("size");
    let handle = std::thread::Builder::new()
        .stack_size(size)
        .spawn(move || match stage.as_str() {
            "open" => {
                let _ = document();
            }
            other => {
                let document = std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn(document)
                    .expect("spawn")
                    .join()
                    .expect("open");
                match other {
                    "svg" => {
                        let _ = document.render_svg(&strict_ooxml::RenderOptions::default());
                    }
                    "write" => {
                        let _ = strict_ooxml::write_package(
                            document.document(),
                            Some(document.package()),
                            &strict_ooxml::WriteOptions::default(),
                        );
                    }
                    "pdf" => {
                        let _ = document.render_pdf(&strict_ooxml::RenderOptions::default());
                    }
                    _ => panic!("unknown stage"),
                }
            }
        })
        .expect("spawn");
    handle.join().expect("stage");
}

fn survives(stage: &str, size: usize) -> bool {
    Command::new(std::env::current_exe().expect("exe"))
        .args([
            "stack_probe_child",
            "--exact",
            "--test-threads=1",
            "--quiet",
        ])
        .env(STAGE, stage)
        .env(SIZE, size.to_string())
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn stack_probe_report() {
    if std::env::var(STAGE).is_ok() {
        return;
    }
    let mut err = std::io::stderr();
    for stage in ["open", "svg", "write", "pdf"] {
        let (mut low, mut high) = (16 * 1024_usize, 32 << 20);
        if !survives(stage, high) {
            let _ = writeln!(err, "STACK-PROBE stage={stage} fails even at {high} bytes");
            continue;
        }
        while high - low > 8 * 1024 {
            let mid = low + (high - low) / 2;
            if survives(stage, mid) {
                high = mid;
            } else {
                low = mid;
            }
        }
        let _ = writeln!(
            err,
            "STACK-PROBE stage={stage} needs <= {} KiB",
            high / 1024
        );
    }
}
