# RC06 tables

Card: [STATUS.json](STATUS.json). Plan:
[REMAINING_COMPATIBILITY_PLAN.md](../../REMAINING_COMPATIBILITY_PLAN.md).

## Table look and bidi (2026-10-10)

`\tbllk*` are `w:tblLook` conditional-formatting flags and `\rtlrow` is
`w:bidiVisual`; both are table-level in the Strict model and are applied at table
creation from the row definition:

| RTF | WML |
| --- | --- |
| `\tbllkhdrrows` | `w:tblLook/@w:firstRow` |
| `\tbllkhdrcols` | `w:tblLook/@w:firstColumn` |
| `\tbllklastrow` | `w:tblLook/@w:lastRow` |
| `\tbllklastcol` | `w:tblLook/@w:lastColumn` |
| `\tbllknocolband` | `w:tblLook/@w:noVBand` |
| `\tbllknorowband` | `w:tblLook/@w:noHBand` |
| `\rtlrow` / `\ltrrow` | `w:tblPr/w:bidiVisual` |

`structure.rs::table_look_and_bidi_map_to_table_properties` pins it through
import -> Strict write -> reopen. The renderer's conditional-formatting pass
consumes `w:tblLook`; five corpus documents gain the properties but all 100
rendered PDFs stay byte-identical.

## Table compat controls (2026-10-10)

`\cgrid`, `\snaptogridincell` (character grid), `\nocxsptable`,
`\nogrowautofit`, `\usenormstyforlist` (table-growth/format hints) and
`\irow`/`\irowband`/`\lastrow` (conditional-formatting band ids) have no WML
equivalent and become Info. `structure.rs::table_compat_controls_are_info_not_loss`
pins it. Corpus import losses fall by a further 1354.

## Already mapped (unchanged here)

Row/cell grid (`\trowd`/`\cellx`/`\trleft`), horizontal merge
(`\clmgf`/`\clmrg`), vertical merge (`\clvmgf`/`\clvmrg`), vertical align
(`\clvertalt`/`\clvertalc`/`\clvertalb`), header rows (`\trhdr`) and row
`cantSplit` (`\trkeep`) are mapped; nested tables are explicitly Rejected
(`nested-table-structure`) rather than flattened. The shared table geometry and
units come from P6.

## Open

- **Borders and shading** are entirely unmapped (~35k Loss): paragraph borders
  (`\brdrt`/`\brdrb`/`\brdrl`/`\brdrr`/`\brdrbar`/`\brdrbtw`), cell borders
  (`\clbrdr{t,b,l,r}`), table borders (`\brdrtbl`), row borders (`\trbrdr*`),
  page borders (`\pgbrdr*`) and the per-edge style/width/colour tokens
  (`\brdrs`/`\brdrth`/`\brdrdb`/`\brdrdot`/`\brdrdash`/`\brdrw`/`\brdrcf`/
  `\brsp`). The RTF grammar is `<edge> <style> \brdrw? \brsp? \brdrcf?`; the four
  standard edges map to `Borders`, but `\brdrbar`/`\brdrbtw` need `w:pBdr`
  `between`/`bar`, which the shared `Borders` model does not carry yet.
- Cell shading (`\chshdng`/`\clshdrawnil`) and diagonal cell borders
  (`\cldgll`/`\cldglu`) are unreviewed.
- Table indent `\tblind`/`\tblindtype`: represented by the row position
  (`\trleft`); WPS writes no `w:tblInd` for the corpus.
