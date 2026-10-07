# RC02 registry and renderer tests

Contract/limits/validation: [FONT_RESOURCE_CONTRACT.md](../../FONT_RESOURCE_CONTRACT.md).

## Earlier API-only slice

- Six `tests/font_assets.rs` public API regressions verify exact style lookup, deterministic
  enumeration, shared lifetime, owned metrics/shaping parity, budgets, pinned hashes,
  metadata/index/variation rejection and OS/2 flags overriding permission attestations.
- Missing CJK glyphs remain explicit; this negative witness is not CJK support acceptance.
- Earlier resource/shape-cache regressions remain in the full suite.
- Full SVG/PDF/RTF suites: 418 Pass with the existing single alias-test exclusion.
- Final registry suite: 6 Pass. MSRV Clippy retains only the recorded baseline suppressions.
- Renderer admission and fresh corpus/XSD/OPC/pixel receipts remain pending.


## Renderer admission slice

- Ten registry/SVG tests cover layout/embedded identity, retained ownership, escaped
  CSS instance settings, unchanged default output, missing script glyphs and TTC rejection.
- PDF independent extraction/parse witnesses cover resource/weight collisions,
  collection index 1, combining marks, deterministic output and ownership after options drop.
- RTF public integration resolves both Latin and East Asian aliases through the same registry.
- 425 full-suite Pass (52 groups, same baseline alias exclusion); final SVG 10 Pass.
- Clippy passes on MSRV with previously recorded baseline suppressions; formatting
  and diff checks are recorded in STATUS.json.
- Default corpus: 100 text/integrity/resource Pass; Strict XSD and OPC: 100, zero violations.
- These default CLI measurements and deliberately missing CJK fixtures do not establish
  positive CJK/Indic or full visual acceptance.

Fresh pagination is 62/100 Pass; prior pixels remain 3/100 Pass, carried forward
by exact equality of all actual/reference PDF hashes and page correspondence.
No fresh raster run is claimed; see pixel equivalence receipt in STATUS.json.
