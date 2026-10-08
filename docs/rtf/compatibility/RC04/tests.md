# RC04 style cascade and text geometry

Card: [STATUS.json](STATUS.json). Plan: [REMAINING_COMPATIBILITY_PLAN.md](../../REMAINING_COMPATIBILITY_PLAN.md).

## Character geometry mapping (2026-10-08)

RC04 owns the RTF mapping and witnesses; shared metric fixes come from P5. The
first slice closes three run/paragraph controls the RTF reader was dropping and
pins them with an import → Strict write → reopen regression
(`strict-ooxml-rtf/tests/structure.rs`):

| RTF | WML | Rule |
| --- | --- | --- |
| `\charscalexN` | `w:rPr/w:w` | percentage; `0..=600` accepted, out-of-range is an explicit Loss |
| `\expndN` | `w:rPr/w:spacing` | quarter-points → twips (`N * 5`) |
| `\expndtwN` | `w:rPr/w:spacing` | twips |
| `\tlth` | `w:tab/@w:leader="heavy"` | leader must precede the `\tx` that creates the stop |
| `\tleq` | — | no WML leader; explicit Loss `tab:leader-equal-unsupported` |
| `\aexpnd` | — | associated-script spacing has no WML run slot; explicit Loss |

`\plain` resets both `scale` and `spacing` because it resets the whole
`RunProperties`. `Twips` multiplication is checked, so an overflowing `\expnd` is
an explicit Loss rather than a wrapped value. The unit distinction matters: one
quarter-point is five twips, which is why `\expnd4` is 20 twips.

## Not yet closed

- Independent effective-style facts from the WPS reference index (the corpus
  cascade: document defaults → paragraph style → character style → direct), the
  tab/leader and keep/widow geometry probes on 019/039 then 052/076, and section
  defaults.
- Targeted pixel/geometry acceptance needs a rasterizer, which is not installed
  here; that half stays Blocked.
