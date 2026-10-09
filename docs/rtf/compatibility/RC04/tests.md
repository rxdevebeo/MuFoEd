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

## Run effects, kerning and position (2026-10-08)

A second slice closes run properties that the reader was dropping and the writer
already supports:

| RTF | WML | Rule |
| --- | --- | --- |
| `\outl` | `w:rPr/w:outline` | toggle |
| `\shad` | `w:rPr/w:shadow` | toggle (`\shad0` off) |
| `\embo` | `w:rPr/w:emboss` | toggle |
| `\impr` | `w:rPr/w:imprint` | toggle |
| `\noproof` | `w:rPr/w:noProof` | toggle |
| `\snaptogrid` | `w:rPr/w:snapToGrid` | toggle |
| `\kernN` | `w:rPr/w:kern` | minimum kerning size in half-points; negative is an explicit Loss |
| `\positionN` | `w:rPr/w:position` | signed half-points |
| `\upN` / `\dnN` | `w:rPr/w:position` | +N / -N half-points |

`strict-ooxml-rtf/tests/structure.rs::run_effects_kerning_and_position_roundtrip`
checks each through import → Strict write → reopen. The writer already emitted all
of these (`strict-ooxml-write/src/props.rs`), so the fix is the missing mapping,
not new schema work.

## Not yet closed

- Independent effective-style facts from the WPS reference index (the corpus
  cascade: document defaults → paragraph style → character style → direct), the
  tab/leader and keep/widow geometry probes on 019/039 then 052/076, and section
  defaults.
- Targeted pixel/geometry acceptance needs a rasterizer, which is not installed
  here; that half stays Blocked.
