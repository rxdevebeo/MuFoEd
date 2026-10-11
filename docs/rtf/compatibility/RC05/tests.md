# RC05 lists and numbering

Card: [STATUS.json](STATUS.json). Plan:
[REMAINING_COMPATIBILITY_PLAN.md](../../REMAINING_COMPATIBILITY_PLAN.md).

## Legacy list constructs are Info, not Loss (2026-10-10)

Several list-table constructs were reported as `control:*`/`destination:*`
Loss even though they are either redundant or compatibility noise:

- `{\*\listtext ...}` / `{\listtext ...}` / `{\pntext ...}` — the cached list
  text for a paragraph. The list table already regenerates the marker, so
  keeping this text would double it; it is dropped as Info.
- `{\*\levelmarker ...}` — a legacy alternative level marker superseded by
  `\leveltext`; dropped as Info.
- `\jclisttab` — flags the following `\tx` tab stop as a list tab. The tab stop
  itself is captured from `\tx`; the flag has no WML equivalent.
- `\levelspace` / `\levelindent` — Word 7 list-level spacing, superseded by the
  level's paragraph indents.
- `\leveltemplateid` — a cosmetic list-template id.
- `\levelold` — marks a level converted from Word 6.0/7.0 (2026-10-11).
- `\fbias` — the East-Asian font-bias hint Word emits only inside a
  `\listlevel`; no direct `w:rFonts` analogue (2026-10-11).

`structure.rs::legacy_list_constructs_are_info_not_loss` pins that a list with
all of them imports with zero Loss and that `destination:listtext` /
`destination:levelmarker` appear as Info. Corpus import losses fall by 8798.

## List override levels (2026-10-11)

`{\lfolevel ...}` (with or without the `\*` prefix) is the per-level override
inside `\listoverride`. It is now anchored on the `w:num` as a `w:lvlOverride`:

- `{\lfolevel\listoverrideformat{\listlevel ...}}` — a full replacement level.
  The nested `\listlevel` controls (`\levelnfc`/`\leveltext`/`\levelstartat`/…)
  are routed through the same helper as the `\listtable` levels
  (`Parser::level_control`); the finished level becomes `LevelOverride.level`.
- `{\lfolevel\listoverridestart\levelstartatN}` — a start-only override: the
  `\levelstartat` that has no enclosing `\listlevel` becomes
  `LevelOverride.start_override`, and no `w:lvl` is emitted.

The level index is the `\lfolevel` ordinal within the `\listoverride` (0-based,
bounded to nine); `\listoverridecount` is accepted for any value because it is
redundant with the groups actually present. The model and renderer already
carried and consumed `w:lvlOverride`, so this slice is parser-local.

`structure.rs::list_override_level_replaces_the_level` and
`...::list_override_start_sets_the_start_override` pin both forms, assert zero
import Loss with no `list-override-control:*` feature, and prove the override
survives import → Strict write → reopen (the abstract level keeps its `*`,
the instance carries the replacement / start).

Corpus 021 (the only document with `\lfolevel`) now carries the override: its
marker becomes the Symbol bullet glyph instead of the abstract `*`. Import
losses fall 152 → 131 (`-18` `list-override-control:*`, `-3`
`list:unbound-text`); the same `\fbias`/`\levelold` reclassifications also drop
015 by 1 and 087 by 9. 021's rendered page count is unchanged (1 page).

## Already mapped (unchanged here)

The list table maps `\list`/`\listid`/`\listlevel`/`\levelnfc`/`\levelnfcn`/
`\levelstartat`/`\leveljc`/`\leveljcn`/`\levelfollow`/`\leveltext` to
`w:abstractNum`/`w:lvl` and `\listoverride`/`\listid`/`\ls` to `w:num`. A
negative `\listid` is preserved as an unsigned source identity
(`negative_list_ids_and_symbol_style_names_never_become_body_text`).

## Full WPS gate (2026-10-11)

The whole corpus was re-run against a fresh, independent WPS oracle (WPS Writer
12.1.0.28485 via `wps_oracle.py --pdf`, 100/100 Captured; facts 100/100;
`strict-ooxml check` 100/100 `ok: strict`). Receipts under
`target/rtf-import/RC05-wps-full/`.

- Import losses `127228 -> 127197` (`-31`): 021 `-21`
  (`-18 list-override-control:*`, `-3 list:unbound-text`), 015 `-1` and 087
  `-9` (`\fbias` list-compat Info). Resources 100/100 Pass; text 99/100.
- The one text Fail is 014: a `\flddirty` PAGEREF keeps the RTF's stale cached
  page number (`1,4,8,...`) while the WPS reference recomputes it
  (`3,6,10,...`). Our importer is faithful to the source field cache; this is a
  WPS field-recompute quirk, not an RC05 change.
- Pagination `65/100` Pass, pixels `6/100` docs / `22/458` pages — identical to
  the RC08-final receipt, so RC05 introduces no visual regression. 021 (the only
  `\lfolevel` document) stays a pixel Fail at ssim 0.9067 from the pre-existing
  ~20 px line-pitch gap vs WPS; only its 46-px marker region changed
  (`*` -> Symbol bullet).

## Open

- Counter continuity across paragraphs/tables/sections is a renderer concern
  and was not re-audited here.
- `list:unbound-text` (text directly inside `listtable`/`listoverridetable`) is
  malformed source and stays a Loss.
- The 014 `\flddirty` PAGEREF reference quirk needs an explicit accepted status
  in the RC11 reviewed profile (importer keeps the cached result; WPS
  recomputes on export).
