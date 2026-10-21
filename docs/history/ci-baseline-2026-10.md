# CI baseline, 2026-10 (REWORK-AUDIT-2026-10, AUD-00)

Remote: <https://github.com/rxdevebeo/MuFoEd>, branch `master`. Before 2026-10-02 the
repository had no remote, so `.github/workflows/ci.yml` had never run.

## First run on the unchanged tree

Run [37068095330](https://github.com/rxdevebeo/MuFoEd/actions/runs/37068095330),
commit `1af8988`.

| Job | Result | Cause | Closed by |
|---|---|---|---|
| test (ubuntu, macos, windows) | red at Clippy | the job used floating `stable` (1.99), whose new `while_let_loop` fired on `write/src/passthrough.rs:743`; no test step ever ran | toolchain pinned to 1.92.0 in every job (`13a66dc`) |
| fuzz smoke | red | `taiki-e/install-action@cargo-fuzz` installed nothing; then the musl build of cargo-fuzz defaulted to `--target x86_64-unknown-linux-musl`, where ASan cannot link | `install-action@v2` with `tool:` (`13a66dc`), explicit `--target x86_64-unknown-linux-gnu` (`1007ab4`) |
| Census gate | red | `strict-ooxml-core/tests/docx/` is gitignored (local corpus, never published) and the gate requires it | job removed; run by hand, waiver `CENSUS-LOCAL` |
| XSD gate, coverage, msrv, cargo-deny | green | | |

Found once the first set was fixed:

| Job | Cause | Closed by |
|---|---|---|
| test (windows) | `core.autocrlf` checked the golden SVGs out as CRLF; then `rustfmt`'s `newline_style = "Native"` demanded CRLF from LF sources | `.gitattributes` `* text=auto eol=lf` (`1007ab4`), `newline_style = "Unix"` |
| cargo-deny | `strict-ooxml-testkit` is a path-only dev-dependency (a version would make `cargo publish` look for it on crates.io) | `allow-wildcard-paths = true` (`1007ab4`) |

The Node 20 deprecation warnings are gone: `actions/checkout@v5`, `actions/setup-python@v6`.

## Decided

- **Census gate.** It measures two Transitional corpora, `tests/samples/` (committed)
  and `tests/docx/` (local only, not to be published). The owner decided on
  2026-10-03 to keep the gate local: the job is gone from CI and the gap is waiver
  `CENSUS-LOCAL` in `docs/waivers.toml`.

## After (AUD-94 / F9, 2026-10-04)

Local gate on the F9 tip (`59be16d` + this commit), toolchain `1.92.0`:

| Check | Result |
|---|---|
| `cargo +1.92.0 fmt --all -- --check` | PASS |
| `cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo +1.92.0 check --workspace --all-targets` (default features) | PASS |
| `cargo +1.92.0 test --workspace --all-features` | **FAIL** — pre-existing `strict-stage5c` page 1 layout drift (SVG + PDF SSIM / extent ratchet); see blockers |
| same with SSIM/`pdf_pixels` skipped | PASS |
| `word_oracle` on `tests/strict/` | PASS |
| `opc_gate.py` / `xsd_gate.py` | PASS (0 violations of ours) |
| `census_gate.py` | local only (`CENSUS-LOCAL`) |
| AUD-92 1 h × 8 targets | started on WSL; CI `fuzz-nightly` is the durable evidence path |

CI run URL after the F9 push is recorded below once Actions finishes.

### Blockers remaining after F9

1. ~~**AUD-17, AUD-37, AUD-38, AUD-68, AUD-69**~~ — закрыты 2026-10-04; корпус в `docx/`,
   `rec.docx` удалён (`docs/corpus-incoming.md`).
2. **`strict-stage5c` page 1** — SVG/PDF pixel gates fail on `master` already
   (bottom ink +240 px, `corr_y ≈ 0.24`). Reproduced at `7bad7e8` before F9;
   not introduced by AUD-90…94. Needs a layout fix outside F9.
3. **FUZZ 24 h** — Stage-7 release gate (`docs/fuzz-protocol.md` / waiver `FUZZ`).
4. **Coverage §15** — not re-measured in this F9 close-out (CI coverage job also
   red on the same `strict-stage5c` SSIM failure).

### CI after F9 push

Run <https://github.com/rxdevebeo/MuFoEd/actions/runs/37165495480>, tip
`683ed3b` (AUD-94 + hash naming). Expected red on `test` / `coverage` for the
pre-existing `strict-stage5c` page-1 layout failure (`STAGE5C-P1-LAYOUT`);
`fuzz-smoke` should exercise all eight targets.
