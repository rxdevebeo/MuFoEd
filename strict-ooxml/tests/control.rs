//! Progress and cancellation of an open and a render (`strict_ooxml_core::control`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::io::Cursor;
use std::thread;
use std::time::{Duration, Instant};

use strict_ooxml::{
    CancelReason, OpenControl, OpenOptions, RenderOptions, Stage, StrictDocument, StrictError,
};
use strict_ooxml_testkit::DocxBuilder;

/// A Strict package whose body is `paragraphs` short paragraphs.
fn document(paragraphs: usize) -> Vec<u8> {
    let body = "<w:p><w:r><w:t>Lorem ipsum dolor sit amet.</w:t></w:r></w:p>".repeat(paragraphs);
    DocxBuilder::strict().body(&body).deflated().build()
}

fn open(bytes: Vec<u8>, control: &OpenControl) -> Result<StrictDocument, StrictError> {
    StrictDocument::open_reader(
        Cursor::new(bytes),
        &OpenOptions::default().control(control.clone()),
    )
}

fn cancelled_because(result: Result<StrictDocument, StrictError>) -> CancelReason {
    match result {
        Err(StrictError::Cancelled { reason }) => reason,
        Err(other) => panic!("expected Cancelled, got {other}"),
        Ok(_) => panic!("expected Cancelled, the open finished"),
    }
}

#[test]
fn an_open_with_a_control_finishes_and_says_so() {
    let control = OpenControl::new();
    let doc = open(document(50), &control).expect("open");
    assert_eq!(doc.document().body.blocks.len(), 50);
    assert_eq!(control.progress().stage, Stage::Finished);
}

#[test]
fn a_control_cancelled_before_the_open_stops_it() {
    let control = OpenControl::new();
    control.cancel();
    assert_eq!(
        cancelled_because(open(document(10), &control)),
        CancelReason::Requested
    );
}

#[test]
fn a_deadline_already_passed_stops_the_open() {
    let control = OpenControl::with_deadline(Instant::now());
    assert_eq!(
        cancelled_because(open(document(10), &control)),
        CancelReason::Deadline
    );
}

/// The case the feature exists for: a large document, watched from another
/// thread, stopped in the middle of parsing.
#[test]
fn a_large_open_is_watched_and_stopped_from_another_thread() {
    let bytes = document(200_000);
    let control = OpenControl::new();
    let watcher = {
        let control = control.clone();
        thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(60) {
                let progress = control.progress();
                if progress.stage == Stage::ParsingDocument && progress.done > 0 {
                    assert!(progress.total >= progress.done, "{progress:?}");
                    control.cancel();
                    return Some(progress);
                }
                if progress.stage == Stage::Finished {
                    return None;
                }
                thread::sleep(Duration::from_millis(1));
            }
            None
        })
    };
    let result = open(bytes, &control);
    let seen = watcher.join().expect("watcher");
    let progress = seen.expect("the watcher saw the main part being parsed");
    assert!(progress.done < progress.total, "{progress:?}");
    assert_eq!(cancelled_because(result), CancelReason::Requested);
}

#[test]
fn a_render_with_a_cancelled_control_stops() {
    let doc = open(document(20), &OpenControl::new()).expect("open");
    let control = OpenControl::new();
    let pages = doc
        .render_svg(&RenderOptions::default().control(control.clone()))
        .expect("render");
    assert!(!pages.is_empty());
    assert_eq!(control.progress().stage, Stage::Finished);

    control.cancel();
    let error = doc
        .render_svg(&RenderOptions::default().control(control))
        .expect_err("cancelled");
    assert!(
        matches!(
            error,
            StrictError::Cancelled {
                reason: CancelReason::Requested
            }
        ),
        "{error}"
    );
}

#[test]
fn without_a_control_nothing_changes() {
    let doc = StrictDocument::open_reader(Cursor::new(document(5)), &OpenOptions::default())
        .expect("open");
    assert!(!doc
        .render_svg(&RenderOptions::default())
        .expect("render")
        .is_empty());
}
