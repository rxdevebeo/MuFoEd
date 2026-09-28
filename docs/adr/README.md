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

Copy `0000-template.md` when adding a new record.
