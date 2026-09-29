#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! CI gate: extended-scenario coverage for Stage 5 (`STAGE-5-TASK.md` §11.1,
//! S5.11) from `coverage/stage5-scenarios.toml`.
//!
//! This is an independent, human-authored matrix (not the parser's
//! problem-only `SupportModel`); the criterion is `>= 85%`.
//!
//! Coverage = `(supported + partial) / (supported + partial + unsupported)`;
//! `ignored` entries (out of the 5A scope) are excluded from the ratio.

use std::path::Path;

/// Minimum extended-scenario coverage, in percent (`STAGE-5-TASK.md` §11.1).
const MIN_COVERAGE: f64 = 85.0;

#[test]
fn stage5_scenario_coverage_gate() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../coverage/stage5-scenarios.toml");
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
    assert!(denominator > 0, "inventory has no counted scenarios");
    let coverage = f64::from(supported + partial) * 100.0 / f64::from(denominator);
    assert!(
        coverage + f64::EPSILON >= MIN_COVERAGE,
        "Stage-5 extended-scenario coverage {coverage:.1}% is below {MIN_COVERAGE:.1}% \
         (supported={supported}, partial={partial}, unsupported={unsupported})"
    );
}
