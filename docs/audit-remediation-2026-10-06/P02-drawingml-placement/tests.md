# P2 — DrawingML placement tests

## Unit / integration

```powershell
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked locked_canvas_preserves_off_ext_and_ch_off
cargo +1.92.0 test -p strict-ooxml-wml --test drawing_stage5b --locked group_parses_children_and_transform
cargo +1.92.0 test -p strict-ooxml-write --lib --locked locked_canvas_markup_is_emitted_with_placement
cargo +1.92.0 test -p strict-ooxml-write --lib --locked changing_offset_by_one_emu_is_visible
```

| ID | Result |
|---|---|
| T-P2-1 Roundtrip `a:xfrm` off/ext via locked canvas markup | PASS |
| T-P2-2 Nested group `chOff`/`chExt` preserved (wpg + locked canvas) | PASS |
| T-P2-3 Negative: offset ±1 EMU not equal | PASS |
| T-P2-4 SVG bbox on `070_` (≥3 figures, pages 1–30) | PASS |
| T-P2-4-negative 1-inch `a:off` shift must break match | PASS |

## Slice

Witness-slice A_placement residual rows: **0**.
`corpus_slice_exit=1` from pre-existing `unmatched_schema≈1182` (not A_placement).
