# Real Strict OOXML corpus (`tests/strict/`)

Real, externally produced **WordprocessingML Strict** documents
(`purl.oclc.org/ooxml/...` namespaces), as opposed to the synthetic Strict
packages built in memory by the tests and the Transitional corpus in
`../samples/`.

## Contents

| File | Origin | License | Status |
|---|---|---|---|
| `strict-profile.docx` | [kklimuk/docx-cli](https://github.com/kklimuk/docx-cli) — `tests/fixtures/strict-profile.docx` | MIT (© 2026 Kirill Klimuk) | committed |

`strict-profile.docx` is redistributed under the MIT License of its source
repository; the original license text is preserved in the source project. It is
used here only as a test fixture.

> Note: an additional real Strict fixture was found in
> `Esword618/unioffice` (`document/testdata/strict.docx`), but that repository is
> **AGPL-3.0**, which is incompatible with this project's MIT/Apache-2.0
> licensing, so it is **not** committed (local use only).

## Status

As of the current commit these fixtures are **detected** as Strict
(`conformance: Strict`) but do **not** parse yet: they place the XML declaration
and the root element on separate lines, and the Stage-2 parser rejects leading
prolog whitespace (`REWORK-WML-1.md`, finding C-3). Once C-3 is fixed, this
directory is exercised by a dedicated Strict parse/report/render test.
