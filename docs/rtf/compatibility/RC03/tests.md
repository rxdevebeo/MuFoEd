# RC03 shaping and bidirectional witnesses

Contract and profile: [FONT_RESOURCE_CONTRACT.md](../../FONT_RESOURCE_CONTRACT.md),
[RC02 profile](../RC02/profile.md). Card: [STATUS.json](STATUS.json).

## Pinned complex-script witnesses (2026-10-08)

RC03 reuses the RC02 machinery: the renderer shapes through the existing
harfrust/cluster pipeline (`strict-ooxml-render-svg/src/font/shape.rs`); no second
RTF shaper exists. To give RC03 its own independent geometry oracle, three pinned
OFL witnesses were added next to the CJK and Devanagari ones, under
`strict-ooxml-render-svg/fonts/witnesses/`:

| id | family | source | fixture | script |
| --- | --- | --- | --- | --- |
| arabic | StrictLib Witness Arabic | NotoSansArabic-Regular.ttf @ `ffebf8c` | `arabic.ttf` | Arabic (RTL, joining, ASCII digits) |
| tamil | StrictLib Witness Tamil | NotoSansTamil-Regular.ttf @ `ffebf8c` | `tamil.ttf` | Tamil (Indic, combining signs) |
| bengali | StrictLib Witness Bengali | NotoSansBengali-Regular.ttf @ `ffebf8c` | `bengali.ttf` | Bengali (Indic, conjuncts) |

Each fixture is a finite subset with all layout features and closure, renamed
family/PostScript/CFF names, and unchanged timestamps — test assets only, never
runtime fallbacks. `fonts/witnesses/manifest.json` records source/fixture SHA-256,
license file and the oracle method. Licenses are the verbatim OFL text
(`Arabic-OFL.txt`, `Tamil-OFL.txt`, `Bengali-OFL.txt`, all identical to the
`noto-fonts` `LICENSE`); copyright strings come from each font's name table.

Regenerate with the same toolchain as RC02 (fontTools 4.60.1, uharfbuzz 0.56.3):

```powershell
$env:PYTHONPATH = "target/rtf-import/font-witness-python"
python -X utf8 xtool/rtf/font_witnesses.py --source target/rtf-import/font-witness-source --output target/rtf-import/font-witness-rc03
# the cjk.otf and devanagari.ttf fixtures reproduce byte-identically with 4.60.1
```

`strict-ooxml-render-svg/tests/font_witnesses.rs` shapes each witness through the
shared pipeline and compares glyph id, UTF-8 cluster, x/y advance, x/y offset and
origin against the saved native HarfBuzz TSV. All five scripts match.

## Logical order and bidirectional reordering

The same test file keeps the pipeline honest on three fronts RC03 cares about:

- `unicode_cluster_map` reconstructs the logical string exactly (no scalar lost or
  reordered in the logical layer);
- every cluster span is non-empty and on `char` boundaries;
- `unicode_x_positions_px` returns exactly one position per logical scalar, so a
  cluster (ligature or mark) shares one origin.

For the Arabic witness the logical first scalar sits right of the visual left edge,
which is only true if the glyphs were reordered rather than accumulated
left-to-right. Tamil/Bengali stay left-to-right while still exercising combining
and conjunct substitution.

## Line-break boundaries

`long_devanagari_token_breaks_at_cluster_boundaries` renders an unbreakable
Devanagari token (120 repetitions of a conjunct) through the witness face and
checks that the fallback wrap cuts only at cluster boundaries: no line piece
starts with a dependent sign (`U+093A`, `U+093C`, `U+093E..=U+094F`,
`U+0951..=U+0957`, `U+0962..=U+0963`), and concatenating the pieces returns the
exact logical string. This guards the RC03 "break boundaries" requirement against
the line pieces that `place_long_token` produces.

## PDF Unicode extraction for the new scripts

`strict-ooxml-render-pdf/tests/pdf.rs::licensed_programs_embed_with_cjk_unicode_and_explicit_indic_degradation`
now embeds and extracts all five witnesses. For each it checks one embedded
program, no missing/shaping/embedding diagnostic, the exact ActualText structure
(`verify_cluster_actual_text`), and the raw `ToUnicode` layer:

- CJK extracts exactly (no continuation CIDs);
- Devanagari/Tamil/Bengali keep every logical character once the documented
  replacement characters are removed;
- Arabic (RTL) is asserted by logical character presence because the content
  stream is visual order and the `lopdf` helper ignores ActualText.

The independent MuPDF gate was extended to all five and passes:
`python xtool/rtf/font_extraction_witness.py --pdfs <dir> --receipt
target/rtf-import/RC03-actualtext-extraction.json` (PyMuPDF 1.26.4 / MuPDF
1.26.7), after generating the PDFs with `STRICTLIB_FONT_WITNESS_OUTPUT`.

## Not yet closed

- The corpus witnesses 007/010/011/018 need their real system faces; those are not
  bundled or licensed here and no rasterizer is available, so they are Blocked.
- Mixed LTR/RTL segmentation inside one run remains open.
