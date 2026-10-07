# RC02 registry and resource consumer tests

Contract/limits/validation: [FONT_RESOURCE_CONTRACT.md](../../FONT_RESOURCE_CONTRACT.md).

- Six `tests/font_assets.rs` public API regressions verify exact style lookup, deterministic
  enumeration, shared lifetime, owned metrics/shaping parity, budgets, pinned hashes,
  metadata/index/variation rejection and OS/2 flags overriding permission attestations.
- Missing CJK glyphs remain explicit; this negative witness is not CJK support acceptance.
- Earlier resource/shape-cache regressions remain in the full suite.
- Full SVG/PDF/RTF suites: 418 Pass with the existing single alias-test exclusion.
- Final registry suite: 6 Pass. MSRV Clippy retains only the recorded baseline suppressions.
- Renderer admission and fresh corpus/XSD/OPC/pixel receipts remain pending.
