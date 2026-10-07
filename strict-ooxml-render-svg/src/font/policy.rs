//! Bounded font diagnostics and explicit rendering failure policy.
use super::{
    resolve_bundled_handle, shape_resource, FaceInstanceKey, FaceMapping, ResolvedFaceHandle,
};
use crate::layout::{Item, PlacedPage, TextAdvanceKind};
use crate::style::{registered_face, requested_family};

/// How a rendering font failure is handled. Admission errors always remain errors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontFailurePolicy {
    /// Continue with a bounded explicit report of fallback, degraded or omitted text.
    #[default]
    ReportAndContinue,
    /// Return an error before returning placed pages or backend output.
    Reject,
}

/// Stable font resolution/failure classifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontDiagnosticKind {
    /// The declared family matches the request; not proof of internal naming or coverage.
    Exact,
    /// A known alias or explicit manifest selects a declared substitute.
    Substitute,
    /// Unknown request uses the deterministic fallback; strict policy refuses it.
    Unresolved,
    /// The selected program has a missing scalar or shaped glyph.
    MissingGlyph,
    /// The shaper could not produce positions for the selected resource.
    ShapingFailed,
    /// Complex-script acceptance is explicitly degraded by the shared shaper.
    DegradedScript,
    /// The backend cannot deliver the selected resource.
    UnsupportedDelivery,
    /// PDF subsetting/embedding failed for the selected resource.
    EmbeddingFailed,
}
impl FontDiagnosticKind {
    /// Stable machine-readable diagnostic identifier.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Exact => "font.resolved.exact",
            Self::Substitute => "font.resolved.substitute",
            Self::Unresolved => "font.unresolved",
            Self::MissingGlyph => "font.glyph.missing",
            Self::ShapingFailed => "font.shaping.failed",
            Self::DegradedScript => "font.script.degraded",
            Self::UnsupportedDelivery => "font.delivery.unsupported",
            Self::EmbeddingFailed => "font.embed.failed",
        }
    }
    /// Whether this is a failure rather than resolution information.
    #[must_use]
    pub fn is_failure(self) -> bool {
        !matches!(self, Self::Exact | Self::Substitute)
    }
}

/// A bounded diagnostic preserving the original script-font request and program identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontDiagnostic {
    /// Classification and stable id.
    pub kind: FontDiagnosticKind,
    /// Original font-slot request, before substitution (bounded excerpt if oversized).
    pub requested_family: String,
    /// Selected declared family, including the deterministic fallback if used.
    pub resolved_family: Option<String>,
    /// Requested bold style.
    pub bold: bool,
    /// Requested italic style.
    pub italic: bool,
    /// Exact program/index/coordinate identity when a resource was selected.
    pub resource: Option<FaceInstanceKey>,
    /// Missing Unicode scalar, when identifiable; not an entire potentially huge cluster.
    pub codepoint: Option<u32>,
    /// Bounded explanation.
    pub detail: String,
    /// An original metadata/explanation string exceeded the per-field byte bound.
    pub details_truncated: bool,
}
impl FontDiagnostic {
    /// Constructs a bounded diagnostic against the actual selected resource.
    #[must_use]
    pub fn for_face(
        kind: FontDiagnosticKind,
        face: &ResolvedFaceHandle,
        codepoint: Option<char>,
        detail: &str,
    ) -> Self {
        let (requested_family, a) = excerpt(&face.requested_family);
        let (resolved_family, b) = excerpt(&face.family);
        let (detail, c) = excerpt(detail);
        Self {
            kind,
            requested_family,
            resolved_family: Some(resolved_family),
            bold: face.bold,
            italic: face.italic,
            resource: Some(face.resource.key().clone()),
            codepoint: codepoint.map(u32::from),
            detail,
            details_truncated: a || b || c,
        }
    }
    /// Stable human-readable representation used by strict errors and SVG warnings.
    #[must_use]
    pub fn message(&self) -> String {
        let scalar = self
            .codepoint
            .map_or_else(String::new, |value| format!(" U+{value:04X}"));
        format!(
            "{}: {}{scalar}: {}",
            self.kind.id(),
            self.requested_family,
            self.detail
        )
    }
}

fn excerpt(value: &str) -> (String, bool) {
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), end < value.len())
}

/// Bounded deterministic details plus an occurrence count that survives truncation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontReport {
    diagnostics: Vec<FontDiagnostic>,
    limit: usize,
    /// Failure occurrences, including duplicate occurrences and omitted details.
    pub failure_count: u64,
    /// Some distinct details could not be retained within the configured bound.
    pub details_truncated: bool,
}
impl Default for FontReport {
    fn default() -> Self {
        Self::new(256)
    }
}
impl FontReport {
    /// Sets a detail bound, clamped to the hard ceiling of 4096. Zero retains counts only.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            diagnostics: Vec::new(),
            limit: limit.min(4096),
            failure_count: 0,
            details_truncated: false,
        }
    }
    /// Retained distinct diagnostics in deterministic discovery order.
    #[must_use]
    pub fn diagnostics(&self) -> &[FontDiagnostic] {
        &self.diagnostics
    }
    /// Whether there were no failures, independent of retained detail capacity.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.failure_count == 0
    }
    /// Checks strict policy before deduplication or detail truncation. Returns whether retained.
    ///
    /// # Errors
    /// Returns a render error for a failure under Reject, even when the detail bound is zero.
    pub fn record(
        &mut self,
        diagnostic: FontDiagnostic,
        policy: FontFailurePolicy,
    ) -> strict_ooxml_core::error::Result<bool> {
        if diagnostic.kind.is_failure() {
            if policy == FontFailurePolicy::Reject {
                return Err(strict_ooxml_core::error::StrictError::Render(format!(
                    "font policy rejected {}",
                    diagnostic.message()
                )));
            }
            self.failure_count = self.failure_count.saturating_add(1);
        }
        Ok(self.retain(diagnostic))
    }
    fn retain(&mut self, diagnostic: FontDiagnostic) -> bool {
        if self.diagnostics.contains(&diagnostic) {
            return false;
        }
        if self.diagnostics.len() >= self.limit {
            self.details_truncated = true;
            return false;
        }
        self.details_truncated |= diagnostic.details_truncated;
        self.diagnostics.push(diagnostic);
        true
    }
    /// Merges bounded details and all occurrence counts, returning newly retained entries.
    #[must_use]
    pub fn append(&mut self, other: Self) -> Vec<FontDiagnostic> {
        self.failure_count = self.failure_count.saturating_add(other.failure_count);
        self.details_truncated |= other.details_truncated;
        let mut retained = Vec::new();
        for diagnostic in other.diagnostics {
            if self.retain(diagnostic.clone()) {
                retained.push(diagnostic);
            }
        }
        retained
    }
    /// Failure warnings plus a truncation summary; resolution info remains available as typed data.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<_> = self
            .diagnostics
            .iter()
            .filter(|entry| entry.kind.is_failure())
            .map(FontDiagnostic::message)
            .collect();
        if self.details_truncated {
            out.push(format!(
                "font.report.truncated: {} failure occurrence(s); detail capacity {}",
                self.failure_count, self.limit
            ));
        }
        out
    }
}

/// Backend capabilities checked before delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontBackend {
    /// Shared geometry; no backend font-program delivery requirement.
    Placement,
    /// Standalone SVG currently accepts single-face programs only.
    Svg,
    /// PDF accepts collection face indices; subsetting failures are recorded by its collector.
    Pdf,
}

/// Whether the SVG writer can deliver the explicit face without selecting another collection face.
#[must_use]
pub fn svg_resource_supported(face: &ResolvedFaceHandle) -> bool {
    face.resource.key().face_index == 0 && !face.resource.bytes().starts_with(b"ttcf")
}

/// Resolves the same original font slot that measurement and delivery use.
#[must_use]
pub fn selected_face(text: &crate::layout::TextItem) -> Option<ResolvedFaceHandle> {
    registered_face(&text.run, &text.text).or_else(|| {
        resolve_bundled_handle(
            requested_family(&text.run, &text.text),
            text.run.bold,
            text.run.italic,
        )
    })
}

/// Inspects actual placed text. Default bundled-only continuation preserves legacy diagnostics.
///
/// # Errors
/// Strict policy returns the first detected failure before any backend output is returned.
pub fn inspect_page_fonts(
    page: &PlacedPage,
    policy: FontFailurePolicy,
    backend: FontBackend,
    limit: usize,
    force_reports: bool,
) -> strict_ooxml_core::error::Result<FontReport> {
    let mut report = FontReport::new(limit);
    for item in &page.items {
        let Item::Text(text) = item else { continue };
        if text.text.is_empty()
            || policy == FontFailurePolicy::ReportAndContinue
                && text.run.font_assets.is_none()
                && !force_reports
        {
            continue;
        }
        let Some(face) = selected_face(text) else {
            let (family, truncated) = excerpt(requested_family(&text.run, &text.text));
            report.record(
                FontDiagnostic {
                    kind: FontDiagnosticKind::Unresolved,
                    requested_family: family,
                    resolved_family: None,
                    bold: text.run.bold,
                    italic: text.run.italic,
                    resource: None,
                    codepoint: None,
                    detail: "no selected font resource".into(),
                    details_truncated: truncated,
                },
                policy,
            )?;
            continue;
        };
        let kind = match face.mapping {
            FaceMapping::ExactFamily => FontDiagnosticKind::Exact,
            FaceMapping::Substitute => FontDiagnosticKind::Substitute,
            FaceMapping::DefaultFallback => FontDiagnosticKind::Unresolved,
        };
        report.record(
            FontDiagnostic::for_face(
                kind,
                &face,
                None,
                if kind == FontDiagnosticKind::Unresolved {
                    "request unresolved; deterministic bundled fallback selected"
                } else {
                    "selected declared family and program"
                },
            ),
            policy,
        )?;
        if backend == FontBackend::Svg && !svg_resource_supported(&face) {
            report.record(
                FontDiagnostic::for_face(
                    FontDiagnosticKind::UnsupportedDelivery,
                    &face,
                    None,
                    "SVG requires a single-face program; text omitted",
                ),
                policy,
            )?;
            continue;
        }
        inspect_text_glyphs(&mut report, &face, text, policy)?;
    }
    Ok(report)
}

fn inspect_text_glyphs(
    report: &mut FontReport,
    face: &ResolvedFaceHandle,
    text: &crate::layout::TextItem,
    policy: FontFailurePolicy,
) -> strict_ooxml_core::error::Result<()> {
    let shown = super::present_text(&text.run.family, &text.text);
    if text.advance == TextAdvanceKind::Metric {
        for ch in shown.chars() {
            if face.resource.glyph_id(ch).is_none() {
                report.record(
                    FontDiagnostic::for_face(
                        FontDiagnosticKind::MissingGlyph,
                        face,
                        Some(ch),
                        "selected resource has no scalar glyph",
                    ),
                    policy,
                )?;
            }
        }
    } else if let Some(shaped) = shape_resource(&shown, face) {
        for cluster in &shaped.clusters {
            if shaped.glyphs[cluster.glyph_start..cluster.glyph_start + cluster.glyph_count]
                .iter()
                .any(|glyph| glyph.glyph_id == 0)
            {
                let label = shown
                    .get(cluster.byte_start..cluster.byte_end)
                    .unwrap_or("");
                let scalar = label
                    .chars()
                    .find(|ch| face.resource.glyph_id(*ch).is_none());
                report.record(
                    FontDiagnostic::for_face(
                        FontDiagnosticKind::MissingGlyph,
                        face,
                        scalar,
                        "selected resource produced a missing shaped glyph",
                    ),
                    policy,
                )?;
            }
        }
    } else {
        report.record(
            FontDiagnostic::for_face(
                FontDiagnosticKind::ShapingFailed,
                face,
                None,
                "selected resource could not be shaped",
            ),
            policy,
        )?;
    }
    if super::needs_complex_script(&shown) {
        report.record(
            FontDiagnostic::for_face(
                FontDiagnosticKind::DegradedScript,
                face,
                None,
                "complex-script acceptance remains degraded",
            ),
            policy,
        )?;
    }
    Ok(())
}
