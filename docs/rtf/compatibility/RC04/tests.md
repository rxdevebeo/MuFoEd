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

## Corpus effective-facts probe (019/039, 2026-10-08)

Method: import the RTF with the corpus options (`--wps-text
--allow-unsigned-unicode --drop-nul`), extract facts from our DOCX with
`xtool/rtf/docx_facts.py::inspect`, and compare against the WPS reference facts
(`target/rtf-import/R01/wps-facts-v2/files/039/facts.json`) at both the
construct-count and per-paragraph level.

For 039 the per-paragraph comparison shows our run carries
`rFonts {ascii,hAnsi,eastAsia,cs}` + `color auto` + `sz 24`, while WPS mirrors
`szCs 24` on every run and writes `w:spacing/beforeLines/afterLines` and
docDefaults `w:kern`. Those are Word/WPS writer defaults (docDefaults and mirror
`szCs`), not a required RTF control mapping — this is exactly the "WPS quirk vs
correct RTF preservation" split the plan asks for, so no global default is
invented here. Count-level comparison is dominated by styles.xml and is recorded
as provisional, not as effective-property equality.

Provisional observation only; the effective-style acceptance for the corpus is
still open.

## Corpus geometry fixes (2026-10-09)

The 97-pixel triage (`../PIXEL_TRIAGE.md`) isolated the dominant failure as
`line_metrics` (402/417 pages). Three proven, independent geometry bugs were
fixed, each pinned by an import → Strict write → reopen test:

| Fix | Symptom | Witness |
| --- | --- | --- |
| `\deff` absent → the first font-table entry now reaches `w:docDefaults/w:rPrDefault/w:rPr/w:rFonts` | paragraph marks without `\f` fell back to the renderer's Carlito default and empty-line pitch drifted | 013 COPYING paragraph gap 27.6pt → 27.6pt (was 28.4) |
| empty paragraph marks carry the ambient run formatting (`w:pPr/w:rPr`) | a blank line used the 12pt document default instead of the 10pt body, shifting the rest of the page | 054 Hotyn2 empty-line gap 25.3pt → 23.0pt |
| `render-svg::map_family` strips script qualifiers (`Times New Roman Cyr`, `Arial CE`, `Calibri (Hebrew)`, `Cambria Math Cyr`, …) before mapping | the per-charset font name fell through to the Carlito wildcard, so both line pitch and advances used the wrong face | 054 Hotyn2 line pitch 12.2pt → 11.5pt |

Tests: `structure.rs::absent_deff_defaults_to_the_first_font_in_the_table`,
`explicit_deff_wins_over_the_first_font_in_the_table`,
`empty_paragraph_mark_carries_the_ambient_run_size`; the mapping is pinned by
`font::family::tests::strips_script_qualifiers_before_mapping`.

Measured on the corpus (receipts `target/rtf-import/RC04-parmark-*`):

| Gate | Before | After |
| --- | --- | --- |
| import text/resource | 100/100 | 100/100 |
| Strict XSD | 100/100, 0 violations | 100/100, 0 violations |
| OPC | 100 packages, 0 violations | 100 packages, 0 violations |
| pagination | 62/100 | **65/100** |
| pixels | 3/100 docs, 6/423 pages | **6/100 docs, 22/458 pages**, 0 regressions |

## Resilience of the fix

The default-font and family fixes are the RTF-correct reading: an absent `\deff`
means font 0 (RTF 1.9.1), and a script-qualified name is the same design under a
different char-set. The paragraph-mark fix restores the ambient run formatting
that the paragraph mark carries in the source rather than inventing a default.

## Not yet closed

- Residual corpus pixels: the remaining `line_metrics` pages fail on a shared
  ~1pt baseline/leading offset (P5 metrics) and rasterizer glyph-ink (row-ink
  correlation), plus a WPS quirk where leading spaces advance by ~0.5em while
  internal spaces match our Arimo/real advance (013 witness). None of these is a
  missing RTF mapping; they need the shared metrics queue and exact faces.
- Independent effective-style facts from the WPS reference index for the full
  corpus cascade remain open; the 019/039 probes are provisional.

