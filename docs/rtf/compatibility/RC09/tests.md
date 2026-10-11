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

## Remaining

The rest of the ledger is now small. The classes still Loss (see
[STATUS.json](STATUS.json) `remaining`) are the table-cell / character controls
with real WML slots (`\trpadd*` -> `w:tblCellMar`, `\ulc` -> `w:u/@w:color`,
`\cltxlrtb` -> `w:textDirection`, ...) plus the content classes that need a
shared model (`comments`, `revisions`, `textboxes`, `OLE`, document metadata) and
the metadata destinations (`{\*\rsidtbl}`, `{\*\expandedcolortbl}`,
`{\*\xmlnstbl}`, `{\*\pgptbl}`, `{\*\pgdsctbl}`, `{\*\revtbl}`,
`{\*\panose}`, `{\*\pnseclvl}`, `{\*\hyphen}`).
