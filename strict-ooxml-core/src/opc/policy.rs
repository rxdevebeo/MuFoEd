//! The single conformance-policy decision matrix (AUD-23 / ADR-0016).
//!
//! Before this module existed, the same decision was taken three times —
//! `opc::mod::enforce_policy`, `wml::parse::parse_document`'s own gate and the
//! CLI's `check` branching — and the three disagreed: `Permissive` with no
//! normalizer let a Transitional package through unexamined, `Mixed` was
//! accepted under any policy as long as a normalizer was installed (even
//! `StrictOnly`, which [`decide`] now refuses), and `Unknown` was accepted
//! under `StrictOnly` (silently treating "no signal" as "Strict").
//!
//! [`decide`] is the only place a [`ConformancePolicy`] is weighed against a
//! detected [`Conformance`] and whether a `RawNormalizer` is configured.
//! [`Package::open_archive`](super::Package) calls it once, after T0
//! detection; nothing else re-implements any cell of this table.

use crate::error::{Result, SourceLocation, StrictError};
use crate::ns::Conformance;
use crate::opc::ConformancePolicy;

/// Decides whether a package may be opened under `policy`, given its T0
/// (pre-normalization) [`Conformance`] and whether a `RawNormalizer` was
/// configured.
///
/// | policy \ detected | Strict | Transitional | Mixed | Unknown |
/// |---|---|---|---|---|
/// | `StrictOnly` | ok | `TransitionalNotSupported` | `MixedConformance` | `UndeterminedConformance` |
/// | `Normalize`, has normalizer | ok | ok | ok | ok |
/// | `Normalize`, no normalizer | ok | `Unsupported` | `Unsupported` | `Unsupported` |
/// | `Permissive`, has normalizer | ok | ok | ok | ok |
/// | `Permissive`, no normalizer | ok | `Unsupported` | `Unsupported` | ok |
///
/// `Permissive` differs from `Normalize` in exactly one cell: without a
/// normalizer it still accepts `Unknown` (so `inspect`-style callers can look
/// at a package with no determinable conformance), where `Normalize` without a
/// normalizer refuses everything but `Strict`.
///
/// # Errors
///
/// Returns the error named by the matrix cell above; `Ok(())` for every "ok"
/// cell.
pub fn decide(
    policy: ConformancePolicy,
    detected: Conformance,
    has_normalizer: bool,
) -> Result<()> {
    match policy {
        ConformancePolicy::StrictOnly => match detected {
            Conformance::Strict => Ok(()),
            Conformance::Transitional => Err(StrictError::TransitionalNotSupported {
                location: SourceLocation::unknown(),
            }),
            Conformance::Mixed => Err(StrictError::MixedConformance {
                detail: "both Strict and Transitional signals were detected".to_owned(),
            }),
            Conformance::Unknown => Err(StrictError::UndeterminedConformance {
                detail: "no Strict or Transitional signal was detected".to_owned(),
            }),
        },
        ConformancePolicy::Normalize | ConformancePolicy::Permissive if has_normalizer => Ok(()),
        ConformancePolicy::Normalize => match detected {
            Conformance::Strict => Ok(()),
            Conformance::Transitional | Conformance::Mixed | Conformance::Unknown => {
                Err(unsupported_without_normalizer(policy))
            }
        },
        ConformancePolicy::Permissive => match detected {
            Conformance::Strict | Conformance::Unknown => Ok(()),
            Conformance::Transitional | Conformance::Mixed => {
                Err(unsupported_without_normalizer(policy))
            }
        },
    }
}

/// The error for a cell where a policy needs a normalizer it was not given.
fn unsupported_without_normalizer(policy: ConformancePolicy) -> StrictError {
    StrictError::Unsupported(format!(
        "{policy:?} policy requires a RawNormalizer to open Transitional or Mixed content"
    ))
}

#[cfg(test)]
mod tests {
    use super::decide;
    use crate::error::StrictError;
    use crate::ns::Conformance;
    use crate::opc::ConformancePolicy;

    /// All 20 cells of the matrix (5 rows × 4 columns): the table in
    /// [`decide`]'s doc comment, made executable. A row is `(policy,
    /// has_normalizer)`; a cell asserts `Ok` or the exact error variant.
    #[test]
    fn every_cell_of_the_matrix() {
        use Conformance::{Mixed, Strict, Transitional, Unknown};

        let rows: &[(ConformancePolicy, bool)] = &[
            (ConformancePolicy::StrictOnly, false),
            (ConformancePolicy::StrictOnly, true),
            (ConformancePolicy::Normalize, true),
            (ConformancePolicy::Normalize, false),
            (ConformancePolicy::Permissive, true),
            (ConformancePolicy::Permissive, false),
        ];

        for &(policy, has_normalizer) in rows {
            for &detected in &[Strict, Transitional, Mixed, Unknown] {
                let result = decide(policy, detected, has_normalizer);
                let ok = matches!(
                    (policy, detected, has_normalizer),
                    (ConformancePolicy::StrictOnly, Strict, _)
                        | (
                            ConformancePolicy::Normalize | ConformancePolicy::Permissive,
                            _,
                            true
                        )
                        | (ConformancePolicy::Permissive, Strict | Unknown, false)
                        | (ConformancePolicy::Normalize, Strict, false)
                );
                // Eager format keeps this diagnostic line in the §15 100% file gate.
                let detail = format!(
                    "{policy:?} / {detected:?} / has_normalizer={has_normalizer} -> {result:?}"
                );
                assert_eq!(result.is_ok(), ok, "{detail}");
                if !ok {
                    match (policy, detected) {
                        (ConformancePolicy::StrictOnly, Transitional) => assert!(matches!(
                            result,
                            Err(StrictError::TransitionalNotSupported { .. })
                        )),
                        (ConformancePolicy::StrictOnly, Mixed) => {
                            assert!(matches!(result, Err(StrictError::MixedConformance { .. })));
                        }
                        (ConformancePolicy::StrictOnly, Unknown) => assert!(matches!(
                            result,
                            Err(StrictError::UndeterminedConformance { .. })
                        )),
                        _ => assert!(matches!(result, Err(StrictError::Unsupported(_)))),
                    }
                }
            }
        }
    }

    #[test]
    fn strict_only_rejects_unknown() {
        // The bug this closes: `Unknown` used to be accepted under
        // `StrictOnly`, treating "no signal at all" as "Strict".
        assert!(matches!(
            decide(ConformancePolicy::StrictOnly, Conformance::Unknown, false),
            Err(StrictError::UndeterminedConformance { .. })
        ));
    }

    #[test]
    fn strict_only_rejects_mixed_even_with_a_normalizer() {
        // The bug this closes: `Mixed` used to be accepted under any policy,
        // including `StrictOnly`, as long as a normalizer was installed.
        assert!(matches!(
            decide(ConformancePolicy::StrictOnly, Conformance::Mixed, true),
            Err(StrictError::MixedConformance { .. })
        ));
    }

    #[test]
    fn permissive_without_a_normalizer_no_longer_opens_transitional_as_is() {
        // The bug this closes: `Permissive` used to accept Transitional
        // unconditionally, normalizer or not.
        assert!(matches!(
            decide(
                ConformancePolicy::Permissive,
                Conformance::Transitional,
                false
            ),
            Err(StrictError::Unsupported(_))
        ));
    }

    #[test]
    fn permissive_without_a_normalizer_still_opens_unknown() {
        // The one cell `Permissive` keeps that `Normalize` does not: a
        // package with no determinable conformance can still be inspected.
        assert!(decide(ConformancePolicy::Permissive, Conformance::Unknown, false).is_ok());
    }
}
