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

## Remaining

The classes still Loss (see [STATUS.json](STATUS.json) `remaining`) are the WPS
leading-group line ending (`wps:leading-group-line-ending`, where our preserved
byte differs from WPS's `&#10;` spelling), the table styles (`\ts`), picture
crop/`\picprop`, the content classes that need a shared model (`comments`,
`revisions`, `textboxes`, `OLE`, document metadata), the pinned unknown-LCID
languages, and a deliberately unclassified tail of lower-confidence application
and layout flags.
