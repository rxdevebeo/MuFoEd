//! Images out of a PDF and into a `.docx`.
//!
//! The converter does not re-encode what it can carry. A JPEG goes in as the
//! bytes the PDF holds, because the PDF's filter *is* the codec and a
//! re-encode would differ from the document the PDF came from for no reason. A
//! raw-sample image is wrapped in a PNG, which is lossless for 8-bit components.

use std::sync::Arc;

use strict_ooxml_core::part::PartId;
use strict_ooxml_pdf::Encoded;
use strict_ooxml_wml::model::drawing::{MediaIndex, MediaItem, MediaKind};

/// A media part ready to be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaPart {
    /// Where it goes in the package.
    pub part: PartId,
    /// The bytes.
    pub bytes: Arc<Vec<u8>>,
    /// The file extension, which is also the content type.
    pub extension: &'static str,
    /// A stable identity, so the same image on two pages is stored once.
    pub fingerprint: String,
}

/// Collects the images a conversion needs, deduplicating by content.
#[derive(Debug, Default)]
pub(crate) struct MediaCollector {
    parts: Vec<MediaPart>,
    by_fingerprint: std::collections::BTreeMap<String, PartId>,
}

impl MediaCollector {
    /// An empty collector.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds an image, returning the part it was stored in.
    ///
    /// Returns `None` for an image that cannot be carried, with the reason as
    /// the second value so the caller can record it rather than swallow it.
    pub(crate) fn add(&mut self, index: usize, image: &Encoded) -> Result<PartId, &'static str> {
        let (bytes, extension) = match image {
            Encoded::Jpeg { data, .. } => (data.as_ref().clone(), "jpeg"),
            Encoded::Raw { .. } => {
                // The samples are laid out by the image itself; the PNG wrapper is
                // lossless for 8-bit components, so nothing is re-encoded twice.
                let png = image.to_png().map_err(|_| "raw samples are not a PNG")?;
                (png, "png")
            }
        };
        // The fingerprint is the length and a hash of the bytes: two copies of
        // the same picture deduplicate, and two different pictures that happen
        // to be the same size do not.
        let fingerprint = fingerprint(&bytes);
        if let Some(part) = self.by_fingerprint.get(&fingerprint) {
            return Ok(part.clone());
        }
        let number = self.parts.len() + 1;
        let part = PartId::new(format!("/word/media/image{number}.{extension}").as_str());
        self.by_fingerprint
            .insert(fingerprint.clone(), part.clone());
        self.parts.push(MediaPart {
            part: part.clone(),
            bytes: Arc::new(bytes),
            extension,
            fingerprint,
        });
        let _ = index;
        Ok(part)
    }

    /// The parts, in insertion order.
    #[must_use]
    /// The parts and their bytes, for a caller that is about to write them.
    pub(crate) fn into_parts(self) -> Vec<(PartId, Vec<u8>)> {
        self.parts
            .into_iter()
            .map(|part| (part.part, part.bytes.as_ref().clone()))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn parts(&self) -> &[MediaPart] {
        &self.parts
    }

    /// Registers the parts in the model's media index.
    pub(crate) fn register(&self, index: &mut MediaIndex) {
        for part in &self.parts {
            let kind = match part.extension {
                "png" => MediaKind::Png,
                "jpeg" => MediaKind::Jpeg,
                _ => MediaKind::Other,
            };
            index.insert(MediaItem {
                part: part.part.clone(),
                content_type: None,
                kind,
            });
        }
    }

    /// How many distinct images were stored.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.parts.len()
    }

    /// Whether anything was stored. The collector is empty until a page
    /// contributes an image, so this is a real condition the tests check.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// A cheap content fingerprint: the length, plus an FNV-1a pass over the
/// bytes.
///
/// A cryptographic hash would be the right tool for anything adversarial; this
/// only has to notice that a second copy of a picture is the same picture, and
/// an FNV-1a collision on two document images is not a security question.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}-{}", bytes.len(), hash)
}

#[cfg(test)]
mod tests {
    // The extension is the one *this* collector chose, so a case-insensitive
    // comparison would be testing the wrong thing.
    #![allow(clippy::case_sensitive_file_extension_comparisons)]

    use super::MediaCollector;
    use strict_ooxml_pdf::Encoded;

    fn jpeg(byte: u8) -> Encoded {
        Encoded::Jpeg {
            width: 2,
            height: 2,
            data: std::sync::Arc::new(vec![0xFF, 0xD8, byte, 0xD9]),
            alpha: None,
        }
    }

    fn raw() -> Encoded {
        Encoded::Raw {
            width: 2,
            height: 1,
            samples: std::sync::Arc::new(vec![255, 0, 0, 0, 255, 0]),
            components: 3,
            alpha: None,
        }
    }

    #[test]
    fn a_collector_starts_empty() {
        assert!(MediaCollector::new().is_empty());
    }

    #[test]
    fn the_same_image_twice_is_one_part() {
        let mut collector = MediaCollector::new();
        let first = collector.add(0, &jpeg(1)).expect("added");
        let second = collector.add(0, &jpeg(1)).expect("added");
        assert_eq!(first, second);
        assert_eq!(collector.len(), 1);
    }

    #[test]
    fn two_different_images_are_two_parts() {
        let mut collector = MediaCollector::new();
        let first = collector.add(0, &jpeg(1)).expect("added").clone();
        let second = collector.add(0, &jpeg(2)).expect("added").clone();
        assert_ne!(first, second);
        assert_eq!(collector.len(), 2);
    }

    #[test]
    fn raw_samples_become_a_png() {
        let mut collector = MediaCollector::new();
        let part = collector.add(0, &raw()).expect("added");
        assert!(part.as_str().ends_with(".png"), "{part}");
        let stored = collector.parts()[0].bytes.clone();
        assert_eq!(
            &stored[..8],
            b"\x89PNG\r\n\x1a\n",
            "the bytes must be a PNG"
        );
    }

    #[test]
    fn parts_are_numbered_in_the_order_they_appear() {
        let mut collector = MediaCollector::new();
        let first = collector.add(0, &jpeg(1)).expect("added");
        let second = collector.add(0, &raw()).expect("added");
        assert_eq!(first.as_str(), "/word/media/image1.jpeg");
        assert_eq!(second.as_str(), "/word/media/image2.png");
    }
}
