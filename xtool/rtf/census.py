#!/usr/bin/env python3
"""Bounded lexical RTF census. Never a semantic parser or a text oracle."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import sys

VERSION = 1
MAX_INPUT = 64 * 1024 * 1024
MAX_DEPTH = 256
MAX_TOKENS = 10_000_000
DESTINATIONS = set("""
fonttbl colortbl stylesheet listtable listoverridetable info generator
header headerl headerr headerf footer footerl footerr footerf footnote
annotation atnauthor atnid pict shppict nonshppict object objdata result
field fldinst fldrslt bkmkstart bkmkend filetbl revtbl rsidtbl xmlnstbl
datastore themedata colorschememapping latentstyles docvar upr ud shptxt
shp shpinst shpgrp sp sn sv pn pntext listtext private datafield
""".split())
FEATURES = {
    "unicode": {"u", "uc", "upr", "ud"},
    "font_tables": {"fonttbl"},
    "color_tables": {"colortbl"},
    "styles": {"stylesheet"},
    "lists": {"listtable", "listoverridetable", "pn", "pntext", "listtext", "ls", "ilvl"},
    "tables": {"trowd", "cell", "row", "intbl", "cellx"},
    "nested_tables": {"nesttableprops", "nestrow", "nestcell"},
    "merges": {"clmgf", "clmrg", "clvmgf", "clvmrg"},
    "pictures": {"pict"},
    "png": {"pngblip"},
    "jpeg": {"jpegblip"},
    "metafiles": {"wmetafile", "emfblip"},
    "bitmap": {"dibitmap", "wbitmap"},
    "objects": {"object", "objdata", "objemb", "objlink"},
    "shapes": {"shp", "shpinst", "shpgrp", "shptxt"},
    "fields": {"field", "fldinst", "fldrslt"},
    "headers_footers": {"header", "headerl", "headerr", "headerf", "footer", "footerl", "footerr", "footerf"},
    "notes": {"footnote", "ftnalt"},
    "comments": {"annotation", "atnauthor", "atnid"},
    "revisions": {"revised", "deleted", "revauth", "revdttm", "insrsid", "delrsid"},
    "sections": {"sect", "sectd", "sbknone", "sbkpage", "sbkcol", "sbkeven", "sbkodd"},
    "columns": {"cols", "colno", "colw", "colsr"},
    "rtl": {"rtlch", "rtlpar", "rtlsect", "rtldoc"},
    "math": {"mmath", "mmathP", "mfrac", "mrad", "mobj"},
    "html_encapsulation": {"fromhtml", "htmltag", "htmlrtf"},
    "binary": {"bin"},
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def scan(data: bytes, *, max_depth: int = MAX_DEPTH, max_tokens: int = MAX_TOKENS) -> dict:
    """Count lexical constructs, treating binary bytes and escapes atomically.

    Counts include ignored destinations; no visible-text or support claim follows.
    First control words describe group starts, not a complete destination registry.
    """
    words: Counter = Counter()
    symbols: Counter = Counter()
    first_words: Counter = Counter()
    destinations: Counter = Counter()
    ignorable: Counter = Counter()
    parameters: dict[str, Counter] = {name: Counter() for name in ("ansicpg", "fcharset", "cpg", "uc", "rtf", "itap", "revised", "deleted")}
    first_offsets: dict[str, int] = {}
    stack: list[dict] = []
    errors: list[dict] = []
    warnings: list[dict] = []
    maximum_depth = groups = tokens = text_bytes = binary_bytes = hex_escapes = 0
    root_closed = False
    i = 0

    def error(code: str, offset: int, detail: str) -> None:
        errors.append({"id": code, "offset": offset, "detail": detail})

    if len(data) > MAX_INPUT:
        error("input_limit", 0, f"{len(data)} > {MAX_INPUT}")
    elif not re.match(rb"\{\\rtf[0-9]+(?:[^a-zA-Z0-9]|$)", data):
        error("invalid_header", 0, "expected root {\\rtfN at byte zero")

    while i < len(data) and not errors:
        start = i
        ch = data[i]
        if ch in (10, 13):
            i += 1
            continue
        if root_closed:
            if ch in (9, 32):
                i += 1
                continue
            if ch == 0 and all(value in (0, 9, 10, 13, 32) for value in data[i:]):
                warnings.append({"id": "nul_trailer_candidate", "offset": i, "bytes": len(data) - i,
                                 "detail": "padding after complete root; importer acceptance requires explicit policy"})
                i = len(data)
                break
            error("trailing_content", i, "non-whitespace after root group")
            break
        tokens += 1
        if tokens > max_tokens:
            error("token_limit", i, f"more than {max_tokens} tokens")
            break
        if ch == 123:
            if stack:
                stack[-1]["leading"] = False
            stack.append({"leading": True, "ignorable": False, "offset": i})
            groups += 1
            maximum_depth = max(maximum_depth, len(stack))
            i += 1
            if len(stack) > max_depth:
                error("depth_limit", start, f"more than {max_depth} groups")
            continue
        if ch == 125:
            if not stack:
                error("unexpected_close", i, "closing brace without group")
                break
            stack.pop()
            i += 1
            root_closed = not stack
            continue
        if not stack:
            error("outside_root", i, "content outside root group")
            break
        frame = stack[-1]
        if ch != 92:
            # One text span is one token, including picture hex payload.
            j = i + 1
            while j < len(data) and data[j] not in (92, 123, 125, 10, 13):
                j += 1
            text_bytes += j - i
            frame["leading"] = False
            i = j
            continue
        i += 1
        if i == len(data):
            error("truncated_escape", start, "backslash at end")
            break
        ch = data[i]
        if not (65 <= ch <= 90 or 97 <= ch <= 122):
            symbols[chr(ch)] += 1
            i += 1
            if ch == 39:
                if i + 2 > len(data) or any(x not in b"0123456789abcdefABCDEF" for x in data[i:i + 2]):
                    error("invalid_hex_escape", start, "expected two hex digits")
                    break
                hex_escapes += 1
                i += 2
            if ch == 42 and frame["leading"]:
                frame["ignorable"] = True
            else:
                frame["leading"] = False
            continue
        word_start = i
        while i < len(data) and (65 <= data[i] <= 90 or 97 <= data[i] <= 122):
            i += 1
        name = data[word_start:i].decode("ascii")
        if len(name) > 32:
            error("control_word_limit", start, "control word longer than 32 letters")
            break
        number_start = i
        if i < len(data) and data[i] == 45:
            i += 1
        digit_start = i
        while i < len(data) and 48 <= data[i] <= 57:
            i += 1
        if i > digit_start:
            if i - digit_start > 10:
                error("parameter_overflow", start, "parameter exceeds 10 decimal digits")
                break
            parameter = int(data[number_start:i])
            if not -(2**31) <= parameter < 2**31:
                error("parameter_overflow", start, "parameter outside signed 32-bit range")
                break
        else:
            parameter = None
            # A '-' not followed by digits is ordinary following text.
            i = number_start
        if i < len(data) and data[i] == 32:
            i += 1
        words[name] += 1
        first_offsets.setdefault(name, start)
        if name in parameters:
            parameters[name][str(parameter)] += 1
        if frame["leading"]:
            first_words[name] += 1
            if name in DESTINATIONS:
                destinations[name] += 1
            if frame["ignorable"]:
                ignorable[name] += 1
            frame["leading"] = False
        if name == "bin":
            if parameter is None or parameter < 0:
                error("invalid_binary_length", start, "bin requires nonnegative length")
                break
            if parameter > len(data) - i:
                error("truncated_binary", start, f"requested {parameter}, available {len(data) - i}")
                break
            binary_bytes += parameter
            i += parameter

    if not errors and stack:
        error("unclosed_group", stack[-1]["offset"], f"{len(stack)} group(s) still open")
    feature_hits = {}
    for feature, triggers in FEATURES.items():
        hits = {name: words[name] for name in sorted(triggers) if words[name]}
        if feature == "nested_tables":
            nested_itap = sum(count for value, count in parameters["itap"].items() if value != "None" and int(value) > 1)
            if nested_itap:
                hits["itap_gt_1"] = nested_itap
        if hits:
            feature_hits[feature] = {"controls": hits, "first_offset": min(first_offsets["itap" if name == "itap_gt_1" else name] for name in hits)}
    return {
        "status": "Failed" if errors else "Pass",
        "claim": "bounded_lexical_scan_only",
        "complete": not errors,
        "errors": errors,
        "warnings": warnings,
        "scanned_bytes": i,
        "tokens": tokens,
        "groups": groups,
        "max_group_depth": maximum_depth,
        "text_payload_bytes": text_bytes,
        "binary_bytes": binary_bytes,
        "hex_escapes": hex_escapes,
        "control_words": dict(sorted(words.items())),
        "control_symbols": dict(sorted(symbols.items())),
        "group_first_control_words": dict(sorted(first_words.items())),
        "recognized_destination_groups": dict(sorted(destinations.items())),
        "ignorable_group_first_words": dict(sorted(ignorable.items())),
        "parameters": {name: dict(sorted(count.items())) for name, count in parameters.items()},
        "feature_hits": feature_hits,
    }


def inventory(root: Path) -> dict:
    manifest_data = (root / "manifest.json").read_bytes()
    entries = json.loads(manifest_data)
    if not isinstance(entries, list):
        raise ValueError("manifest must be an array")
    names = [entry["filename"] for entry in entries]
    actual = {path.name for path in root.iterdir() if path.is_file() and path.suffix.lower() == ".rtf"}
    global_errors = []
    # Any corpus with a manifest is checked against it, whatever its size.
    if not entries or len(set(names)) != len(entries):
        global_errors.append("expected a non-empty manifest with unique filenames")
    if actual != set(names):
        global_errors.append({"missing": sorted(set(names) - actual), "extra": sorted(actual - set(names))})
    rows = []
    for entry in entries:
        name = entry["filename"]
        if Path(name).name != name or any(char in name for char in "/\\:"):
            raise ValueError("manifest filename must be a single local filename")
        row = {"filename": name, "expected_sha256": entry["sha256"], "language": entry.get("language"), "integrity": "Fail"}
        try:
            path = root / name
            if path.resolve().parent != root.resolve():
                raise ValueError("manifest input resolves outside corpus directory")
            row["byte_size"] = path.stat().st_size
            if row["byte_size"] > MAX_INPUT:
                row["error"] = "input_limit"
            else:
                data = path.read_bytes()
                row["sha256"] = sha256(data)
                row["integrity"] = "Pass" if row["sha256"] == entry["sha256"] and len(data) == entry["byte_size"] else "Fail"
                row["lexical"] = scan(data)
        except OSError as exc:
            row["error"] = f"{type(exc).__name__}: {exc.strerror}"
        rows.append(row)
    feature_files = Counter()
    control_files = Counter()
    control_counts = Counter()
    for row in rows:
        lexical = row.get("lexical", {})
        feature_files.update(lexical.get("feature_hits", {}).keys())
        control_files.update(lexical.get("control_words", {}).keys())
        control_counts.update(lexical.get("control_words", {}))
    return {
        "version": VERSION,
        "manifest_sha256": sha256(manifest_data),
        "limits": {"input_bytes": MAX_INPUT, "group_depth": MAX_DEPTH, "tokens": MAX_TOKENS},
        "global_errors": global_errors,
        "summary": {
            "files": len(rows),
            "integrity_pass": sum(row["integrity"] == "Pass" for row in rows),
            "lexical_pass": sum(row.get("lexical", {}).get("status") == "Pass" for row in rows),
            "lexical_warning_files": sum(bool(row.get("lexical", {}).get("warnings")) for row in rows),
            "total_bytes": sum(row.get("byte_size", 0) for row in rows),
            "max_group_depth": max((row.get("lexical", {}).get("max_group_depth", 0) for row in rows), default=0),
            "feature_file_counts": dict(sorted(feature_files.items())),
            "control_file_counts": dict(sorted(control_files.items())),
            "control_occurrences": dict(sorted(control_counts.items())),
        },
        "files": rows,
    }


def expectation_template(result: dict) -> dict:
    return {
        "version": VERSION,
        "manifest_sha256": result["manifest_sha256"],
        "status": "PendingIndependentOracle",
        "oracle": {"status": "Blocked", "application": None, "version": None, "options": None},
        "files": [{
            "filename": row["filename"],
            "sha256": row["expected_sha256"],
            "expected_import_outcome": None,
            "text_oracle": "Blocked",
            "structure_oracle": "Blocked",
            "visual_oracle": "Blocked",
            "lexical_candidates": sorted(row.get("lexical", {}).get("feature_hits", {})),
            "reviewed_unsupported": [],
            "reviewed": False,
        } for row in result["files"]],
    }


def write_json(path: Path, data: dict, *, exclusive: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x" if exclusive else "w", encoding="utf-8", newline="\n") as handle:
        json.dump(data, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=Path("testdata/CC0_RTF"))
    parser.add_argument("--output", type=Path, default=Path("target/rtf-import/R01/corpus/census.json"))
    parser.add_argument("--expectations-template", type=Path, help="create a new pending template; never overwrite reviewed expectations")
    args = parser.parse_args()
    try:
        result = inventory(args.corpus)
        write_json(args.output, result)
        if args.expectations_template:
            write_json(args.expectations_template, expectation_template(result), exclusive=True)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"census preparation failed: {exc}", file=sys.stderr)
        return 2
    summary = result["summary"]
    print(json.dumps({key: value for key, value in summary.items() if key not in ("control_occurrences", "control_file_counts")}, ensure_ascii=False, sort_keys=True))
    files = summary["files"]
    return 0 if not result["global_errors"] and summary["integrity_pass"] == files and summary["lexical_pass"] == files else 1


if __name__ == "__main__":
    sys.exit(main())
