#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! CI gate: optional-element coverage from `coverage/wml-elements.toml` ≥ 90%
//! (STAGE-2 §13.1, S2.20).
//!
//! Coverage = `(supported + partial) / (supported + partial + unsupported)`;
//! `mandatory` and `ignored` entries are excluded from the denominator.

use std::path::Path;

/// Minimum optional-element coverage, in percent.
const MIN_COVERAGE: f64 = 90.0;

#[test]
fn optional_element_coverage_gate() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../coverage/wml-elements.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let value: toml::Value = toml::from_str(&text).expect("valid TOML inventory");
    let elements = value
        .get("elements")
        .and_then(toml::Value::as_array)
        .expect("inventory has [[elements]]");

    let (mut supported, mut partial, mut unsupported) = (0u32, 0u32, 0u32);
    for element in elements {
        match element.get("status").and_then(toml::Value::as_str) {
            Some("supported") => supported += 1,
            Some("partial") => partial += 1,
            Some("unsupported") => unsupported += 1,
            _ => {}
        }
    }
    let denominator = supported + partial + unsupported;
    assert!(denominator > 0, "inventory has no optional elements");
    let coverage = f64::from(supported + partial) * 100.0 / f64::from(denominator);
    assert!(
        coverage + f64::EPSILON >= MIN_COVERAGE,
        "optional-element coverage {coverage:.1}% is below {MIN_COVERAGE:.1}%"
    );
}
