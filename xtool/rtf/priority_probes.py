#!/usr/bin/env python3
"""Independent WPS probes for bounded, authored RTF compatibility examples."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from census import write_json
from docx_facts import inspect
from wps_oracle import stop_owned_worker

CASES = {
    "section_after_par": br"{\rtf1 A\par\sect B\par}",
    "section_mid_par": br"{\rtf1 A\sect B}",
    "section_continuous": br"{\rtf1\sbknone A\par\sect B}",
    "raw_newline": b"{\\rtf1 A\nB}",
    "raw_tab_newline": b"{\\rtf1 A\t\nB}",
    "escaped_line": b"{\\rtf1 A\\\nB}",
    "soft_hyphen": br"{\rtf1 A\-B}",
    "unicode_soft_hyphen": br"{\rtf1\uc0 A\u173 B}",
    "unsigned_surrogates": br"{\rtf1\uc0 A\u55357 \u56457 B}",
    "signed_surrogates": br"{\rtf1\uc0 A\u-10179 \u-9079 B}",
    "cp0_unicode": br"{\rtf1\ansi\ansicpg0\uc1 ASCII \u1046?}",
    "cp0_nonascii": br"{\rtf1\ansi\ansicpg0 \'cf}",
    "escaped_nul": br"{\rtf1 A\'00B}",
    "wingdings": br"{\rtf1\deff0{\fonttbl{\f0\fcharset0 Arial;}{\f1\fcharset2 Wingdings;}}\f0 A\f1\'fc\f0 B}",
    "cocoa_property": br"{\rtf1 A{\*\cocoaligature0 CocoaLigature1 }B}",
    "unknown_ignorable": br"{\rtf1 A{\*\unrecognized metadata}B}",
    "line_breaks": br"{\rtf1 A\line\line B}",
}
FOLLOWUP_CASES = {
    "group_tab_lf": b"{\\rtf1 A{\t\nB}}",
    "group_lf_spaces": b"{\\rtf1 A{\n    }B}",
    "group_lf_text": b"{\\rtf1 A{\nB}}",
    "group_crlf_text": b"{\\rtf1 A{\r\nB}}",
    "raw_lf_after_text": b"{\\rtf1 A{B\nC}}",
    "control_lf": b"{\\rtf1 A{\\b\nB}}",
    "nul_suffix": br"{\rtf1 A\par\'00\'00}",
    "cocoa_inline": br"{\rtf1 A\CocoaLigature1 B}",
    "cocoa_initial": br"{\rtf1\CocoaLigature1 A}",
    "apple_inline": br"{\rtf1 A\AppleTypeServices B}",
    "apple_initial": br"{\rtf1\AppleTypeServices A}",
    "unknown_inline": br"{\rtf1 A\NotAKnownControl1 B}",
    "cell_final_par": br"{\rtf1\trowd\cellx1000\cellx2000\intbl A\par\cell B\cell\row}",
    "cell_merge": br"{\rtf1\trowd\clmgf\cellx1000\clmrg\cellx2000\intbl A\cell\cell\row}",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--followup", action="store_true")
    parser.add_argument("--fixtures", type=Path, help="Replay an independently captured probe index with input_hex")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    worker = Path(__file__).with_name("wps_export.ps1").resolve()
    rows = []
    cases = FOLLOWUP_CASES if args.followup else CASES
    if args.fixtures:
        index = json.loads(args.fixtures.read_text(encoding="utf-8"))
        cases = {row["case"]: bytes.fromhex(row["input_hex"]) for row in index["files"]}
    for name, source in cases.items():
        if not name.replace("_", "").isalnum():
            raise ValueError("probe names must be alphanumeric or underscores")
        directory = output / name
        directory.mkdir()
        input_path = directory / "input.rtf"
        input_path.write_bytes(source)
        row = {"case": name, "input_sha256": hashlib.sha256(source).hexdigest()}
        try:
            process = subprocess.run(["powershell.exe", "-NoProfile", "-STA", "-File", str(worker),
                                      "-InputPath", str(input_path), "-OutputDirectory", str(directory)],
                                     capture_output=True, timeout=60, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=False)
            (directory / "worker.log").write_bytes(process.stdout + process.stderr)
            result = json.loads((directory / "result.json").read_text(encoding="utf-8-sig"))
            row.update(result)
            if process.returncode != 0 or result["status"] != "Captured":
                raise ValueError("WPS probe failed")
            facts = inspect(directory / "reference.docx")
            assert facts["docx_sha256"] == result["docx_sha256"]
            write_json(directory / "facts.json", facts, exclusive=True)
            region = next(r for r in facts["regions"] if r["kind"] == "document")
            row["body_text"] = [p["text"] for p in region["paragraphs"]]
        except subprocess.TimeoutExpired:
            row.update(status="Fail", reason="timeout", cleanup=stop_owned_worker(directory))
        except (OSError, ValueError, KeyError, AssertionError) as exc:
            row.update(status="Fail", reason=str(exc))
        rows.append(row)
        write_json(output / "probes.json", {"worker_sha256": hashlib.sha256(worker.read_bytes()).hexdigest(), "files": rows})
        print(name, row["status"], ascii(row.get("body_text")), flush=True)
    return 0 if all(r["status"] == "Captured" for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
