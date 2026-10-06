# P4 — WPS `bodyPr` / text-box tests

```powershell
cargo +1.92.0 test -p strict-ooxml-write --lib --locked body_pr_and_tx_box_are_written
```

| ID | Result |
|---|---|
| T-P4-1 parse/write `wps:bodyPr` key attrs | PASS |
| T-P4-2 text-box vert/wrap affects SVG (SoftUni) | PASS |
| T-P4-3 negative: drop non-default wrap → report / fail gate | PASS |

## Slice

Witness-slice C_wps residual rows: **0**.
