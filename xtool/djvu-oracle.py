#!/usr/bin/env python3
"""Compare native extracted DjVu text against an external DjVuLibre binary.

No DjVuLibre source or library is loaded into the Rust application.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def compact(text):
    # Only format separators and Unicode whitespace are advisory. All other
    # characters, including punctuation, accents and combining marks, must match.
    return "".join(c for c in text if not c.isspace() and c not in "\x00\x1d\x1f")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--text", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--retained-page", action="append", default=[], metavar="FILE:PAGE",
                        help="Explicitly verify extra retained text on a page the oracle omits")
    parser.add_argument("--resume", action="store_true", help="Reuse successful rows with unchanged SHA256")
    args = parser.parse_args()
    retained = {}
    for entry in args.retained_page:
        name, page = entry.rsplit(":", 1)
        retained.setdefault(name, []).append(int(page))
    previous = {}
    if args.resume and args.report.exists():
        previous = {row["file"]: row for row in json.loads(args.report.read_text(encoding="utf-8"))["results"]}
    results = []
    for source in sorted(p for p in args.corpus.iterdir() if p.is_file() and p.suffix.lower() in (".djvu", ".djv")):
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        native_path = args.text / source.with_suffix(".txt").name
        native_digest = hashlib.sha256(native_path.read_bytes()).hexdigest()
        old = previous.get(source.name)
        if old and old["sha256"] == digest and old.get("native_sha256") == native_digest and old["equal"]:
            results.append(old)
            continue
        native = (args.text / source.with_suffix(".txt").name).read_text(encoding="utf-8")
        proc = subprocess.run([str(args.oracle), str(source)], capture_output=True, timeout=60)
        oracle = proc.stdout.decode("utf-8")
        left, right = compact(native), compact(oracle)
        row = {"file": source.name, "sha256": digest, "native_sha256": native_digest,
                        "native_characters": len(left), "oracle_characters": len(right),
                        "oracle_exit": proc.returncode,
                        "equal": proc.returncode == 0 and left == right}
        if not row["equal"] and source.name in retained:
            pages = native.split("\f")
            omitted = retained[source.name]
            checks = []
            for page in omitted:
                single = subprocess.run([str(args.oracle), f"--page={page}", str(source)], capture_output=True, timeout=60)
                checks.append(0 < page <= len(pages) and bool(compact(pages[page - 1]))
                              and single.returncode == 0 and not compact(single.stdout.decode("utf-8")))
            rest = compact("".join(text for page, text in enumerate(pages, 1) if page not in omitted))
            row["retained_pages"] = omitted
            row["verified_retention_difference"] = all(checks) and rest == right and proc.returncode == 0
            row["detail"] = "Native UTF-8 text retained on unusable zone page; oracle omits it; all remaining characters match"
        print(source.name, "OK" if row["equal"] else "VERIFIED RETENTION" if row.get("verified_retention_difference") else "MISMATCH", flush=True)
        results.append(row)
    report = {"files": len(results), "equal": sum(row["equal"] for row in results),
              "accounted": sum(row["equal"] or row.get("verified_retention_difference", False) for row in results),
              "normalization": "Remove whitespace, native structural separators and NUL (djvutxt converts the terminating NUL to a newline)",
              "results": results}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("files", "equal", "accounted")}))
    return 0 if report["files"] > 0 and report["accounted"] == report["files"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
