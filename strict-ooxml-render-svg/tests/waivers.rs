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
    classes: Vec<PolicyClass>,
    #[serde(default)]
    page_count_only: Vec<Named>,
}

#[derive(Debug, serde::Deserialize)]
struct Thresholds {
    #[serde(default)]
    amber: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Named {
    name: String,
}

#[derive(Debug, serde::Deserialize)]
struct Waiver {
    id: String,
    binds: String,
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
            "amber" => {
                assert!(
                    amber.contains(target),
                    "waiver {} registers an amber document {target}, which the gate \
                     policy does not list as amber",
                    waiver.id
                );
                gated.contains(target)
            }
            "process" => !target.is_empty(),
            other => panic!(
                "waiver {} uses the unknown binds kind {other:?}; \
                 the vocabulary is class|page-count-only|amber|process",
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
