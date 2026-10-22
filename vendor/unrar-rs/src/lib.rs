#![forbid(unsafe_code)]

//! A pure Rust port of the UnRAR extraction library.
//!
//! The crate parses RAR 5 and RAR 1.5 - 4.x archives. It implements the RAR 5
//! decompressor (including filters, BLAKE2sp and AES-256 encryption), the RAR
//! 3.x LZ decompressor and AES-128 file encryption without any C or C++
//! dependencies.
//!
//! RAR 3.x PPM-compressed blocks are not implemented; entries that need them
//! return [`Error::Unsupported`]. RAR 2.x / 1.5 compressed data is also
//! unsupported (stored files are fine). See `README.md` for details.
//!
//! ```no_run
//! use unrar_rs::Archive;
//!
//! # fn main() -> unrar_rs::Result<()> {
//! let mut archive = Archive::open("example.rar")?;
//! for entry in &archive.entries {
//!     println!("{} ({} bytes)", entry.name, entry.unpacked_size);
//! }
//! archive.extract_all("output")?;
//! # Ok(())
//! # }
//! ```
//!
//! # UnRAR license
//!
//! UnRAR source code may be used in any software to handle RAR archives
//! without limitations free of charge, but cannot be used to develop a RAR
//! (WinRAR) compatible archiver and to re-create the RAR compression
//! algorithm, which is proprietary. Distribution of modified UnRAR source
//! code in separate form or as a part of other software is permitted,
//! provided that the full license text from `license.txt` is included in the
//! documentation or source comments of the resulting package.

pub mod archive;
pub mod bit_input;
pub mod crypto;
pub mod error;
pub mod format;
pub mod hash;
pub mod huffman;
pub mod unpack;

pub use archive::{Archive, ArchiveOptions};
pub use error::{Error, Result};
pub use format::{
    ArchiveInfo, DataSegment, Entry, FileCrypt, Format, HostOs, Redirect, RedirectKind, UnixOwner,
    DEFAULT_MAX_SFX_SCAN,
};
