#!/usr/bin/env python3
"""Independent ActualText extraction witness (PyMuPDF 1.26.4).

Generate input PDFs using STRICTLIB_FONT_WITNESS_OUTPUT and the Rust test
licensed_programs_embed_with_cjk_unicode_and_explicit_indic_degradation.
This gate never installs packages or treats missing inputs as passing.
"""
import argparse
import hashlib
import json
from pathlib import Path
import pymupdf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdfs", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    fixtures = Path(__file__).resolve().parents[2] / "strict-ooxml-render-svg/fonts/witnesses"
    rows = []
    for key in ("cjk", "devanagari"):
        path = args.pdfs / (key + ".pdf")
        expected = (fixtures / (key + ".txt")).read_text(encoding="utf-8")
        with pymupdf.open(path) as document:
            assert len(document) == 1, key + ": unexpected pages"
            raw = document[0].get_text()
        actual = "".join(raw.split())
        assert actual == expected, key + ": Unicode extraction differs: " + repr(raw)
        assert "\ufffd" not in raw, key + ": replacement character"
        rows.append({"id": key, "status": "Pass", "pdf_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                     "expected_utf8_sha256": hashlib.sha256(expected.encode("utf-8")).hexdigest(),
                     "extracted_text": actual})
    receipt = {"claim": "narrow Unicode extraction only, not full script/visual acceptance",
               "pymupdf": pymupdf.VersionBind, "mupdf": pymupdf.VersionFitz, "files": rows}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("Pass: CJK and Devanagari Unicode extraction")


if __name__ == "__main__":
    main()
