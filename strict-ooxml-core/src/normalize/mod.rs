//! Extension point for raw-level Transitional → Strict normalization.
//!
//! Stage 1 defines the [`RawNormalizer`] seam that
//! [`Package::open_reader`](crate::opc::Package::open_reader) calls between OPC
//! parsing and namespace resolution. The T1-T8 transformation stages are
//! implemented in Stage 6 (`TZ-STRICT-OOXML-RUST.md` §10; stage task S1.13).
//!
//! [`report`] carries what normalization changed and what it cost, and is the
//! basis of criterion SC-4 (no silent loss).

pub mod mce;
pub mod report;
pub mod tables;
pub mod transitional;
pub mod vml;

use std::borrow::Cow;

use crate::error::Result;
use crate::part::PartId;

pub use report::{LossRecord, NormalizationReport, Severity, TransformRecord};
pub use transitional::{
    DirectionPolicy, InvariantMode, McePolicy, NormalizerOptions, TransitionalNormalizer,
};

/// A raw-layer transformation applied to each part before namespace resolution.
pub trait RawNormalizer: Send + Sync {
    /// Transforms the raw bytes of a part.
    ///
    /// Returning `Cow::Borrowed(bytes)` means "no change". The default Stage-1
    /// implementation ([`NoopNormalizer`]) does exactly that.
    ///
    /// # Errors
    ///
    /// Implementations return a [`StrictError`](crate::error::StrictError) when
    /// normalization cannot proceed.
    fn normalize_part<'a>(&self, part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>>;

    /// Records that the main document's content type was not one of the
    /// `WordprocessingML` main/template (macro-enabled) MIME types (AUD-26).
    ///
    /// Called under `Normalize`/`Permissive` instead of failing the open; the
    /// default is a no-op so a bare [`NoopNormalizer`] stays silent.
    fn note_unexpected_main_content_type(&self, part: &PartId, content_type: &str) {
        let _ = (part, content_type);
    }

    /// The accumulated [`NormalizationReport`], if this normalizer keeps one.
    ///
    /// Default `None` so a bare [`NoopNormalizer`] stays silent. AUD-31 /
    /// ADR-0017: [`TransitionalNormalizer`] returns the per-part merge.
    fn report(&self) -> Option<NormalizationReport> {
        None
    }
}

/// A [`RawNormalizer`] that leaves parts unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopNormalizer;

impl RawNormalizer for NoopNormalizer {
    fn normalize_part<'a>(&self, _part: &PartId, bytes: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        Ok(Cow::Borrowed(bytes))
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{NoopNormalizer, RawNormalizer};
    use crate::part::PartId;

    #[test]
    fn noop_normalizer_borrows_input_unchanged() {
        let normalized = NoopNormalizer
            .normalize_part(&PartId::new("/word/document.xml"), b"abc")
            .unwrap();
        assert!(matches!(normalized, Cow::Borrowed(_)));
        assert_eq!(&*normalized, b"abc");
    }
}
