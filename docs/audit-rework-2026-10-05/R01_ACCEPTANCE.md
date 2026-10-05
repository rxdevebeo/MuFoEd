# R01 ACCEPTANCE — U+FEFF text corruption

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` |
| Dirty-tree fingerprint | `3407be7296e103863cc872eb21e84755610cd87044e2d3cc4c0e559aad69ba13` |
| `strict-ooxml-core/src/xml/mod.rs` SHA-256 | `6a0a72fc488d301fae69d3ec464e3987c434594216cf403a51ba7811eafb7f1a` |
| Fixture `leading-feff.docx` SHA-256 | `068a3d5a0becc3f3ecc0ec50b339946d0a7cefb002bc6689f3321a503856e684` |
| CC0 manifest SHA-256 | `68b0aead98882ac1d85bdaf24eda015a6eae06b01fd1f582b2f1321e86ed5745` |
| Toolchain | `cargo +1.92.0` |
| Result | **PASS** |

## Root cause

`XmlReader::read_parsed` recreated `quick_xml::Reader` on `&data[pos..]`. When a text node began with UTF-8 `EF BB BF` (U+FEFF), quick-xml treated those bytes as a **stream BOM**, stripped them without advancing `buffer_position`, dropped the character from `Event::Text`, and caused the next read to restart three bytes early — duplicating the last Tamil sign. Document-level BOM removal in `normalize_encoding` is a separate path and remains correct.

## Fix

Persistent `Reader<Cursor<Vec<u8>>>` for the whole normalized part. BOM detection runs once at the start of an already BOM-stripped buffer; mid-document U+FEFF stays character data. U+FEFF is never stripped from text content.

## Evidence

### RED

```
cargo +1.92.0 test -p strict-ooxml-core --locked --lib text_node_ -- --nocapture
```

Log: `target/audit-rework-2026-10-05/R01/red/unit.log` — leading FEFF Tamil case failed with `[2980…3021,3021]` vs `[65279,2980…3021]`.

### GREEN

```
cargo +1.92.0 test -p strict-ooxml-core --locked --lib feff -- --nocapture
cargo +1.92.0 test -p strict-ooxml-core --locked --lib xml::
cargo +1.92.0 run -p strict-ooxml-cli --locked -- write docs/audit-review-2026-10-05/leading-feff.docx --out target/audit-rework-2026-10-05/R01/green/leading-feff-out.docx
python docs/audit-review-2026-10-05/cc0_probe.py
```

| Check | Result |
|---|---|
| Unit FEFF matrix (6 tests) | 6/6 pass |
| Full `xml::` module | 40/40 pass (after inverse added: 41) |
| `leading-feff.docx` write | exit 0; code points identical (`same=true`) |
| CC0 probe 100/100 | fixed_point pass=100 fail=0 |
| `025_iyothee_…` | fixed_point=true, write exit=0 |
| `080_makkal_…` | fixed_point=true, write exit=0 |

Logs: `target/audit-rework-2026-10-05/R01/green/`.

### Inverse

`inverse_recreated_slice_reader_corrupts_leading_feff` recreates the old per-event slice `Reader` and still produces `[BA4,BAE,BBF,BB4,BCD,BCD]` while production yields the intact FEFF sequence. Log: `target/audit-rework-2026-10-05/R01/inverse/unit.log`.

## Limits

- Commit/push not performed (owner permission required).
- R01 does not claim R02 Strict XSD closure for the CC0 corpus.
