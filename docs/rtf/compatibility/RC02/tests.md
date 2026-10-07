# RC02 contract tests

Contract and exact commands: [FONT_RESOURCE_CONTRACT.md](../../FONT_RESOURCE_CONTRACT.md).

- Four resource tests cover lifetime/identity/provenance and invalid inputs.
- The cache regression supplies two real font programs under the same family/style.
  It must retain the correct program hash and advances after alternating requests.
- Full SVG/PDF/public API suites pass with one recorded baseline alias-test exclusion.
- MSRV Clippy retains the recorded baseline suppressions; changed-file formatting passes.
- These tests do not admit external assets into renderers or establish CJK/Indic support.
