# Fuzz protocol

Rework R10: fixed conditions and a record template for the 24-hour acceptance
runs of the Stage-1/Stage-2 fuzz targets.

## Targets

| Target | Entry point | Invariant |
|---|---|---|
| `fuzz_zip` | `Package::open_reader` over raw bytes | no panic / no UB / no unbounded allocation |
| `fuzz_xml` | `XmlReader::new` + `next_event` to EOF | no panic / no UB / no unbounded allocation |
| `fuzz_relpath` | `canonicalize_part_name` / `resolve_target` | no panic / no UB |
| `fuzz_wml` | `Package::open_reader` + `parse_document` | no panic / no UB |

## Conditions (acceptance run)

- Toolchain: `nightly` via `cargo-fuzz` (`cargo +nightly fuzz`).
- Engine: `libFuzzer` (default), ASan enabled (default for `cargo-fuzz`).
- References: `TZ-STRICT-OOXML-RUST.md` §12.3 (24 h without panics/UB/leaks),
  `STAGE-1-TASK.md` §9.4.
- Per target, run a continuous session of **24 h**:

  ```text
  cargo +nightly fuzz run fuzz_zip     -- -max_total_time=86400
  cargo +nightly fuzz run fuzz_xml     -- -max_total_time=86400
  cargo +nightly fuzz run fuzz_relpath -- -max_total_time=86400
  cargo +nightly fuzz run fuzz_wml     -- -max_total_time=86400
  ```

- A shared seed corpus may be used (packages produced by
  `strict-ooxml-core/tests/common`); the corpus and `artifacts/` are
  `.gitignore`d.
- Acceptance: zero crashes / timeouts / OOM in all four sessions.

## CI

- Pull requests: a 60-second smoke run of each target (`fuzz-smoke` job).
- Schedule: a nightly job runs each target for a bounded window
  (`fuzz-nightly`). The full 24-hour sessions are run manually (or on a longer
  scheduled window) and recorded below.

## Status (evidence)

**24-hour acceptance sessions: WAIVED for the Stage-2 delivery (2026-09-28).**
No 24-hour session has been recorded, so criterion §12.3 of the TZ is not
formally satisfied. Per `STAGE-2-REWORK.md` M8 the maintainers record an explicit
waiver instead: the acceptance environment is Windows x86_64 (where `cargo-fuzz`
cannot link, see below) and no Linux/macOS nightly host is available to the
Stage-2 executor. CI continues to provide the bounded evidence below, and the
24-hour runs remain an open release-gate item (Stage 7).

> Waiver scope: covers `fuzz_zip`, `fuzz_xml`, `fuzz_relpath`, `fuzz_wml`.
> Owner: maintainers. Re-evaluate before Stage-7 release.

- CI provides continuous evidence: 4 × 60 s per PR (`fuzz-smoke`) and 4 × 1 h
  nightly (`fuzz-nightly`). These are bounded runs, not the 24-hour acceptance
  evidence.
- Local execution on **Windows x86_64 (MSVC)** is not possible: `cargo-fuzz`
  cannot link the fuzzing runtime there. Both the default ASan build
  (`clang_rt.asan_dynamic_runtime_thunk-x86_64.lib` missing) and
  `--sanitizer none` (unresolved `__start___sancov_pcs` /
  `__stop___sancov_cntrs`) fail at link time. Sessions must run on a Linux or
  macOS nightly host — as the CI jobs do.
- Until a 24-hour run is attached, acceptance rests on the smoke/nightly CI runs
  plus the property tests: `strict-ooxml-core` `package_open_never_panics` and
  `xml_reader_never_panics`, and `strict-ooxml-wml` `random_bytes_never_panic`,
  `random_bodies_never_panic` and `parsing_is_deterministic` (256 cases each per
  `cargo test`).

To produce the evidence, run the four commands from "Conditions" on a Linux
nightly host and paste the resulting block into the session record below.

## Session record

Copy this block when a session is executed and store it in the release notes
(`docs/fuzz-sessions/` may be used for reports that should be versioned).

```text
Date:            YYYY-MM-DD
Host:            <os/arch/cpu>
Rust + cargo-fuzz: <versions>
Commit:          <git sha>

fuzz_zip:     duration=24h  execs=<n>  cov=<n>  crashes=0  timeouts=0  oom=0
fuzz_xml:     duration=24h  execs=<n>  cov=<n>  crashes=0  timeouts=0  oom=0
fuzz_relpath: duration=24h  execs=<n>  cov=<n>  crashes=0  timeouts=0  oom=0
fuzz_wml:     duration=24h  execs=<n>  cov=<n>  crashes=0  timeouts=0  oom=0

Result: PASS / FAIL
Notes: <artifacts, if any>
```
