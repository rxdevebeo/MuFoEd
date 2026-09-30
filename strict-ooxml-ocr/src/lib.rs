//! What a recogniser can be asked to do.
//!
//! A PDF says where the ink is. A *scanned* page has ink and no text layer, and
//! a figure is a picture whose meaning is nowhere in the file. Both are things
//! only a model can supply, and both are things a converter must not guess at:
//! invented text in a document is worse than a recorded gap, because nobody can
//! find it later.
//!
//! So the capability is a trait, the transport is a feature, and every answer
//! that reaches a document is marked as recovered. A caller that has no model
//! running gets a report that says what was missing and why nothing was done
//! about it - which is a usable answer, and the only honest one.
//!
//! # What this crate does *not* do
//!
//! It does not rasterise a page. Turning a PDF page into an image needs a
//! rasteriser (a `raster` feature on the reader's side, `STAGE-8-OPEN.md` O-3),
//! and until one exists a *page* cannot be sent to a model - only the images a
//! page already carries. That is why [`FigureClassifier`] is usable today and
//! [`TextRecovery`] is a trait with no producer: see `O-3` and `O-11`.

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::too_many_lines
)]

pub mod base64;
#[cfg(feature = "ocr-ollama")]
pub mod ollama;
pub mod report;
pub mod traits;

pub use report::{OcrLoss, OcrReport, Severity};
pub use traits::{FigureClassifier, Recovered, TextRecovery, VisionError};

/// A PNG of a page or a region, ready to be sent to a model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The encoded bytes.
    pub bytes: Vec<u8>,
    /// What the bytes are, for the report and for a caller that has to log it.
    pub encoding: Encoding,
}

/// How an [`Image`] is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
}

impl Encoding {
    /// The name ollama's `images` field expects.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }

    /// The media type.
    #[must_use]
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

impl Image {
    /// A PNG image.
    #[must_use]
    pub fn png(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            encoding: Encoding::Png,
        }
    }

    /// A JPEG image.
    #[must_use]
    pub fn jpeg(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            encoding: Encoding::Jpeg,
        }
    }

    /// The image as base64, which is the only form ollama's API accepts.
    #[must_use]
    pub fn to_base64(&self) -> String {
        base64::encode(&self.bytes)
    }
}

/// A response limit the client enforces before it reads a body.
///
/// Without one, a model that answers with a hundred megabytes of text is a
/// memory exhaustion bug in the caller, and a local daemon is not a trusted
/// source the way a file the user picked is.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// A wall-clock limit on one request.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[cfg(test)]
mod tests {
    use super::{Encoding, Image};

    #[test]
    fn an_image_knows_its_own_encoding() {
        let image = Image::png(vec![0x89, b'P', b'N', b'G']);
        assert_eq!(image.encoding, Encoding::Png);
        assert_eq!(image.encoding.mime(), "image/png");
        assert_eq!(Image::jpeg(Vec::new()).encoding.as_str(), "jpeg");
    }

    #[test]
    fn base64_matches_the_reference_alphabet() {
        // "PNG" -> "UE5H", the encoding every other implementation agrees on.
        assert_eq!(Image::png(b"PNG".to_vec()).to_base64(), "UE5H");
    }
}
