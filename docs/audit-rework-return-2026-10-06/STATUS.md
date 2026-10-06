# Return 2026-10-06 — production receipts (in progress)

HEAD at last production increment: `5fffaae` plus this D05 inventory/writer increment.
Toolchain: `cargo +1.92.0 --locked`. WORD-COMPAT: NOT_RUN. CI exact-SHA: NOT_RUN (no push).

Census 221 docs (rebuilt CLI): `unmatched_schema=0`, `ours=0`, `unclassified_element_changes=11711` FAIL. Chart `lblOffset`/`gapWidth`/`overlap` are source messages, not ours.

## D02 witnesses

| Check | Result |
|---|---|
| `hostile::a_heavy_footer` debug `--features write,svg,report` | PASS 1.81 s (1 MiB stack inside the test; 10 s budget) |
| same, `--release` | PASS 0.45 s |
| CC0 017 first page `--transitional` | SVG 1 793 689 B; wall ~3.6 s after warm CLI; CLI exit 1 (lossy/unsupported, file written) |
| CC0 035 first page `--transitional` | SVG 986 877 B; 0.91 s; CLI exit 1 (same) |
| CC0 100 first pages | 100/100 SVG written; 0 slow (>10 s); two tiny pages (014 709 B, 086 238 B blank canvas) |

## D05 this increment

- `w:tblLook` bits written as `1`/`0`; Transitional `@val` decoded into the same bits.
- `w:pgSz/@w:code` round-trips.
- `mc:Ignorable` on roots recorded as `w:*@Ignorable` / `a:*@Ignorable`.
- `w:sectPr@rsid*` recorded as Partial (named loss).
- Census treats `1`/`true`/`on` as one on/off spelling; `100` vs `100%` on `@percent`.
- Registry TZ-44/46 expanded; TZ-47 `w:tblLook@val` declared_transform.
- Full census re-run on the rebuilt release CLI: `unmatched_schema=0`, `ours=0`, `unclassified_element_changes=11711` FAIL.

## D01

`clippy --workspace --all-targets --all-features --locked -- -D warnings` PASS after rustfmt of `parse/props.rs`.

## D06 / D07 / D08 / D09

- D06: F16 frame origin/xAlign still in tree; WORD-COMPAT NOT_RUN; Clio p.54/56/104 vs WPS PDF ledger not remeasured (no Word; wps-reference folder empty in this tree).
- D07: 13 matrix rows measured; F16-complex-word-schemes blocked (no Word).
- D08: WML `--branch` llvm-cov **65.07% (678/1042)** after `--all-features` (below 70%). Fuzz: 8 WSL targets launched in parallel for `-max_total_time=3600`; `fuzz_xml`/`fuzz_zip` completed 3603 s; `fuzz_pdf`/`fuzz_convert` exited 1 (ASan leak artifacts). Parallel jobs shared `fuzz-0.log`, so this is not eight independent isolated hours. CI NOT_RUN.
- D09: sequential acceptance waits for census + cov + fuzz.

## Commands

- `cargo +1.92.0 test -p strict-ooxml --features write,svg,report --test hostile a_heavy_footer` exit 0 (debug and release)
- `target/release/strict-ooxml.exe render --transitional --pages 1` over `testdata/CC0_DOCX` (100 files)
- `cargo +1.92.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` exit 0
- `cargo +1.92.0 fmt --all` applied
