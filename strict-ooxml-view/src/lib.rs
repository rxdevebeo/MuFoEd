//! Library surface of the local viewer.
//!
//! The binary serves HTTP; integration tests (AUD-87) call [`discover`] and
//! [`render`] directly so the Strict corpus can be opened without a socket.

#![deny(missing_docs)]

mod catalog;

pub use catalog::{discover, failed, render, Cache, DocumentView, Entry, Rendered, Summary};
