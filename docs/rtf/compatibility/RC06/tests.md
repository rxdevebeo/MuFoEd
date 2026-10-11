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

## Borders (2026-10-11)

RTF borders are the grammar `<edge> <style> \brdrw? \brsp? \brdrcf?`. A parser
state machine remembers the running edge; the style/width/colour/space tokens
fill it. Edge families map to three collections:

| RTF edges | WML |
| --- | --- |
| `\brdrt`/`\brdrb`/`\brdrl`/`\brdrr` | `w:pBdr` top/bottom/left/right |
| `\brdrbar`/`\brdrbtw` | `w:pBdr` `bar`/`between` |
| `\clbrdr{t,b,l,r}` | `w:tcBorders` |
| `\trbrdr{t,b,l,r,h,v}` | `w:tblPrEx/w:tblBorders` (row exception) |

Style tokens map `\brdrs`->single, `\brdrth`->thick, `\brdrdb`->double,
`\brdrdot`->dotted, `\brdrdash`->dashed, `\brdrnil`->nil, `\brdrnone`->none,
`\brdrinset`/`\brdroutset`, `\brdrtriple`, `\brdrdashsm`/`\brdrdashd`/
`\brdrdashdd`/`\brdrdashdotstr` and the `\brdrtnth*`/`\brdrwavy` families.
`\brdrwN` (twips) becomes `w:sz` in eighths of a point (clamped to
`ST_EighthsPoint` 0..=96); `\brdrcfN` resolves through the colour table;
`\brspN` becomes `w:space` in points.

The shared `Borders` model gained `between`/`bar` (only `CT_PBdr` carries them)
and `tl2br`/`tr2bl` (only `CT_TcBorders`): the writer emits each only in its own
container, the reader parses them, and the render-svg style merge carries them.
The change touches wml/write/convert/render-svg; their full test suites were
re-run (font-family substitution test excepted: environment-only, pre-existing).

Tests: `cell_borders_map_to_tc_borders`, `paragraph_borders_map_to_pbdr`,
`row_exception_borders_map_to_tbl_prex`, `brdrtbl_makes_the_cell_edge_inherit`.

## Cell properties and shading (2026-10-11)

- `\clhidemark` -> `w:hideMark`; `\clpad{t,b,l,r}` -> `w:tcMar`; the `\clpadf*`
  unit selectors are Info; `\clftsWidth`/`\clwWidth` -> `w:tcW`
  (auto/nil/pct/dxa); `\trgaph` -> `w:tblCellMar` left/right.
- `\clcbpat`/`\clcfpat`/`\clshdng`/`\clshdrawnil` -> cell `w:shd`;
  `\trcbpat`/`\trcfpat`/`\trshdng` shade every cell of the row;
  `\chcbpat`/`\chcfpat`/`\chshdng` -> `w:rPr/w:shd`; `\cbpat`/`\cfpat`/`\shading`
  -> paragraph `w:shd`. The intensity maps 0 -> `clear`, 10000 -> `solid`, else
  the nearest `pctNN`.
- `\cldgll`/`\cldglu` -> `w:tcBorders` `tl2br`/`tr2bl`.
- `\tblind`/`\tblindtype`/`\alntblind` are Info (the indent is the row position,
  and WPS writes no `w:tblInd`).

Tests: `cell_shading_margins_width_and_diagonals`, `table_gap_and_row_shading`,
`character_shading_maps_to_run_shading` (through Strict write + reopen).

## Page and character borders (2026-10-11)

- `\pgbrdr{t,b,l,r}` -> the section `w:pgBorders` edges (style/width/colour/space);
  `\pgbrdrhead`/`\pgbrdrfoot`/`\pgbrdropt` are Info; `\brdrart` (art border) has no
  Strict `ST_Border` form, so it drops the edge and stays an explicit Loss.
- `\chbrdr` + its style/width -> the single run border `w:rPr/w:bdr`. The reader
  now retains `w:bdr` (it used to discard it as Partial).

Tests: `page_borders_map_to_pg_borders`, `character_border_maps_to_run_border`.

## Corpus measurement

Against the fresh RC05 WPS references:

- cell borders: `127197 -> 123549` (`-3648`; 008/015/053/073/086/087).
- paragraph + row borders: `123549 -> 87193` (`-36356`;
  003/015/017/019/050/053/055/073/098).
- cell props + shading: `87193 -> 83721` (`-3472`; 008/053/073/086/087/098/015...).
- compat: `83721 -> 83684` (`-37`); char borders `83684 -> 83675` (`-9`, 017);
  page borders `83675 -> 83671` (`-4`, 015). RC06 total `-43526`.
- text 99/100 (only the 014 WPS PAGEREF quirk), resources 100/100,
  `check` 100/100 `ok: strict`.
- pagination 65/100 and pixels 6/100 docs / 22/458 pages — identical to RC05:
  no reflow and no visual regression.
- the only border Losses left in the whole corpus are the four page-border art
  styles (`\brdrart`, 015).

## Already mapped (unchanged here)

Row/cell grid (`\trowd`/`\cellx`/`\trleft`), horizontal merge
(`\clmgf`/`\clmrg`), vertical merge (`\clvmgf`/`\clvmrg`), vertical align
(`\clvertalt`/`\clvertalc`/`\clvertalb`), header rows (`\trhdr`) and row
`cantSplit` (`\trkeep`) are mapped. The shared table geometry and units come
from P6.

## Closed with explicit status

- Nested tables (`\itap > 1`): no corpus witness; kept as a bounded, named
  Reject (`nested-table-structure`), never flattened
  (`nested_tables_are_an_explicit_bounded_reject`).
- Renderer: draws `w:tcBorders`/`w:tblPrEx`/`w:pgBorders`, but does not stroke
  `w:pBdr` (only its top/bottom pads affect spacing), paint run borders
  (`w:rPr/w:bdr`) or paint diagonals. Documented renderer gap, not an import gap.
