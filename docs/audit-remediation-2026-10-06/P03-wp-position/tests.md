# P3 — `wp:` position / wrap / extent tests

```powershell
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked inline_dist_and_doc_pr_title_are_parsed
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked anchor_positioning_and_wrap_are_parsed
cargo +1.92.0 test -p strict-ooxml-wml --test softuni_position_v --locked softuni_preserves_page_percent_position
```

| ID | Result |
|---|---|
| T-P3-1 inline extent + dist roundtrip | PASS |
| T-P3-2 floating positionH/V + wrap | PASS |
| T-P3-3 effectExtent zero vs non-zero | PASS |
| T-P3-4 visual wrap / position (SoftUni, pages 1–20) | PASS |

## Residuals

Witness-slice B_wp: **5** rows — all `wp:docPr@id` remaps on `070_` (uniqueness after expanded drawing trees). Declared transform with visual proof (T-P2-4 / T-P3-4).
