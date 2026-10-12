//! Render error model: every failure is mapped onto [`StrictError::Render`].

use strict_ooxml_core::error::StrictError;

/// A rendering failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// A media part could not be read (missing or corrupt).
    MediaUnavailable(String),
    /// A configured output limit was exceeded.
    LimitExceeded {
        /// Which limit (for diagnostics).
        what: &'static str,
        /// Configured bound.
        limit: u64,
        /// Observed value.
        actual: u64,
    },
    /// The document contains no usable section geometry.
    NoGeometry,
    /// An internal invariant failed.
    Internal(String),
    /// Page/region geometry did not stabilize within the bounded reflow limit.
    DidNotConverge {
        /// Number of layout passes that were attempted.
        passes: u64,
    },
}

impl RenderError {
    /// Converts into the crate-wide [`StrictError`].
    #[must_use]
    pub fn into_strict(self) -> StrictError {
        StrictError::Render(self.to_string())
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MediaUnavailable(detail) => write!(f, "media unavailable: {detail}"),
            Self::LimitExceeded {
                what,
                limit,
                actual,
            } => write!(
                f,
                "render limit exceeded ({what}): limit = {limit}, actual = {actual}"
            ),
            Self::NoGeometry => f.write_str("document has no page geometry"),
            Self::Internal(detail) => write!(f, "internal render error: {detail}"),
            Self::DidNotConverge { passes } => write!(
                f,
                "layout did not converge in {passes} passes (page count / header-footer region geometry still changing)"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests {
    use super::RenderError;

    #[test]
    fn errors_display_and_convert() {
        let error = RenderError::MediaUnavailable("/word/media/image1.png".to_owned());
        assert!(error.to_string().contains("media unavailable"));
        let strict = error.into_strict();
        assert!(matches!(
            strict,
            strict_ooxml_core::error::StrictError::Render(_)
        ));

        let limit = RenderError::LimitExceeded {
            what: "pages",
            limit: 10,
            actual: 11,
        };
        assert!(limit.to_string().contains("pages"));
        assert!(RenderError::NoGeometry.to_string().contains("geometry"));
        assert!(RenderError::Internal("x".to_owned())
            .to_string()
            .contains("internal"));
        assert!(RenderError::DidNotConverge { passes: 8 }
            .to_string()
            .contains("did not converge"));
        let _ = format!("{:?}", RenderError::NoGeometry);
    }
}
