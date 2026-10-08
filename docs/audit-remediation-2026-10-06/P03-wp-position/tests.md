# P3 — `wp:` position / wrap / extent tests

```powershell
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked inline_dist_and_doc_pr_title_are_parsed
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked anchor_positioning_and_wrap_are_parsed
cargo +1.92.0 test -p strict-ooxml-wml --test softuni_position_v --locked softuni_preserves_page_percent_position
cargo +1.92.0 test -p strict-ooxml-write --lib --locked doc_pr_id_is_preserved_until_collision_forces_remap
```

| ID | Result |
|---|---|
| T-P3-1 inline extent + dist roundtrip | PASS |
| T-P3-2 floating positionH/V + wrap | PASS |
| T-P3-3 effectExtent zero vs non-zero | PASS |
| T-P3-4 visual wrap / position (SoftUni, pages 1–20) | PASS |

## Residuals / TZ

Witness-slice B_wp unclassified residuals: **0** after `TZ-48` (`wp:docPr@id` uniqueness remap, `requires_cited = ["wp:docPr"]`).

Negative: uncited foreign `wp:docPr@id` remap (no collision / no `wp:docPr` cite) stays unclassified → FAIL (`census_gate_selftest.test_docpr_id_remap_requires_citation`). Free source ids are preserved until a real collision.
