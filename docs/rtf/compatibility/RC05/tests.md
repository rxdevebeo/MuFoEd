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

`structure.rs::legacy_list_constructs_are_info_not_loss` pins that a list with
all of them imports with zero Loss and that `destination:listtext` /
`destination:levelmarker` appear as Info. Corpus import losses fall by 8798.

## Already mapped (unchanged here)

The list table maps `\list`/`\listid`/`\listlevel`/`\levelnfc`/`\levelnfcn`/
`\levelstartat`/`\leveljc`/`\leveljcn`/`\levelfollow`/`\leveltext` to
`w:abstractNum`/`w:lvl` and `\listoverride`/`\listid`/`\ls` to `w:num`. A
negative `\listid` is preserved as an unsigned source identity
(`negative_list_ids_and_symbol_style_names_never_become_body_text`).

## Open

- **List override levels** (`{\*\lfolevel ...}` with `\listoverrideformat{...}`
  or `\listoverridestart\levelstartatN`) are still `list-override-control:*`
  Loss. The model and renderer already support `w:lvlOverride`
  (`LevelOverride { start_override, level }`), so this is a bounded parser
  addition: route the nested `\listlevel` controls into a `LevelOverride` on the
  `w:num`. One corpus witness (021) whose override changes the bullet glyph.
- `list:unbound-text` (text directly inside `listtable`/`listoverridetable`) is
  malformed source and stays a Loss.
