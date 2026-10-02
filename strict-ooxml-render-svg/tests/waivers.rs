//! CI gate: the waiver registry (`docs/waivers.toml`, GATE-STRATEGY §2) must
//! match what the gates actually do.
//!
//! A registry nobody checks is a document, and a document about waivers is
//! worth very little: it is written once, at acceptance, and read never. Every
//! entry here is a place where the work is accepted below what the criteria
//! ask for, so the interesting failure mode is not a missing entry — it is an
//! entry that has quietly stopped being true.
//!
//! Three things are checked, in both directions:
//!
//! 1. every loosened bound in `coverage/render-gates.toml` has a waiver;
//! 2. every waiver that claims to authorise a bound points at something that
//!    still exists, so a resolved problem cannot linger as an open one;
//! 3. the `binds` targets are spelled like a target, so a typo cannot turn a
//!    machine-checked waiver into an unchecked one.
//!
//! Since stage 8 there are **two** gates over the same references — the SVG one
//! and the PDF one — so there are two bound sets and two amber registers, and
//! both are checked here. See `CORE-QUEUE.md` §1.

#![allow(clippy::uninlined_format_args)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The strict defaults a class must not exceed to count as un-waived.
const STRICT_MAX_SHIFT_PX: i64 = 2;
const STRICT_MAX_CENTROID_PX: f64 = 2.5;
const STRICT_MIN_ROW_CORRELATION: f64 = 0.90;
const STRICT_MIN_COLUMN_CORRELATION: f64 = 0.85;
const STRICT_MAX_EXTENT_PX: f64 = 2.0;

/// The workspace root: this crate is `<root>/strict-ooxml-render-svg`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate directory has a parent")
        .to_path_buf()
}

/// One `[[classes]]` entry of the gate policy.
#[derive(Debug, serde::Deserialize)]
struct PolicyClass {
    name: String,
    documents: Vec<String>,
    max_shift_px: i64,
    max_centroid_px: f64,
    min_row_correlation: f64,
    min_column_correlation: f64,
    max_extent_px: f64,
}

impl PolicyClass {
    /// Whether any bound is looser than the strict default.
    fn is_loosened(&self) -> bool {
        self.max_shift_px > STRICT_MAX_SHIFT_PX
            || self.max_centroid_px > STRICT_MAX_CENTROID_PX
            || self.min_row_correlation < STRICT_MIN_ROW_CORRELATION
            || self.min_column_correlation < STRICT_MIN_COLUMN_CORRELATION
            || self.max_extent_px > STRICT_MAX_EXTENT_PX
    }

    /// The bounds that are looser than the strict default.
    fn loosened_bounds(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.max_shift_px > STRICT_MAX_SHIFT_PX {
            out.push(format!("max_shift_px={}", self.max_shift_px));
        }
        if self.max_centroid_px > STRICT_MAX_CENTROID_PX {
            out.push(format!("max_centroid_px={}", self.max_centroid_px));
        }
        if self.min_row_correlation < STRICT_MIN_ROW_CORRELATION {
            out.push(format!("min_row_correlation={}", self.min_row_correlation));
        }
        if self.min_column_correlation < STRICT_MIN_COLUMN_CORRELATION {
            out.push(format!(
                "min_column_correlation={}",
                self.min_column_correlation
            ));
        }
        if self.max_extent_px > STRICT_MAX_EXTENT_PX {
            out.push(format!("max_extent_px={}", self.max_extent_px));
        }
        out
    }
}

#[derive(Debug, serde::Deserialize)]
struct Policy {
    thresholds: Thresholds,
    /// The PDF backend's own fidelity criterion. A file without one inherits the
    /// SVG gate's, which is what `strict-ooxml-fidelity::policy` does too.
    #[serde(default = "same_as_svg")]
    pdf: Thresholds,
    classes: Vec<PolicyClass>,
    #[serde(default)]
    pdf_classes: Vec<PdfClassLimits>,
    #[serde(default)]
    page_count_only: Vec<Named>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct Thresholds {
    #[serde(default)]
    amber: Vec<String>,
}

/// A policy file with no `[pdf]` table is read as if it had the SVG gate's amber
/// list, so a file written before the PDF gate existed still says something true.
fn same_as_svg() -> Thresholds {
    Thresholds::default()
}

/// One `[[pdf_classes]]` entry: the PDF backend's bounds for a class, where they
/// differ from that class's own.
#[derive(Debug, serde::Deserialize)]
struct PdfClassLimits {
    class: String,
    #[serde(default)]
    max_shift_px: Option<i64>,
    #[serde(default)]
    max_centroid_px: Option<f64>,
    #[serde(default)]
    min_row_correlation: Option<f64>,
    #[serde(default)]
    min_column_correlation: Option<f64>,
    #[serde(default)]
    max_extent_px: Option<f64>,
}

impl PdfClassLimits {
    /// Whether any bound is looser than the strict default.
    fn is_loosened(&self) -> bool {
        self.max_shift_px
            .is_some_and(|value| value > STRICT_MAX_SHIFT_PX)
            || self
                .max_centroid_px
                .is_some_and(|value| value > STRICT_MAX_CENTROID_PX)
            || self
                .min_row_correlation
                .is_some_and(|value| value < STRICT_MIN_ROW_CORRELATION)
            || self
                .min_column_correlation
                .is_some_and(|value| value < STRICT_MIN_COLUMN_CORRELATION)
            || self
                .max_extent_px
                .is_some_and(|value| value > STRICT_MAX_EXTENT_PX)
    }

    /// The bounds that are looser than the strict default.
    fn loosened_bounds(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(value) = self.max_shift_px {
            out.push(format!("max_shift_px={value}"));
        }
        if let Some(value) = self.max_centroid_px {
            out.push(format!("max_centroid_px={value}"));
        }
        if let Some(value) = self.min_row_correlation {
            out.push(format!("min_row_correlation={value}"));
        }
        if let Some(value) = self.min_column_correlation {
            out.push(format!("min_column_correlation={value}"));
        }
        if let Some(value) = self.max_extent_px {
            out.push(format!("max_extent_px={value}"));
        }
        out
    }
}

#[derive(Debug, serde::Deserialize)]
struct Named {
    name: String,
}

#[derive(Debug, serde::Deserialize)]
struct Waiver {
    id: String,
    binds: String,
    /// When the waiver was closed, if it was. A closed waiver is a record of what
    /// it cost, so it must **not** still be listed as outstanding — the same
    /// bidirectional rule the gate applies to its own amber list.
    #[serde(default)]
    closed: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Registry {
    waivers: Vec<Waiver>,
}

fn policy() -> Policy {
    let path = repo_root().join("coverage/render-gates.toml");
    toml::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
    .expect("gate policy parses")
}

fn registry() -> Registry {
    let path = repo_root().join("docs/waivers.toml");
    toml::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
    .expect("waiver registry parses")
}

/// Every class whose bounds are looser than the strict defaults has a waiver.
#[test]
fn every_loosened_class_has_a_waiver() {
    let registry = registry();
    let waived: BTreeSet<&str> = registry
        .waivers
        .iter()
        .filter_map(|waiver| waiver.binds.strip_prefix("class:"))
        .collect();
    let policy = policy();
    for class in &policy.classes {
        if !class.is_loosened() {
            continue;
        }
        assert!(
            waived.contains(class.name.as_str()),
            "class {} loosens {} but has no waiver in docs/waivers.toml \
             (binds = \"class:{}\")",
            class.name,
            class.loosened_bounds().join(", "),
            class.name
        );
    }
}

/// Every document compared by page count only has a waiver: for those the gate
/// says nothing about appearance, which is worth stating out loud rather than
/// leaving implied by an absence.
#[test]
fn every_page_count_only_document_has_a_waiver() {
    let registry = registry();
    let waived: BTreeSet<&str> = registry
        .waivers
        .iter()
        .filter_map(|waiver| waiver.binds.strip_prefix("page-count-only:"))
        .collect();
    for entry in policy().page_count_only {
        assert!(
            waived.contains(entry.name.as_str()),
            "{} is compared by page count only but has no waiver \
             (binds = \"page-count-only:{}\")",
            entry.name,
            entry.name
        );
    }
}

/// Every waiver points at something that still exists.
///
/// This is the direction that catches rot. A waiver whose target is gone is a
/// resolved problem still described as open, and the registry is the only
/// place a reader would look to find out what is still outstanding.
#[test]
fn every_waiver_binds_to_something_that_still_exists() {
    let policy = policy();
    let class_names: BTreeSet<&str> = policy
        .classes
        .iter()
        .map(|class| class.name.as_str())
        .collect();
    let page_count: BTreeSet<&str> = policy
        .page_count_only
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    let gated: BTreeSet<&str> = policy
        .classes
        .iter()
        .flat_map(|class| class.documents.iter().map(String::as_str))
        .collect();
    let amber: BTreeSet<&str> = policy.thresholds.amber.iter().map(String::as_str).collect();
    let pdf_amber: BTreeSet<&str> = policy.pdf.amber.iter().map(String::as_str).collect();
    let pdf_classes: BTreeSet<&str> = policy
        .pdf_classes
        .iter()
        .map(|entry| entry.class.as_str())
        .collect();

    for waiver in registry().waivers {
        let Some((kind, target)) = waiver.binds.split_once(':') else {
            panic!(
                "waiver {} has binds = {:?}, which is not \"<kind>:<target>\"",
                waiver.id, waiver.binds
            );
        };
        let exists = match kind {
            "class" => class_names.contains(target),
            "page-count-only" => page_count.contains(target),
            "amber" | "pdf-amber" => {
                let listed = if kind == "amber" { &amber } else { &pdf_amber };
                // A **closed** waiver is the record of a gap that no longer exists,
                // so it must not still be registered as amber; an open one must be,
                // because otherwise the gap is being carried without a name.
                if waiver.closed.is_some() {
                    assert!(
                        !listed.contains(target),
                        "waiver {} was closed ({}) but {} is still in the {kind} list — a \
                         closed waiver must not keep looking like outstanding debt",
                        waiver.id,
                        waiver.closed.as_deref().unwrap_or(""),
                        target
                    );
                } else {
                    assert!(
                        listed.contains(target),
                        "waiver {} registers an amber document {target}, which the gate \
                         policy does not list in {kind}",
                        waiver.id
                    );
                }
                gated.contains(target)
            }
            // A PDF class's bounds differ from its own only where the policy file
            // says so, so the target has to be a class the policy has an entry
            // for — the same reason `class:` does.
            "pdf-class" => pdf_classes.contains(target),
            "process" => !target.is_empty(),
            other => panic!(
                "waiver {} uses the unknown binds kind {other:?}; \
                 the vocabulary is class|page-count-only|amber|pdf-amber|pdf-class|process",
                waiver.id
            ),
        };
        assert!(
            exists,
            "waiver {} binds to {:?}, which no longer exists — \
             either the gap was closed (delete the waiver) or the name changed",
            waiver.id, waiver.binds
        );
    }
}

/// Every document in the gate's amber list has a waiver.
///
/// Without this, a document could be added to the margin band and the registry
/// would not mention it — which is the whole failure the amber register exists
/// to prevent.
#[test]
fn every_amber_document_has_a_waiver() {
    let registry = registry();
    let waived: BTreeSet<&str> = registry
        .waivers
        .iter()
        .filter_map(|waiver| waiver.binds.strip_prefix("amber:"))
        .collect();
    for document in &policy().thresholds.amber {
        assert!(
            waived.contains(document.as_str()),
            "{document} is in the gate's amber list but has no waiver \
             (binds = \"amber:{document}\")"
        );
    }
}

/// The PDF backend's own amber list is held to the same rule.
///
/// It is a separate list because it is a separate renderer with a separate
/// threshold, and a document can be amber in one and not the other. What it must
/// not do is be amber for a reason nobody wrote down.
#[test]
fn every_pdf_amber_document_has_a_waiver() {
    let registry = registry();
    let waived: BTreeSet<&str> = registry
        .waivers
        .iter()
        .filter_map(|waiver| waiver.binds.strip_prefix("pdf-amber:"))
        .collect();
    for document in &policy().pdf.amber {
        assert!(
            waived.contains(document.as_str()),
            "{document} is in the PDF gate's amber list but has no waiver \
             (binds = \"pdf-amber:{document}\")"
        );
    }
}

/// Every class the PDF gate holds to a looser bound than its own has a waiver.
///
/// A structural bound is a statement about a renderer, and a second renderer
/// measuring the same page differently is expected — but each loosening is still
/// a place where the work is accepted below what the criteria ask for, so each one
/// is named.
#[test]
fn every_loosened_pdf_class_has_a_waiver() {
    let registry = registry();
    let waived: BTreeSet<&str> = registry
        .waivers
        .iter()
        .filter_map(|waiver| waiver.binds.strip_prefix("pdf-class:"))
        .collect();
    for entry in &policy().pdf_classes {
        if !entry.is_loosened() {
            continue;
        }
        assert!(
            waived.contains(entry.class.as_str()),
            "the PDF gate loosens {} for class {} but there is no waiver in \
             docs/waivers.toml (binds = \"pdf-class:{}\")",
            entry.loosened_bounds().join(", "),
            entry.class,
            entry.class
        );
    }
}

/// The PDF gate's structural bounds must actually differ from the SVG gate's
/// somewhere, or the second table is decoration.
///
/// This is the direction that would otherwise go unchecked: adding a
/// `[[pdf_classes]]` entry whose numbers equal the class's own buys nothing and
/// costs a reader a paragraph of explanation.
#[test]
fn the_pdf_classes_table_is_not_a_copy_of_the_classes_table() {
    let policy = policy();
    let redundant: Vec<&str> = policy
        .pdf_classes
        .iter()
        .filter(|entry| entry.loosened_bounds().is_empty())
        .map(|entry| entry.class.as_str())
        .collect();
    assert!(
        redundant.is_empty(),
        "these [[pdf_classes]] entries name bounds identical to their class's, so they \
         loosen nothing and only add a reader's work: {redundant:?}"
    );
}

/// Waiver ids are unique and non-empty; a duplicate id makes "the waiver for
/// X" ambiguous.
#[test]
fn waiver_ids_are_unique() {
    let mut seen = BTreeSet::new();
    for waiver in registry().waivers {
        assert!(!waiver.id.is_empty(), "a waiver has an empty id");
        assert!(
            seen.insert(waiver.id.clone()),
            "duplicate waiver id {}",
            waiver.id
        );
    }
}
