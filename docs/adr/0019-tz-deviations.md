# ADR-0019: Accepted deviations from `TZ-STRICT-OOXML-RUST.md`

- **Status:** Accepted
- **Date:** 2026-10-04
- **Deciders:** Strict OOXML maintainers
- **Related:** AUD-90 (`REWORK-AUDIT-2026-10.md`), AUD-20 / ADR-0015,
  AUD-23 / ADR-0016, AUD-26, AUD-34, `TZ-STRICT-OOXML-RUST.md` (2.1),
  `STAGE-8-TASK.md`

## Context

The technical specification (`TZ-STRICT-OOXML-RUST.md` 2.0) predates Stages 6–8
and several audit decisions. In a few places the implemented library deliberately
differs from the letter of the TZ. Those differences were already decided in
earlier AUDs; this record names them so the TZ can be edited in AUD-91 without
re-litigating each one, and so a future reader does not treat the gap as a bug.

## Decision

The following TZ statements are **accepted deviations**. The implementation
keeps the behaviour in the "Implementation" column; the TZ is amended in AUD-91
to match.

| TZ (2.0) | Implementation | Why it stays |
|---|---|---|
| §7.3 «модель неизменяема» after parse | `StrictDocument::document_mut()` exists | The parser still builds an immutable snapshot; mutation is an explicit API for write/edit paths (Stage 8+). |
| §7.3 `SourceLocation` on every node | Absent on `TextNode`, `Symbol`, `FieldChar`, `Bookmark`, `GridCol`, `FootnoteRef` | Location on `Run` / `Paragraph` / `Table` is enough for Feature Report / Loss Report; per-leaf locations would bloat the model without a consumer. |
| §5.4 feature `round-trip-normalize` | Normalizer lives in `strict-ooxml-core` always; no Cargo feature | Normalization is part of the core pipeline (`ConformancePolicy::Normalize`); a feature gate would hide the only path that accepts Transitional. |
| §5.4 / G.7 `default = svg + report` was also drifted to include `write, pdf, convert` | **Corrected:** meta-crate `default = ["report", "svg"]` | Heavy backends stay opt-in. CLI and other binaries that need them list features explicitly in their `Cargo.toml`. CI `check (default features)` enforces the thin default. |
| §6.3 `resolve_theme`, `load_media` on `OpenOptions` | Not present | Theme resolution is always on; media loading is controlled by `RenderOptions::MediaMode` (lazy). Separate open-time flags would duplicate policy. |
| §6.3 `VmlFallback::RasterizeIfPossible` | Renamed to `VmlFallback::Convert` (AUD-34) | The path converts VML to DrawingML; it does not rasterize. The old name was misleading. |
| §4.1 п.4 content-types as a conformance **signal** | Signal removed (AUD-26); MIME of the main part is a **check** | Wrong MIME is `UnexpectedContentType` / `T2.content-type`, not a detector bit. A content-type bit was dead and lied about OPC family. |
| §9.3 OPC package URI table (Strict purl forms) | Family-neutral openxmlformats URIs (AUD-20 / ADR-0015) | ECMA-376 Part 2 has one OPC vocabulary; Word/LibreOffice Strict packages use it. |
| §3.2 «редактирование / сохранение вне scope» | `write` feature and `document_mut` exist | Stages 8+ expanded scope; see `STAGE-8-TASK.md`. Reading/normalization remain the primary product. |

## Consequences

- Positive: TZ 2.1 (AUD-91) can cite this ADR instead of contradicting the code;
  the meta-crate default build stays small and matches the TZ promise.
- Negative: consumers that relied on `default` pulling in `write` / `pdf` /
  `convert` must enable those features explicitly (the CLI does).
- Validation: `cargo check --workspace --all-targets` (no `--all-features`) and
  the CLI binary with its explicit feature list both succeed.

## Alternatives considered

- Keep the fat meta-crate default — rejected; it contradicted TZ §5.4 / G.7 and
  hid missing feature gates in dependents.
- Introduce `round-trip-normalize` as a real feature — rejected; every
  `Normalize` open would need it, and core already owns the normalizer.
- Add leaf `SourceLocation` fields — rejected; cost without a report consumer.
