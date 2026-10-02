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
| Census gate | red | `strict-ooxml-core/tests/docx/` is gitignored (local corpus, never published) and the gate requires it | **open**, see below |
| XSD gate, coverage, msrv, cargo-deny | green | | |

Found once the first set was fixed:

| Job | Cause | Closed by |
|---|---|---|
| test (windows) | `core.autocrlf` checked the golden SVGs out as CRLF; then `rustfmt`'s `newline_style = "Native"` demanded CRLF from LF sources | `.gitattributes` `* text=auto eol=lf` (`1007ab4`), `newline_style = "Unix"` |
| cargo-deny | `strict-ooxml-testkit` is a path-only dev-dependency (a version would make `cargo publish` look for it on crates.io) | `allow-wildcard-paths = true` (`1007ab4`) |

The Node 20 deprecation warnings are gone: `actions/checkout@v5`, `actions/setup-python@v6`.

## Open

- **Census gate.** It measures two Transitional corpora, `tests/samples/` (committed)
  and `tests/docx/` (local only). Until the owner decides whether `tests/docx/` can be
  published, the job is red by construction and is not evidence about the code.

## After

Filled in by AUD-94: the run on `master` after the whole plan is closed.
