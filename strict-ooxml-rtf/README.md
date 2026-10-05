# strict-ooxml-rtf

RTF bytes → owned `strict-ooxml-wml::model::Document`, embedded media and an
import-only diagnostic report. The runtime uses no Office process, filesystem,
network, OCR or intermediate PDF. WPS is an independent development oracle.

```rust
use strict_ooxml_rtf::{import, RtfOptions};
let imported = import(br"{\rtf1 Hello {\b world}!}", &RtfOptions::default())?;
assert_eq!(imported.report.loss_count, 0);
# Ok::<(), strict_ooxml_rtf::RtfError>(())
```

Save through `strict-ooxml-write::write_package`. Put `imported.media` into its
`package::MediaBag` and pass the bag as source. The writer allocates its own
part names, so validate reopened media through the reopened document's index.
Keep import and write reports separate. `Ok` means parsing completed; inspect
`loss_count` and `details_truncated` before choosing a fidelity verdict.

Implemented: iterative group/binary lexer, scoped character/paragraph properties,
codepages and font charset selection, signed UTF-16 with fallback and surrogate
pairs, `upr/ud`, escaped paragraph endings, text/tabs/breaks, basic fonts/colors,
page geometry, paragraph indents/spacing, ordinary table rows/cells and vertical
merge markers, and embedded PNG/JPEG from hex or binary payloads. Resource budgets
bound input, tokens, depth, text, nodes, definitions, table structure and media.

Unsupported controls/destinations produce named loss records; callers can reject
them. Unknown ignorable destinations are skipped by group. Nested tables are
always rejected because a safe structural fallback is not implemented. OEM/Symbol
codepages and non-signed Unicode parameters return explicit errors. Replacement
of invalid text is opt-in and reported. No machine-default encoding is guessed.

This is a development implementation, with incomplete list/style/region/field/
revision/section/merge fidelity. It has no facade or application integration yet.
Picture signatures and bounds are checked; full image decoding is outside this
crate. See `docs/rtf/IMPLEMENTATION_STATUS.md` for measured coverage and remaining
gates; corpus success alone does not prove release readiness.

```powershell
cargo +1.92.0 test -p strict-ooxml-rtf
cargo +1.92.0 clippy -p strict-ooxml-rtf --all-targets -- -D warnings
cargo +1.92.0 run -p strict-ooxml-rtf --example corpus -- `
  testdata/CC0_RTF target/rtf-import/R01/wps-facts-v2 target/rtf-import/NEW-RUN
```

The runner requires a new output directory and produces exactly 100 records for
ordinary import errors; process crashes/timeouts are not yet supervised. Body text
comparison preserves Unicode, spaces, tabs, breaks and paragraph boundaries.
