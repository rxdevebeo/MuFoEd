# Return 2026-10-06 — production receipts (in progress)

Base HEAD before this commit: `8885fd340803cde76062cd06ab5e775dc075f846`.
Toolchain: `cargo +1.92.0 --locked`. WORD-COMPAT: NOT_RUN. CI exact-SHA: NOT_RUN (no push).

## Production this increment

| Card | Kind | Result |
|---|---|---|
| D05 | production | Writer no longer invents `a:off`/`a:ext` `0,0`. Hyperlink ids collected from drawings/text boxes/groups; header/notes parts bind hyperlinks through part rels. Census CORPORA includes `testdata/CC0_DOCX`. Full census/XSD/OPC 27+121+100 not yet re-run on this SHA. |
| D06 | production | Page-framed tables omit `tblCellMar` start/end (WPS SNP ~10 twips). `w:xAlign`/`w:yAlign` place frames in the page/margin box. F16 synthetic + align tests PASS. WORD-COMPAT NOT_RUN. Clade rhythm / thesis captions vs WPS PDF still need a measured ledger on Clio p.54/56/104. |
| D07 | tests | 13 previously blocked matrix rows now have named tests and `status = measured`. Remaining blocked: `F16-complex-word-schemes` (no Word reference). `f21_public` PASS. |
| D01 | gate | `fmt --check` and `clippy -D warnings` PASS after `layout_row` allow. |
| D02/D04/D08/D09 | | Follow-up runs: hostile footer, CC0 017/035/100, XSD/OPC three corpora, WML branch ≥70%, fuzz 8×3600 Linux, sequential acceptance. |

## Commands (this increment)

- `cargo +1.92.0 test -p strict-ooxml-write --lib` exit 0
- `cargo +1.92.0 test -p strict-ooxml-wml --lib` exit 0
- `cargo +1.92.0 test -p strict-ooxml-render-svg --test f16_frames` exit 0 (8 tests)
- F04/F07–F19 new matrix tests exit 0
- `cargo +1.92.0 test -p strict-ooxml-cli --test cli f03_stage_combination_exits` exit 0
- `cargo +1.92.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` exit 0
