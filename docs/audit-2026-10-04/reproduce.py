"""Independent, bounded probes of the existing CLI; no production code changes.

Run after: cargo +1.92.0 build -p strict-ooxml-cli --locked
"""
import json
import re
import shutil
import subprocess
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
CLI = ROOT / "target/debug/strict-ooxml.exe"
W = "http://purl.oclc.org/ooxml/wordprocessingml/main"


def pdf(path, pages):
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"]
    kids = []
    for width, height, stream in pages:
        page_id = len(objects) + 1
        kids.append(f"{page_id} 0 R")
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 3 0 R >> >> /Contents {page_id + 1} 0 R >>".encode())
        objects.append(f"<< /Length {len(stream)} >>\nstream\n".encode() + stream + b"\nendstream")
    objects[1] = f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(kids)} >>".encode()
    result = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(result))
        result.extend(f"{i} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(result)
    result.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        result.extend(f"{offset:010d} 00000 n \n".encode())
    result.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    path.write_bytes(result)


def run(*args):
    result = subprocess.run([str(CLI), *map(str, args)], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
    return {"args": list(map(str, args)), "exit": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def docx_facts(path):
    with zipfile.ZipFile(path) as z:
        document = ET.fromstring(z.read("word/document.xml"))
    return {
        "sections": len(document.findall(f".//{{{W}}}sectPr")),
        "sizes": [dict(el.attrib) for el in document.findall(f".//{{{W}}}pgSz")],
        "page_breaks": len(document.findall(f".//{{{W}}}br[@{{{W}}}type='page']")),
        "text": [el.text for el in document.findall(f".//{{{W}}}t")],
        "drawings": len(document.findall(f".//{{{W}}}drawing")),
        "xml": ET.tostring(document, encoding="unicode"),
    }


def transitional(path):
    ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    body = f'<w:document xmlns:w="{ns}" xmlns:v="urn:schemas-microsoft-com:vml"><w:body><w:p><w:r><w:t>KEEP THIS TEXT</w:t></w:r></w:p><w:p><w:r><w:pict><v:shape id="lost-shape" style="width:100pt;height:60pt"/></w:pict></w:r></w:p></w:body></w:document>'
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
        z.writestr("_rels/.rels", '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        z.writestr("word/document.xml", body)


def flawed_ratio(want, have):
    index = 0
    for ch in have:
        while index < len(want) and want[index] != ch:
            index += 1
        if index < len(want):
            index += 1
    return index / len(want)


def settings_probe():
    incoming = OUT / "schema-input"
    outgoing = OUT / "schema-output"
    incoming.mkdir(exist_ok=True)
    outgoing.mkdir(exist_ok=True)
    src = incoming / "settings.docx"
    dst = outgoing / "settings.docx"
    transitional(src)
    with zipfile.ZipFile(src) as z:
        parts = {name: z.read(name) for name in z.namelist()}
    ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    parts["word/document.xml"] = f'<w:document xmlns:w="{ns}"><w:body><w:p><w:r><w:t>SETTINGS PROBE</w:t></w:r></w:p></w:body></w:document>'.encode()
    parts["word/settings.xml"] = f'<w:settings xmlns:w="{ns}"><w:stylePaneFormatFilter w:val="0001"/></w:settings>'.encode()
    parts["word/_rels/document.xml.rels"] = b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>'
    parts["[Content_Types].xml"] = parts["[Content_Types].xml"].replace(b"</Types>", b'<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>')
    with zipfile.ZipFile(src, "w", zipfile.ZIP_DEFLATED) as z:
        for name, content in parts.items():
            z.writestr(name, content)
    result = run("write", src, "--transitional", "--out", dst)
    if dst.exists():
        with zipfile.ZipFile(dst) as z:
            result["written_settings_xml"] = z.read("word/settings.xml").decode()
    return result


if __name__ == "__main__":
    cases = {}
    multi = OUT / "two-pages.pdf"
    pdf(multi, [(612, 792, b"BT /F1 12 Tf 72 700 Td (PAGE ONE) Tj ET"), (842, 595, b"BT /F1 12 Tf 72 500 Td (PAGE TWO) Tj ET")])
    vector = OUT / "text-and-vector.pdf"
    pdf(vector, [(612, 792, b"BT /F1 12 Tf 72 700 Td (VECTOR DIAGRAM BELOW) Tj ET\n1 0 0 rg 100 400 200 80 re f")])
    for name, src in [("multipage", multi), ("vector", vector)]:
        for mode in ("semantic", "visual"):
            dst = OUT / f"{name}-{mode}.docx"
            key = f"{name}-{mode}"
            cases[key] = {"conversion": run("from-pdf", src, "--mode", mode, "--out", dst)}
            if dst.exists():
                cases[key]["docx"] = docx_facts(dst)
                rendered = OUT / f"{key}-svg"
                assert rendered.resolve().is_relative_to(OUT.resolve())
                for old_page in rendered.glob("page-*.svg"):
                    old_page.unlink()
                cases[key]["render"] = run("render", dst, "--out", rendered)
                cases[key]["rendered_pages"] = len(list(rendered.glob("page-*.svg")))
                first_svg = rendered / "page-1.svg"
                if first_svg.exists():
                    svg = ET.fromstring(first_svg.read_text(encoding="utf-8"))
                    texts = svg.findall(".//{http://www.w3.org/2000/svg}text")
                    cases[key]["first_svg_text"] = dict(texts[0].attrib) if texts else None
                if key == "vector-visual":
                    cases[key]["expected_first_text_px_at_96dpi"] = {"x": 96.0, "y": 92 * 96 / 72}
    src = OUT / "vml-loss.docx"
    transitional(src)
    dst = OUT / "vml-loss-strict.docx"
    cases["normalization-loss"] = {"write": run("write", src, "--transitional", "--out", dst)}
    if dst.exists():
        cases["normalization-loss"]["docx"] = docx_facts(dst)
    cases["text-test-metric"] = {"expected": "abc", "actual": "c", "reported_ratio": flawed_ratio("abc", "c"), "actual_character_recall": 1 / 3, "unrelated_actual": "Z", "unrelated_reported_ratio": flawed_ratio("abc", "Z")}
    cases["settings-schema-regression"] = settings_probe()
    strict_output = OUT / "strict-output"
    strict_output.mkdir(exist_ok=True)
    for name in ("multipage-semantic", "multipage-visual", "vector-semantic", "vector-visual", "vml-loss-strict"):
        source = OUT / f"{name}.docx"
        if source.exists():
            shutil.copyfile(source, strict_output / source.name)
    (OUT / "probes.json").write_text(json.dumps(cases, ensure_ascii=False, indent=2), encoding="utf-8")
    for key, value in cases.items():
        print(key, json.dumps({k: v for k, v in value.items() if k != "docx"}, ensure_ascii=False))
        if "docx" in value:
            print("  document", json.dumps({k: v for k, v in value["docx"].items() if k != "xml"}, ensure_ascii=False))
