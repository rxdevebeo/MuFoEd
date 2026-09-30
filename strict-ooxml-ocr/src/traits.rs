//! The two capabilities a converter can ask a model for.

use crate::Image;

/// Why a recognition attempt did not produce an answer.
///
/// A refusal is a value, not an absence: the converter has to be able to say
/// "this page has no text and nothing could be done about it", which is a
/// different fact from "this page has no text".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisionError {
    /// No model is configured, or the feature that talks to one is off.
    NotConfigured(String),
    /// The daemon could not be reached.
    Unreachable(String),
    /// The daemon answered with an error.
    Rejected(String),
    /// The answer arrived but could not be used.
    Unusable(String),
    /// The attempt took longer than the caller's budget.
    TimedOut,
}

impl std::fmt::Display for VisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(detail) => write!(f, "no recogniser is configured: {detail}"),
            Self::Unreachable(detail) => write!(f, "the model server is unreachable: {detail}"),
            Self::Rejected(detail) => write!(f, "the model server rejected the request: {detail}"),
            Self::Unusable(detail) => write!(f, "the model's answer could not be used: {detail}"),
            Self::TimedOut => f.write_str("the model did not answer within the budget"),
        }
    }
}

impl std::error::Error for VisionError {}

/// An answer that came from a model rather than from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Recovered {
    /// The text.
    pub text: String,
    /// Which model produced it, for the report.
    pub model: String,
    /// The model's version or digest, so an answer can be reproduced.
    pub version: String,
    /// How sure the model was, when it says.
    pub confidence: Option<f64>,
}

/// Recovers the text of a page that carries no text layer.
///
/// This is the capability a scanned document needs, and it is a trait rather
/// than a function because the transport is a feature: with `ocr-ollama` off,
/// nothing in the crate can reach a model, and a caller who wants text
/// recognition brings their own implementation.
///
/// No producer exists in this crate yet - turning a page into an image needs a
/// rasteriser, which is `O-3` in the register. The trait is here so the
/// converter can ask, record the answer as missing, and be ready when there is
/// something to ask with.
pub trait TextRecovery: Send + Sync {
    /// The model this implementation would use, for the report.
    fn model_name(&self) -> &str;

    /// Recovers a page's text.
    ///
    /// # Errors
    ///
    /// Returns a [`VisionError`] when no answer could be obtained; the caller
    /// records it and continues.
    fn recover_page(&self, page: &Image) -> Result<Option<Recovered>, VisionError>;
}

/// Says what a graphic region is, and describes it in words.
///
/// This is the capability that pays for itself: a figure with a
/// `wp:docPr/@descr` is reachable by a screen reader, and a diagram described as
/// "a bar chart of revenue by quarter" is a caption a human can search for. Both
/// come out of the same call.
pub trait FigureClassifier: Send + Sync {
    /// The model this implementation would use, for the report.
    fn model_name(&self) -> &str;

    /// Classifies and describes one graphic region.
    ///
    /// # Errors
    ///
    /// Returns a [`VisionError`] when no answer could be obtained.
    fn describe(&self, region: &Image, context: &str) -> Result<Recovered, VisionError>;
}

/// What a region turned out to be.
///
/// A closed vocabulary: the point is to decide *how* to treat the region, and an
/// open one would make that a judgement call on every conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FigureKind {
    /// A table of numbers.
    Table,
    /// A diagram, a chart, a schematic.
    Diagram,
    /// A formula.
    Formula,
    /// A logo, a badge, a mark.
    Logo,
    /// A photograph or an illustration with no other structure.
    Illustration,
    /// A scanned region with no text layer of its own.
    Scan,
    /// Something this vocabulary does not name.
    Unknown,
}

impl FigureKind {
    /// Parses a model's answer, case-insensitively.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        // Whole words, not substrings: "photograph" and "graphic" both contain
        // "graph", and a classifier that fires on either calls a photograph a
        // diagram. Model prose is full of such words.
        let words: Vec<String> = text
            .split(|ch: char| !ch.is_ascii_alphabetic())
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        let has = |needle: &str| words.iter().any(|word| word == needle);
        // The order answers a real ambiguity: a model describing a chart *of a
        // table* mentions both, and the shape of the thing decides how it is
        // treated, not the last noun in the sentence.
        if has("chart") || has("diagram") || has("graph") || has("schematic") {
            Self::Diagram
        } else if has("table") {
            Self::Table
        } else if has("formula") || has("equation") {
            Self::Formula
        } else if has("logo") || has("badge") {
            Self::Logo
        } else if has("photo") || has("photograph") || has("illustration") {
            Self::Illustration
        } else if has("scan") {
            Self::Scan
        } else {
            Self::Unknown
        }
    }

    /// The kind's name, for a report line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Diagram => "diagram",
            Self::Formula => "formula",
            Self::Logo => "logo",
            Self::Illustration => "illustration",
            Self::Scan => "scan",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for FigureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{FigureKind, VisionError};

    #[test]
    fn the_vocabulary_covers_what_a_model_answers() {
        assert_eq!(
            FigureKind::parse("A bar chart of revenue"),
            FigureKind::Diagram
        );
        assert_eq!(FigureKind::parse("TABLE of values"), FigureKind::Table);
        // A chart of a table is a chart: the shape decides, not the last noun.
        assert_eq!(
            FigureKind::parse("a chart showing a table of values"),
            FigureKind::Diagram
        );
        assert_eq!(
            FigureKind::parse("an equation, E=mc^2"),
            FigureKind::Formula
        );
        assert_eq!(FigureKind::parse("company logo"), FigureKind::Logo);
        assert_eq!(FigureKind::parse("a photograph"), FigureKind::Illustration);
        // "photograph" contains "graph": a substring match calls a photograph a
        // diagram, and every model describes photographs.
        assert_eq!(
            FigureKind::parse("a photograph of a site"),
            FigureKind::Illustration,
            "a word inside another word must not decide the kind"
        );
        // The vocabulary is closed on purpose. A word nobody listed stays
        // `Unknown`, which is recorded, rather than being forced into the nearest
        // category - and the caller decides what `Unknown` means.
        assert_eq!(
            FigureKind::parse("a graphical abstract"),
            FigureKind::Unknown
        );
        assert_eq!(FigureKind::parse("a scan of a page"), FigureKind::Scan);
        assert_eq!(FigureKind::parse("something else"), FigureKind::Unknown);
        assert_eq!(FigureKind::parse("").as_str(), "unknown");
    }

    #[test]
    fn a_refusal_is_a_value_with_a_reason() {
        let error = VisionError::NotConfigured("the ollama feature is off".to_owned());
        assert!(error.to_string().contains("no recogniser"), "{error}");
        assert!(VisionError::TimedOut.to_string().contains("budget"));
    }
}
