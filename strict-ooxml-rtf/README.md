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
page geometry and section boundaries/types, paragraph indents/spacing, ordinary table rows/cells and vertical
merge markers, horizontal merged cells and final empty cell paragraphs, and
embedded PNG/JPEG from hex or binary payloads. Resource budgets
bound input, tokens, depth, text, nodes, definitions, table structure and media.

Unsupported controls/destinations produce named loss records; callers can reject
them. Unknown ignorable destinations are skipped by group. Nested tables are
always rejected because a safe structural fallback is not implemented. Symbol
font bytes retain their font and glyph code through `w:sym`; visible glyphs still
require the correct font. OEM codepages return explicit errors. `ansicpg0` accepts
ASCII/Unicode-only content but rejects high bytes without a known encoding.
Non-signed Unicode parameters return errors by default; explicitly set
`UnicodeParameterPolicy::AllowUnsignedAndReport` to accept values through 65535
with informational diagnostics. Replacement
of invalid text is opt-in and reported. No machine-default encoding is guessed.

For the independently checked WPS corpus text behavior, opt in explicitly:

```rust
use strict_ooxml_rtf::{CompatibilityProfile, NulPolicy, RtfOptions, UnicodeParameterPolicy};
let options = RtfOptions {
    compatibility_profile: CompatibilityProfile::WpsText,
    nul_policy: NulPolicy::DropAndReport,
    unicode_parameter_policy: UnicodeParameterPolicy::AllowUnsignedAndReport,
    ..RtfOptions::default()
};
```

`DropAndReport` discards only U+0000, retains `report.discarded_nuls` even when
diagnostic details are capped, and reports a loss. Other invalid text remains
subject to `invalid_text_policy`. `WpsText` preserves leading raw line endings in
literal groups, ignores header whitespace before a control word or ignorable
destination, maps discretionary hyphens to ordinary hyphens, and reproduces the
checked inline `AppleTypeServices` / `CocoaLigature1` spellings. These content
changes are losses; unsupported rejection still stops them. This is a bounded
text compatibility profile, not complete emulation of WPS's RTF reader.

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
Symbol glyph codes are compared in the PUA text view used by WPS, with their font
identity separately covered by the Rust roundtrip test. To exercise legacy Cocoa
Unicode in the corpus runner, set `RTF_ALLOW_UNSIGNED_UNICODE=1` explicitly.
Use `RTF_DROP_NUL=1` and `RTF_WPS_TEXT=1` to exercise the other opt-ins; each
run records its actual options and discarded NUL count in `results.json`.
