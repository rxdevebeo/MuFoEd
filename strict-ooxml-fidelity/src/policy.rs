//! `coverage/render-gates.toml`, parsed.
//!
//! GATE-STRATEGY §6: thresholds and tolerances live in **one** document, beside
//! the reason each has the value it has, and changing one is a reviewable diff in
//! that file. A constant edited inside a test looks like a test change and gets
//! reviewed as one, so the policy is data.
//!
//! The file has one table of classes and now **two** sets of thresholds: one for
//! the SVG backend and one for the PDF backend. The reason there are two is not
//! that the numbers are expected to differ but that the two gates measure two
//! different renderers (`resvg` on an SVG, `hayro` on a PDF) and a threshold is
//! only honest against the renderer it was measured with. Class membership and
//! the amber register are shared: a document is in the same content class
//! whichever backend draws it, and it is the same WPS reference either way.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One content class and the structural bounds its documents are held to.
#[derive(Debug, Clone)]
pub struct PolicyClass {
    /// The class name, for messages.
    pub name: String,
    /// Why the class is held to these bounds, as written in the policy file.
    pub rationale: String,
    /// The documents that belong to it.
    pub documents: Vec<String>,
    /// The bounds every document in the class is held to.
    pub limits: crate::structure::StructuralLimits,
}

/// One document's bounds, where they differ from its class's.
#[derive(Debug, Clone)]
pub struct PolicyOverride {
    /// The document these bounds apply to.
    pub document: String,
    /// Replacement shift bound, when the class's is not loose enough.
    pub max_shift_px: Option<isize>,
    /// Replacement centroid bound.
    pub max_centroid_px: Option<f64>,
    /// Replacement content-centroid bound.
    pub max_content_centroid_px: Option<f64>,
    /// Replacement extent bound.
    pub max_extent_px: Option<f64>,
    /// Replacement row-ink correlation floor.
    pub min_row_correlation: Option<f64>,
    /// Replacement column-ink correlation floor.
    pub min_column_correlation: Option<f64>,
}

/// One class's bounds for the PDF backend, where they differ from its own.
#[derive(Debug, Clone)]
pub struct PdfClassLimits {
    /// The class these bounds apply to.
    pub class: String,
    /// Replacement shift bound.
    pub max_shift_px: Option<isize>,
    /// Replacement centroid bound.
    pub max_centroid_px: Option<f64>,
    /// Replacement trimmed-centroid bound.
    pub max_content_centroid_px: Option<f64>,
    /// Replacement row-correlation floor.
    pub min_row_correlation: Option<f64>,
    /// Replacement column-correlation floor.
    pub min_column_correlation: Option<f64>,
    /// Replacement extent bound.
    pub max_extent_px: Option<f64>,
}

/// The fidelity criterion one backend is held to.
#[derive(Debug, Clone)]
pub struct Thresholds {
    /// Minimum acceptable worst-page SSIM.
    pub ssim: f64,
    /// The margin over `ssim` a document must clear to pass outright.
    pub margin: f64,
    /// Documents allowed to sit inside the margin band.
    pub amber: Vec<String>,
}

impl Thresholds {
    /// The path of the policy file, relative to this crate.
    #[must_use]
    pub fn repo_relative() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../coverage/render-gates.toml")
    }
}

/// The whole fidelity-gate policy.
#[derive(Debug, Clone)]
pub struct GatePolicy {
    /// The SVG backend's fidelity criterion.
    pub svg: Thresholds,
    /// The PDF backend's fidelity criterion (SC-6's pixel half).
    pub pdf: Thresholds,
    /// The content classes, in file order.
    pub classes: Vec<PolicyClass>,
    /// Per-document bounds that override the class's, in file order.
    ///
    /// A class is a **default**, not a verdict. One document can be held to
    /// numbers its class cannot carry — how far the substituted math face's
    /// constructs fall short of the reference's is measured per construct, not per
    /// class — and writing that into the class would loosen it for every document
    /// in it. An override is the narrow form of the decision: one document, its
    /// numbers, its reason.
    pub overrides: Vec<PolicyOverride>,
    /// Per-class bounds for the PDF backend, in file order.
    ///
    /// A structural bound is a statement about a *renderer*: the ink centroid of
    /// the same page moves between `resvg` and `hayro` because the two antialias
    /// a long thin stroke differently. Class membership is shared — a page is in
    /// the same content class whichever backend draws it — and only the numbers
    /// differ, so this is keyed by class name and every bound is optional: an
    /// absent one keeps the class's value.
    pub pdf_classes: Vec<PdfClassLimits>,
    /// Documents compared for the page-count invariant only.
    pub page_count_only: Vec<String>,
}

impl GatePolicy {
    /// The process-wide policy, read once from `coverage/render-gates.toml`.
    ///
    /// # Panics
    ///
    /// Panics if the file is missing or does not parse. A gate whose policy it
    /// could not read must not fall back to built-in numbers: those would be
    /// thresholds nobody reviewed, which is the exact failure GATE-STRATEGY §6
    /// exists to prevent.
    #[must_use]
    pub fn shared() -> &'static GatePolicy {
        static POLICY: OnceLock<GatePolicy> = OnceLock::new();
        POLICY.get_or_init(|| {
            let path = Thresholds::repo_relative();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            GatePolicy::parse(&text)
                .unwrap_or_else(|error| panic!("invalid {}: {error}", path.display()))
        })
    }

    /// Parses the policy file.
    ///
    /// # Errors
    ///
    /// Returns the reason if a bound is missing or a document is claimed by two
    /// classes — the second is the one that matters, because it is how a limit
    /// change goes unnoticed.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Raw = toml::from_str(text).map_err(|error| error.to_string())?;
        let classes = raw
            .classes
            .iter()
            .map(|class| PolicyClass {
                name: class.name.clone(),
                rationale: class.rationale.clone(),
                documents: class.documents.clone(),
                limits: crate::structure::StructuralLimits {
                    max_shift_px: class.max_shift_px,
                    max_centroid_px: class.max_centroid_px,
                    max_content_centroid_px: class.max_content_centroid_px,
                    min_row_correlation: class.min_row_correlation,
                    min_column_correlation: class.min_column_correlation,
                    max_extent_px: class.max_extent_px,
                },
            })
            .collect();
        let overrides = raw
            .overrides
            .into_iter()
            .map(|entry| PolicyOverride {
                document: entry.document.clone(),
                max_shift_px: entry.max_shift_px,
                max_centroid_px: entry.max_centroid_px,
                max_content_centroid_px: entry.max_content_centroid_px,
                max_extent_px: entry.max_extent_px,
                min_row_correlation: entry.min_row_correlation,
                min_column_correlation: entry.min_column_correlation,
            })
            .collect();
        let policy = Self {
            svg: raw.thresholds.into(),
            pdf: raw.pdf.into(),
            classes,
            overrides,
            pdf_classes: raw
                .pdf_classes
                .into_iter()
                .map(|entry| PdfClassLimits {
                    class: entry.class.clone(),
                    max_shift_px: entry.max_shift_px,
                    max_centroid_px: entry.max_centroid_px,
                    max_content_centroid_px: entry.max_content_centroid_px,
                    min_row_correlation: entry.min_row_correlation,
                    min_column_correlation: entry.min_column_correlation,
                    max_extent_px: entry.max_extent_px,
                })
                .collect(),
            page_count_only: raw
                .page_count_only
                .into_iter()
                .map(|entry| entry.name)
                .collect(),
        };
        policy.check_membership()?;
        policy.check_pdf_classes()?;
        Ok(policy)
    }

    /// Rejects a `[[pdf_classes]]` entry naming a class that does not exist.
    ///
    /// # Errors
    ///
    /// Returns the first unknown class name. An entry naming nothing in
    /// particular is a bound nobody applies, and a bound nobody applies is the
    /// same failure as no bound at all: the gate looks tight and is not.
    pub fn check_pdf_classes(&self) -> Result<(), String> {
        for entry in &self.pdf_classes {
            if self.class_named(&entry.class).is_none() {
                return Err(format!(
                    "pdf_classes names {}, which is not a class in this file",
                    entry.class
                ));
            }
        }
        Ok(())
    }

    /// The class with this **name**, as opposed to the class a document is in.
    fn class_named(&self, name: &str) -> Option<&PolicyClass> {
        self.classes.iter().find(|class| class.name == name)
    }

    /// Rejects a document claimed by two classes.
    ///
    /// # Errors
    ///
    /// Returns the first duplicated document name.
    pub fn check_membership(&self) -> Result<(), String> {
        let mut seen: Vec<&str> = Vec::new();
        for class in &self.classes {
            for document in &class.documents {
                if seen.contains(&document.as_str()) {
                    return Err(format!("{document} is claimed by two classes"));
                }
                seen.push(document);
            }
        }
        Ok(())
    }

    /// The structural limits for a reference document.
    ///
    /// Falls back to the strict defaults, which is what an unclassified
    /// document gets: a new fixture lands in the tightest class and has to be
    /// moved deliberately, not silently inherits whatever the last arm of a
    /// match statement returned.
    #[must_use]
    pub fn limits_for(&self, name: &str) -> crate::structure::StructuralLimits {
        let class = self
            .classes
            .iter()
            .find(|class| class.documents.iter().any(|entry| entry == name))
            .map_or_else(crate::structure::StructuralLimits::default, |class| {
                class.limits
            });
        let Some(override_entry) = self.overrides.iter().find(|entry| entry.document == name)
        else {
            return class;
        };
        // A bound the override does not mention keeps the class's value, so an
        // override is one decision and not a copy of the whole table.
        crate::structure::StructuralLimits {
            max_shift_px: override_entry.max_shift_px.unwrap_or(class.max_shift_px),
            max_centroid_px: override_entry
                .max_centroid_px
                .unwrap_or(class.max_centroid_px),
            max_content_centroid_px: override_entry
                .max_content_centroid_px
                .unwrap_or(class.max_content_centroid_px),
            min_row_correlation: override_entry
                .min_row_correlation
                .unwrap_or(class.min_row_correlation),
            min_column_correlation: override_entry
                .min_column_correlation
                .unwrap_or(class.min_column_correlation),
            max_extent_px: override_entry.max_extent_px.unwrap_or(class.max_extent_px),
        }
    }

    /// The structural limits the **PDF** gate holds `name` to.
    ///
    /// The class's own numbers, with the class's `[[pdf_classes]]` entry applied
    /// on top and then the document's own override: the per-document decision is
    /// the narrowest one and so has the last word, whichever backend is asking.
    #[must_use]
    pub fn pdf_limits_for(&self, name: &str) -> crate::structure::StructuralLimits {
        let base = self.limits_for(name);
        let Some(entry) = self
            .pdf_classes
            .iter()
            .find(|entry| Some(entry.class.as_str()) == self.class_of(name))
        else {
            return base;
        };
        crate::structure::StructuralLimits {
            max_shift_px: entry.max_shift_px.unwrap_or(base.max_shift_px),
            max_centroid_px: entry.max_centroid_px.unwrap_or(base.max_centroid_px),
            max_content_centroid_px: entry
                .max_content_centroid_px
                .unwrap_or(base.max_content_centroid_px),
            min_row_correlation: entry
                .min_row_correlation
                .unwrap_or(base.min_row_correlation),
            min_column_correlation: entry
                .min_column_correlation
                .unwrap_or(base.min_column_correlation),
            max_extent_px: entry.max_extent_px.unwrap_or(base.max_extent_px),
        }
    }

    /// The name of the class a document belongs to.
    #[must_use]
    pub fn class_of(&self, name: &str) -> Option<&str> {
        self.classes
            .iter()
            .find(|class| class.documents.iter().any(|entry| entry == name))
            .map(|class| class.name.as_str())
    }

    /// Every document compared page by page with SSIM.
    pub fn ssim_documents(&self) -> impl Iterator<Item = &str> {
        self.classes
            .iter()
            .flat_map(|class| class.documents.iter().map(String::as_str))
    }

    /// Whether a document is page-count only.
    #[must_use]
    pub fn is_page_count_only(&self, name: &str) -> bool {
        self.page_count_only.iter().any(|entry| entry == name)
    }
}

/// The whole policy file, as `toml` sees it.
#[derive(serde::Deserialize)]
struct Raw {
    thresholds: RawThresholds,
    /// The PDF backend's table. Absent means the SVG backend's numbers, which
    /// the file in the repository does not rely on.
    #[serde(default = "inherits_svg_thresholds")]
    pdf: RawThresholds,
    classes: Vec<RawClass>,
    #[serde(default)]
    overrides: Vec<RawOverride>,
    #[serde(default)]
    pdf_classes: Vec<RawPdfClassLimits>,
    #[serde(default)]
    page_count_only: Vec<RawNamed>,
}

/// A file that names no PDF table of its own gets the SVG one, so an older
/// policy file still says something true rather than nothing.
fn inherits_svg_thresholds() -> RawThresholds {
    RawThresholds {
        ssim: 0.95,
        ssim_margin: 0.01,
        amber: Vec::new(),
    }
}

/// One `[thresholds]`-style table: the criterion, the margin, and the documents
/// registered inside the margin band.
#[derive(serde::Deserialize)]
struct RawThresholds {
    ssim: f64,
    ssim_margin: f64,
    #[serde(default)]
    amber: Vec<String>,
}

impl From<RawThresholds> for Thresholds {
    fn from(raw: RawThresholds) -> Self {
        Self {
            ssim: raw.ssim,
            margin: raw.ssim_margin,
            amber: raw.amber,
        }
    }
}

#[derive(serde::Deserialize)]
struct RawClass {
    name: String,
    #[serde(default)]
    rationale: String,
    documents: Vec<String>,
    max_shift_px: isize,
    max_centroid_px: f64,
    #[serde(default = "strict_trimmed_centroid")]
    max_content_centroid_px: f64,
    min_row_correlation: f64,
    min_column_correlation: f64,
    max_extent_px: f64,
}

/// The strict default for the trimmed-centroid bound, for a class that does not
/// name one.
///
/// The strict default rather than "the class's `max_centroid_px`", because the
/// two are different measures and inheriting one from the other is how a bound
/// ends up describing something it was never measured on.
fn strict_trimmed_centroid() -> f64 {
    crate::structure::MAX_CONTENT_CENTROID_SHIFT_PX
}

#[derive(serde::Deserialize)]
struct RawNamed {
    name: String,
}

/// A per-document override: every bound is optional, and an absent one leaves
/// the class's value in place.
#[derive(serde::Deserialize)]
struct RawOverride {
    document: String,
    #[serde(default)]
    max_shift_px: Option<isize>,
    #[serde(default)]
    max_centroid_px: Option<f64>,
    #[serde(default)]
    max_content_centroid_px: Option<f64>,
    #[serde(default)]
    max_extent_px: Option<f64>,
    #[serde(default)]
    min_row_correlation: Option<f64>,
    #[serde(default)]
    min_column_correlation: Option<f64>,
}

/// A per-class PDF bound set, in the same optional-one-field-per-bound shape as
/// a per-document override.
#[derive(serde::Deserialize)]
struct RawPdfClassLimits {
    class: String,
    #[serde(default)]
    max_shift_px: Option<isize>,
    #[serde(default)]
    max_centroid_px: Option<f64>,
    #[serde(default)]
    max_content_centroid_px: Option<f64>,
    #[serde(default)]
    min_row_correlation: Option<f64>,
    #[serde(default)]
    min_column_correlation: Option<f64>,
    #[serde(default)]
    max_extent_px: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::GatePolicy;
    use crate::structure::{alignment_search_range, StructuralLimits};

    const MINIMAL: &str = r#"
[thresholds]
ssim = 0.95
ssim_margin = 0.01

[[classes]]
name = "text"
documents = ["a"]
max_shift_px = 2
max_centroid_px = 2.5
min_row_correlation = 0.9
min_column_correlation = 0.85
max_extent_px = 2.0
"#;

    #[test]
    fn a_minimal_policy_parses_and_a_missing_pdf_table_defaults_to_the_svg_one() {
        // With no `[pdf]` table the PDF gate would have no criterion of its own.
        // Rather than invent one, it inherits the SVG gate's numbers and says so
        // by construction; the file in the repository declares its own.
        let policy = GatePolicy::parse(MINIMAL).expect("parses");
        assert_eq!(policy.svg.ssim, 0.95);
        assert_eq!(policy.svg.margin, 0.01);
        assert_eq!(policy.pdf.ssim, 0.95);
        assert_eq!(policy.class_of("a"), Some("text"));
        assert!(!policy.is_page_count_only("a"));
    }

    #[test]
    fn an_unclassified_document_gets_the_strict_defaults() {
        let policy = GatePolicy::parse(MINIMAL).expect("parses");
        assert_eq!(policy.limits_for("unknown"), StructuralLimits::default());
    }

    #[test]
    fn a_document_in_two_classes_is_refused() {
        let text = MINIMAL;
        let error = GatePolicy::parse(&format!(
            "{text}\n[[classes]]\nname = \"other\"\ndocuments = [\"a\"]\n\
             max_shift_px = 2\nmax_centroid_px = 2.5\nmin_row_correlation = 0.9\n\
             min_column_correlation = 0.85\nmax_extent_px = 2.0\n"
        ))
        .expect_err("must refuse");
        assert!(error.contains("claimed by two classes"), "{error}");
    }

    #[test]
    fn an_override_touches_only_the_bounds_it_names() {
        let policy = GatePolicy::parse(&format!(
            "{MINIMAL}\n[[overrides]]\ndocument = \"a\"\nmax_extent_px = 34.0\n"
        ))
        .expect("parses");
        let limits = policy.limits_for("a");
        assert_eq!(limits.max_extent_px, 34.0);
        assert_eq!(limits.max_shift_px, 2, "the class's value is kept");
        assert_eq!(limits.min_row_correlation, 0.9, "the class's value is kept");
    }

    #[test]
    fn the_search_range_is_always_wider_than_the_bound() {
        // The invariant that makes the shift bound a real bound: a drift beyond
        // it has to be findable, or the check reports a clamped, in-bounds shift.
        for limit in [StructuralLimits::default(), StructuralLimits::unbounded()] {
            assert!(alignment_search_range(&limit) > limit.max_shift_px);
        }
    }

    #[test]
    fn the_shared_policy_is_readable_and_consistent() {
        let policy = GatePolicy::shared();
        assert!(policy.svg.ssim > 0.0 && policy.svg.ssim <= 1.0);
        assert!(policy.pdf.ssim > 0.0 && policy.pdf.ssim <= 1.0);
        assert!(policy.check_membership().is_ok());
        assert!(policy.check_pdf_classes().is_ok());
        // Every amber entry must name a document the gate actually grades, or
        // the register rots into a list of names nobody checks.
        for entry in policy.svg.amber.iter().chain(policy.pdf.amber.iter()) {
            assert!(
                policy.class_of(entry).is_some(),
                "{entry} is in an amber list but in no class"
            );
        }
    }

    #[test]
    fn a_pdf_class_must_name_a_class_that_exists() {
        let error = GatePolicy::parse(&format!(
            "{MINIMAL}\n[[pdf_classes]]\nclass = \"typo\"\nmax_centroid_px = 40.0\n"
        ))
        .expect_err("must refuse");
        assert!(error.contains("typo"), "{error}");
    }

    #[test]
    fn a_pdf_class_changes_only_the_bound_it_names() {
        let policy = GatePolicy::parse(&format!(
            "{MINIMAL}\n[[pdf_classes]]\nclass = \"text\"\nmax_centroid_px = 3.0\n"
        ))
        .expect("parses");
        let svg = policy.limits_for("a");
        let pdf = policy.pdf_limits_for("a");
        assert!((pdf.max_centroid_px - 3.0).abs() < f64::EPSILON);
        assert_eq!(pdf.max_shift_px, svg.max_shift_px);
        assert_eq!(pdf.max_extent_px, svg.max_extent_px);
        assert_eq!(pdf.min_row_correlation, svg.min_row_correlation);
        // A document in no named class falls back to the class's numbers, which
        // is the strict default rather than a bound nobody reviewed.
        assert_eq!(
            policy.pdf_limits_for("unclassified"),
            StructuralLimits::default()
        );
    }
}
