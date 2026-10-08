//! Library surface of the local viewer.
//!
//! The binary serves HTTP; integration tests (AUD-87) call [`discover`] and
//! [`render`] directly so the Strict corpus can be opened without a socket.
// Never-crash (docs/WORDCRAFT_ADOPTION_2026-10-07.md §4.2): library paths
// return errors or degrade with a report; tests may still unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]
#![deny(missing_docs)]

mod catalog;

pub use catalog::{
    discover, failed, render, Cache, DocumentView, Entry, PipelineView, Rendered, StageStatus,
    Summary, ViewIssue,
};
