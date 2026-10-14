# RC09 remaining loss classes

Card: [STATUS.json](STATUS.json). Plan:
[REMAINING_COMPATIBILITY_PLAN.md](../../REMAINING_COMPATIBILITY_PLAN.md).

## Revision ids and legacy layout controls (2026-10-11)

The import ledger was dominated by two families that assert the Strict default
(or carry no visual information at all), so a mapping would write nothing back
and the honest classification is a named Info, not a Loss:

- **Revision save identifiers** `\insrsid`/`\charrsid`/`\pararsid`: non-visual
  merge metadata. The model carries optional `w:rsid*` slots the renderer never
  reads, and the RTF stream's decimal ids and their per-run/per-paragraph
  placement are not reconstructed. `rsid-compat:<name>`.
- **Word 6/7 auto-spacing / font alignment** `\faauto`/`\adjustright`/
  `\aspalpha`/`\aspnum`: their bare form is the WML default
  (`w:textAlignment="auto"`, `w:adjustRightInd` on, `w:autoSpaceDE`/
  `w:autoSpaceDN` on). `word6-compat:<name>`.
- **Word 5/6/7 and Asian text-layout hints** `\hyphpar`/`\wrapdefault`/
  `\sectdefaultcl`/`\linex`/`\lnbrkrule`/`\partightenfactor`: they assert default
  layout behaviour. `paragraph-compat:<name>`.

The **non-default** form of a toggle whose property has no paragraph slot stays
an explicit Loss rather than being renamed away: `\hyphpar0` (suppress
auto-hyphenation) and `\adjustright0` would need `w:suppressAutoHyphens`/
`w:adjustRightInd`, which the paragraph model does not carry.
`structure.rs::legacy_layout_and_revision_controls_are_info_not_losses` pins the
Info side and `::non_default_legacy_layout_controls_stay_losses` the negative
control.

Corpus receipt `target/rtf-import/rc09-triage/`: import losses **83010 -> 11394**
(-71616). The change is severity-only, so the imported DOCX bytes are SHA-256
identical (checked on 014/074/092) and pagination/pixels are unchanged.

## Slot mappings and metadata classification (2026-10-11)

The remaining classes with a real WML slot were mapped rather than dropped, and
the reference oracle (WPS Writer 12.1.0.28485, `RC05-wps-full/oracle/files/`)
was used to decide the ones without a slot:

- **Character** `\kerning` -> `w:kern` (WPS writes `w:kern`), `\ulc` ->
  `w:u/@w:color` (WPS writes `w:u w:val="single" w:color="0000FF"`), `\cb` ->
  `w:rPr/w:shd` (the character background; WPS is inconsistent about it, but it
  is the same property as the already-mapped `\chcbpat`), and `\highlightN` ->
  `w:highlight` (086: colortbl index 2 = yellow, 3 = white, matching WPS).
- **Table** `\trpadd{t,b,l,r}` + `\trpaddf{t,b,l,r}` -> `w:tblCellMar`,
  `\trrh` -> `w:trHeight` (negative = `exact`), `\trqr`/`\trql`/`\trqc` ->
  `w:trPr/w:jc`, `\trautofit` -> `w:tblLayout` (all confirmed against 008).
- **Paragraph/settings** `\pardeftab`/`\deftab` -> `w:defaultTabStop`
  (confirmed against 028/032), and `{\*\panose ...}` -> `w:panose1` on the font
  entry (WPS writes `w:panose1`).
- **Classified Info (reference writes nothing back):** `\hyphpar0`/`\noline`,
  `\nooverflow`/`\nocwrap`, `\cltxlrtb` (the default `tb` direction),
  `\hres`/`\chhres`, `\qk`/`\qnatural`, `\animtext`, `\trftsWidth*`/`\trwWidth*`.
- **Metadata destinations as Info:** `{\*\falt}`, `{\*\hyphen}`, `{\*\pnseclvl}`,
  `{\*\rsidtbl}`, `{\*\revtbl}`, `{\*\latentstyles}`, `{\*\expandedcolortbl}`,
  `{\*\xmlnstbl}`, `{\*\pgptbl}`, `{\*\pgdsctbl}`, `{\*\wgrffmtfilter}`,
  `{\*\userprops}`, `{\*\fname}`, `{\*\generator}`.
- **Settings/metadata control families as Info:** the table/section/style revision
  ids, view settings, Cocoa flags, drawing-grid settings, math settings, Word
  application settings, `\widowctrl`, `\additive`, `\pardirnatural` and the
  default-language hints.

Tests: `structure.rs::{kerning_underline_colour_and_character_background_map_to_run_props,
highlight_colour_index_maps_to_the_named_highlight,
table_padding_row_height_alignment_and_layout_map,
default_tab_stop_and_compat_controls_map_and_classify,
font_panose_maps_and_metadata_destinations_are_info,
document_settings_and_compat_controls_are_info}`. Corpus receipt
`target/rtf-import/rc09-slice8/`: import losses **11394 -> 4441**; pagination
66/100 with **0 page-count changes** vs `RC08-columns-layout`; `check` 100/100
`ok: strict`. The metadata/settings slices are severity-only (DOCX SHA-256
identical); the highlight slice changes only 086 and not its page count.

## Picture crop, table styles, metadata and the WPS line ending (2026-10-12)

Continuation of the 2026-10-11 triage. Every slice is either a real slot mapping
or a severity-only classification; the imported DOCX is SHA-256 identical to the
previous slice for each Info slice.

- **Picture crop** `\piccrop{t,b,l,r}` -> `a:srcRect`. The twips inset is stored
  as thousandths of a percent of the displayed extent (`90` twips of a `720`
  twip edge -> `12500`). An all-zero crop writes no `a:srcRect`. Witness 075 has
  the only non-zero crops in the corpus (`\piccropr4308`/`\piccropr4490`,
  `\piccropb-80`), so only its DOCX changes; WPS drops the crop, so this is a
  faithful import where WPS is quirkier. Tests
  `structure.rs::{picture_crop_maps_to_the_source_rectangle_and_shape_hints_are_info,
  absent_picture_crop_writes_no_source_rectangle}`.
- **`{\*\picprop ...}`** Word 97 shape hints for an image: the WPS reference
  writes no picture properties back and the Strict picture has no non-visual
  slot, so `destination:picprop` is a named Info.
- **Table styles** `{\*\tsN ...}` + `\tsN`/`\tsdN`: every independent WPS
  reference writes a single built-in "Normal Table" style and reproduces the
  effective table properties directly from the rows the producer repeats
  alongside the style, so the definition and the reference are named Info
  (`destination:ts`, `table-style-compat:ts`/`:tsd`). Test
  `structure.rs::table_style_definition_and_reference_are_info`.
- **Legacy numbering and non-visual metadata as Info:** `{\*\pn ...}` (superseded
  by the list table; WPS writes no `w:numPr` for 067/024), `\defchp`/`\defpap`
  (Word 6 defaults superseded by `w:docDefaults`), `\template`, `\datastore`,
  `\blipuid`, `\updnprop`, `\mmathPr`, and `\themedata`/`\colorschememapping`
  (the body never references theme colours because RTF resolves them to the
  explicit colour table). Test
  `structure.rs::legacy_numbering_and_non_visual_metadata_destinations_are_info`.
- **WPS leading-group line ending** `wps:leading-group-line-ending` is now Info,
  not Loss. The WPS profile preserves a leading raw group line ending as a line
  feed exactly as WPS does; the independent reference writes the same `&#10;`
  and the decoded body text matches the oracle for every affected document
  (005/058/059/075). This corrects the 2026-10-11 card. Test
  `import.rs::wps_leading_group_line_endings_are_info_not_losses`.

Corpus receipt `target/rtf-import/rc09-slice12/`: import losses **4441 -> 1580**
(-2861). Pagination/pixels unchanged: the crop slice changes only 075 (no page
count change) and every other slice is severity-only with SHA-256-identical
DOCX.

## Floating shapes as DrawingML (2026-10-12)

`{\shp ...}` is no longer dropped. A floating Word 97 shape becomes a
`wp:anchor`:

- `\shpinst` geometry (`\shpleft/top/right/bottom`, `\shpbx*`/`\shpby*` base,
  `\shpwr` wrap, `\shpz` z-order, `\shpfblwtxt`) maps to
  `wp:positionH/V`, `wp:wrap*`, `relativeHeight` and `behindDoc`.
- `{\sp{\sn X}{\sv Y}}` properties select the preset geometry
  (`shapeType` 1/2/3/5/20/86/202 → rect/roundRect/ellipse/line/callout), the
  solid fill (`fillType`/`fillColor`), the outline (`fLine`/`lineWidth`/
  `lineColor`) and the non-visual `wzName`/`wzDescription`.
- `\shptxt` is parsed into a `wps:txbx/w:txbxContent` (fields and all).
- A nested `{\pict}` (from `pib`/`fillBlip`/`\shppict`) becomes the anchored
  `pic:pic` and keeps its media part.

Witnesses 075 (56 shapes, pictures + text boxes + lines), 092 (text boxes with
PAGE fields, freeform), 003 (background rect with a picture fill), 017/078
(text boxes). Shape layout hints with no drawing slot (`\posx`/`\posy`,
`\pvpara`, `\wraparound`, `\dfrmtxt*`, `\dxfrtext`, `\absnoovrlp`, `\anchorText`,
`\shprslt`) and the blip ids (`\bliptag`/`\blipupi`) are named Info. `\shpgrp`
grouped shapes remain an explicit `destination:shpgrp` Loss.

Tests `structure.rs::{floating_text_box_shape_maps_to_an_anchored_wps_shape,
floating_picture_shape_maps_to_an_anchored_picture}`. Corpus receipt
`target/rtf-import/rc09-shapes/`: import losses **1580 -> 1489** (-91); every
shape document still passes `check` → `ok: strict`.

## Remaining

The classes still Loss (see [STATUS.json](STATUS.json) `remaining`) are OLE
(`\object`/`\objdata`), comments, revisions, textboxes outside shapes and
grouped shapes (`\shpgrp`), the pinned unknown-LCID languages, the deliberately
un-materialized implicit `\s0`, and a deliberately unclassified tail of
lower-confidence application and layout flags.
