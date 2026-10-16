"""Compare a recognised scan with the scan, page by page.

    python xtool/ocr-qa/compare.py --source book.pdf --first 1 --docx ocr.docx \
        --cli strict-ooxml.exe --out compare/

`from-pdf --ocr --ocr-layout` starts every page it rebuilt on a page of its
own (`w:pageBreakBefore`); a page of text flows on from the one before, and
its first paragraph carries `w:lastRenderedPageBreak`, Word's note that a
page broke there. This script cuts the DOCX at both, renders each part with
the CLI's `to-pdf`, and sets it beside the source's page:

- `pages`: how many pages the part took - 1 when the page's content fits;
- `ink_iou`: how far the ink lies where the scan's does, on a 40x56 grid of
  the page (1 = same cells inked); fonts differ, so this measures where the
  blocks are, not the letters;
- `ink_ratio`: the part's inked cells over the scan's (text lost or spread);
- `text_share`: of what the model read on the page, the share that is text
  of the document rather than a picture's description - a page kept as a
  picture of the scan matches the scan's ink and keeps none of its text, so
  ink alone would reward it.

Writes `page-NNN.png` (scan | ours), `summary.json`, `results.csv` and
`index.html` with the pages worst first. Needs pypdfium2, numpy and Pillow.
"""

from __future__ import annotations

import argparse
import csv
import html
import io
import json
import re
import subprocess
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

import numpy as np
import pypdfium2
from PIL import Image

W = "http://purl.oclc.org/ooxml/wordprocessingml/main"
GRID = (40, 56)
DPI = 60


def namespaces(xml: bytes) -> list[tuple[str, str]]:
    found = []
    for _event, pair in ET.iterparse(io.BytesIO(xml), events=("start-ns",)):
        found.append(pair)
    return found


def split_docx(docx: Path, out: Path) -> list[Path]:
    """The DOCX cut before every paragraph that starts a source page."""
    with zipfile.ZipFile(docx) as archive:
        entries = {name: archive.read(name) for name in archive.namelist()}
    xml = entries["word/document.xml"]
    for prefix, uri in namespaces(xml):
        ET.register_namespace(prefix, uri)
    root = ET.fromstring(xml)
    w = W if root.tag.startswith("{" + W) else root.tag.split("}")[0][1:]
    body = root.find(f"{{{w}}}body")
    if body is None:
        raise SystemExit("the document has no body")
    children = list(body)
    section = children[-1] if children and children[-1].tag == f"{{{w}}}sectPr" else None
    content = [child for child in children if child is not section]
    parts: list[list[ET.Element]] = [[]]
    for child in content:
        breaks = (child.find(f"{{{w}}}pPr/{{{w}}}pageBreakBefore") is not None
                  or child.find(f"{{{w}}}r/{{{w}}}lastRenderedPageBreak") is not None)
        if breaks and parts[-1]:
            parts.append([])
        parts[-1].append(child)
    paths = []
    for index, part in enumerate(parts, start=1):
        for child in list(body):
            body.remove(child)
        # A part that ends a section keeps that section as its own last one;
        # left in the paragraph it would start an empty page after the part.
        own_section = None
        if part:
            properties = part[-1].find(f"{{{w}}}pPr")
            inner = properties.find(f"{{{w}}}sectPr") if properties is not None else None
            if inner is not None:
                properties.remove(inner)
                own_section = inner
        for child in part:
            body.append(child)
        if own_section is not None:
            body.append(own_section)
        elif section is not None:
            body.append(section)
        data = (
            b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
            + ET.tostring(root, encoding="unicode").encode("utf-8")
        )
        path = out / f"part-{index:03}.docx"
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
            for name, payload in entries.items():
                archive.writestr(name, data if name == "word/document.xml" else payload)
        paths.append(path)
    return paths


def text_counts(docx: Path) -> tuple[int, int]:
    """Characters of document text, and of picture descriptions."""
    with zipfile.ZipFile(docx) as archive:
        xml = archive.read("word/document.xml").decode("utf-8", "replace")
    descriptions = sum(len(html.unescape(value)) for value in re.findall(r'descr="([^"]*)"', xml))
    text = sum(len(html.unescape(value)) for value in re.findall(r"<w:t(?:\s[^>]*)?>([^<]*)</w:t>", xml))
    # A picture's description is written twice (docPr and pic:cNvPr).
    return text, descriptions // 2


def ink_grid(image: Image.Image) -> np.ndarray:
    pixels = np.asarray(image.convert("RGB")).astype(int)
    luma = 0.299 * pixels[..., 0] + 0.587 * pixels[..., 1] + 0.114 * pixels[..., 2]
    chroma = pixels.max(-1) - pixels.min(-1)
    paper = np.median(luma)
    # Ink is what stands apart from the paper either way: dark on a light page,
    # light on a dark slide (where measuring only "darker than the paper" saw
    # no ink on either side and scored a blank result 1.0).
    dark = luma < min(paper - 70, 170)
    light = luma > max(paper + 70, 85)
    ink = dark | light | ((chroma > 60) & (np.abs(luma - paper) > 20))
    height, width = ink.shape
    columns, rows = GRID
    grid = np.zeros((rows, columns), bool)
    for row in range(rows):
        for column in range(columns):
            cell = ink[row * height // rows:(row + 1) * height // rows,
                       column * width // columns:(column + 1) * width // columns]
            grid[row, column] = cell.size > 0 and cell.mean() > 0.02
    return grid


def render(pdf: Path | str, index: int) -> Image.Image:
    document = pypdfium2.PdfDocument(str(pdf))
    return document[index].render(scale=DPI / 72).to_pil().convert("RGB")


def sheet(images: list[Image.Image]) -> Image.Image:
    height = max(image.height for image in images)
    width = sum(image.width for image in images) + 10 * (len(images) - 1)
    out = Image.new("RGB", (width, height), (180, 180, 180))
    x = 0
    for image in images:
        out.paste(image, (x, 0))
        x += image.width + 10
    return out


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--first", type=int, default=1, help="1-based page the DOCX starts at")
    parser.add_argument("--docx", type=Path, required=True)
    parser.add_argument("--cli", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    parts_dir = args.out / "parts"
    parts_dir.mkdir(exist_ok=True)
    parts = split_docx(args.docx, parts_dir)
    source_pages = len(pypdfium2.PdfDocument(str(args.source)))
    rows = []
    for offset, part in enumerate(parts):
        page = args.first + offset
        if page > source_pages:
            break
        pdf = part.with_suffix(".pdf")
        run = subprocess.run([args.cli, "to-pdf", str(part), "--out", str(pdf)],
                             capture_output=True, text=True, encoding="utf-8", errors="replace")
        scan = render(args.source, page - 1)
        if not pdf.exists():
            rows.append({"page": page, "pages": 0, "ink_iou": 0.0, "ink_ratio": 0.0,
                         "text_share": 0.0, "error": run.stderr.strip()[-300:]})
            continue
        ours_doc = pypdfium2.PdfDocument(str(pdf))
        ours = [ours_doc[i].render(scale=DPI / 72).to_pil().convert("RGB")
                for i in range(min(len(ours_doc), 3))]
        a, b = ink_grid(scan), ink_grid(ours[0])
        union = (a | b).sum()
        text, described = text_counts(part)
        rows.append({
            "page": page,
            "pages": len(ours_doc),
            "ink_iou": round(float((a & b).sum() / union) if union else 1.0, 3),
            "ink_ratio": round(float(b.sum() / a.sum()) if a.sum() else 0.0, 3),
            "text_share": round(text / (text + described), 3) if text + described else 1.0,
            "error": "",
        })
        sheet([scan] + ours).save(args.out / f"page-{page:03}.png")
    worst = sorted(rows, key=lambda row: (row["pages"] == 1, row["text_share"] >= 0.5, row["ink_iou"]))
    summary = {
        "pages": len(rows),
        "parts": len(parts),
        "overflowing": sum(row["pages"] > 1 for row in rows),
        "failed": sum(row["pages"] == 0 for row in rows),
        "mean_ink_iou": round(float(np.mean([row["ink_iou"] for row in rows])) if rows else 0.0, 3),
        "mean_text_share": round(float(np.mean([row["text_share"] for row in rows])) if rows else 0.0, 3),
        "pictured": sum(row["text_share"] < 0.5 for row in rows),
        "result_pages": sum(row["pages"] for row in rows),
        "median_ink_iou": round(float(np.median([row["ink_iou"] for row in rows])) if rows else 0.0, 3),
    }
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    with (args.out / "results.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["page", "pages", "ink_iou", "ink_ratio", "text_share", "error"])
        writer.writeheader()
        writer.writerows(rows)
    items = "\n".join(
        f"<tr><td>{row['page']}</td><td>{row['pages']}</td><td>{row['ink_iou']}</td>"
        f"<td>{row['ink_ratio']}</td><td>{row['text_share']}</td><td>{html.escape(row['error'])}"
        f"<a href='page-{row['page']:03}.png'><img src='page-{row['page']:03}.png' height='260'></a></td></tr>"
        for row in worst)
    (args.out / "index.html").write_text(
        "<!doctype html><meta charset='utf-8'><title>OCR compare</title>"
        "<style>body{font-family:sans-serif}td{vertical-align:top;padding:4px}</style>"
        f"<h1>Scan vs recognised</h1><pre>{html.escape(json.dumps(summary, indent=2))}</pre>"
        "<p>Worst first: pages that overflow, then pages mostly kept as pictures (text share under 0.5), then by ink overlap. Each picture is the scan "
        "and the pages our DOCX took for it.</p>"
        f"<table><tr><th>page</th><th>pages</th><th>ink IoU</th><th>ink ratio</th><th>text share</th><th></th></tr>{items}</table>",
        encoding="utf-8")
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
