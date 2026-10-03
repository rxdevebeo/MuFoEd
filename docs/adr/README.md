# Architecture Decision Records

Accepted architectural decisions for the `strict-ooxml` toolkit. Each record is
immutable once accepted; a superseding decision gets a new number.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-zip-backend.md) | Own ZIP reader on top of `miniz_oxide` | Accepted |
| [0002](0002-xml-backend.md) | `quick-xml` tokenizer with an owned limiting wrapper | Accepted |
| [0003](0003-xml-event-ownership.md) | Owned XML events and a self-positioning `XmlReader` | Accepted |
| [0004](0004-wml-model.md) | WordprocessingML Strict DOM, event parser and two-phase assembly | Accepted |
| [0005](0005-report.md) | Feature Report: JSON stack, locations, severity and aggregation | Accepted |
| [0006](0006-render.md) | SVG rendering: font metrics, style cascade, media and SSIM references | Accepted |
| [0007](0007-writing.md) | Writing Strict: one package writer, one loss report, one order function | Accepted (W7 open) |
| [0008](0008-render-backends.md) | One layout, many backends — and the cost of a second consumer | Accepted |
| [0009](0009-pdf-reading.md) | Reading PDF: `lopdf` for objects, our own for meaning | Accepted |
| [0010](0010-tables-from-grid.md) | A table is a grid of ruling lines — and a missing rule is a merge | Accepted |
| [0011](0011-msrv.md) | One MSRV for the workspace — 1.92 | Accepted |
| [0012](0012-djvu-reading.md) | Reading DjVu: `djvu-rs` subcrates for plumbing, our own for the text layer | Accepted |
| [0014](0014-strict-conformance-post-mce.md) | Strict conformance is defined post-MCE — extension namespaces do not go into a Strict package | Accepted |
| [0015](0015-opc-namespaces.md) | OPC namespaces and relationship types are family-neutral | Accepted |
| [0016](0016-conformance-policy.md) | One conformance-policy matrix, decided from raw (T0) signals | Accepted |
| [0017](0017-normalization-report.md) | Per-part NormalizationReport, merge on read | Accepted |
| [0018](0018-revisions.md) | Tracked-change model for `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` | Accepted |
| [0019](0019-tz-deviations.md) | Accepted deviations from `TZ-STRICT-OOXML-RUST.md` | Accepted |

Copy `0000-template.md` when adding a new record.
