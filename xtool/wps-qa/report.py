"""Aggregates per-document measurements into summary.json, results.csv and
report.html under `<store>/cmp/<label>/`."""

from __future__ import annotations

import csv
import html
import statistics
import time
from pathlib import Path

import wpsqa_common as common
from wpsqa_common import read_json, write_json

COLUMNS = [
    ("corpus", "corpus"),
    ("key", "document"),
    ("status", "status"),
    ("readability", "readability"),
    ("ref_readability", "WPS readability"),
    ("read_completeness", "complete & visible"),
    ("read_order", "reading order"),
    ("read_legibility", "no overlap"),
    ("read_images", "pictures"),
    ("readability_delta", "Δ readability vs baseline"),
    ("score", "WPS fidelity"),
    ("grade", "grade"),
    ("pages_ref", "pages WPS"),
    ("pages_ours", "pages ours"),
    ("text_f1", "text F1"),
    ("text_recall", "text recall"),
    ("same_page", "same page"),
    ("line_break_f1", "line breaks F1"),
    ("geom_abs", "abs ≤4pt"),
    ("geom_advance", "advance"),
    ("geom_leading", "leading"),
    ("median_dy_pt", "median |dy| pt"),
    ("mean_ssim", "SSIM"),
    ("row_corr", "row corr"),
    ("delta", "Δ score vs baseline"),
    ("import_status", "RTF import"),
    ("import_losses", "import losses"),
    ("ref_fallback", "WPS fallback font share"),
]


def _ref_fallback(store: Path, doc: common.Doc) -> float | None:
    """Share of the reference drawn in a face the document does not name
    (`metrics.fallback_share`), cached beside the reference (keyed by its and
    the source's size and mtime: a new capture is read again)."""
    pdf = store / "refs" / doc.corpus / f"{doc.key}.pdf"
    if not pdf.is_file():
        return None
    stat = pdf.stat()
    stamp = [stat.st_size, int(stat.st_mtime)]
    cache_path = store / "refs" / doc.corpus / f"{doc.key}.fonts.json"
    cache = read_json(cache_path) or {}
    if cache.get("stamp") != stamp:
        import metrics

        try:
            shares = metrics.font_shares(str(pdf))
        except Exception:  # noqa: BLE001
            return None
        cache = {"stamp": stamp, "shares": shares}
        write_json(cache_path, cache)
    import metrics

    if "named" not in cache or cache.get("source_stamp") != _stamp(doc.path):
        cache["named"] = sorted(metrics.named_families(str(doc.path)))
        cache["source_stamp"] = _stamp(doc.path)
        write_json(cache_path, cache)
    return metrics.fallback_share(cache.get("shares", {}), set(cache["named"]), _host_families(store))


def _stamp(path: Path) -> list[int]:
    stat = Path(path).stat()
    return [stat.st_size, int(stat.st_mtime)]


_HOST_FAMILIES: dict[str, dict[str, str]] = {}


def _host_families(store: Path) -> dict[str, str]:
    """PostScript name -> legacy family, from the host inventory."""
    key = str(store)
    if key not in _HOST_FAMILIES:
        inventory = read_json(store / "host-fonts.json") or {}
        _HOST_FAMILIES[key] = {
            face["postscript"]: face.get("legacy_family") or face.get("family", "")
            for face in inventory.get("faces", [])
            if face.get("postscript")
        }
    return _HOST_FAMILIES[key]


def _is_fallback(row: dict) -> bool:
    import metrics

    return (row.get("ref_fallback") or 0.0) >= metrics.FALLBACK_SHARE


def _group(rows: list[dict]) -> dict:
    scored = [r for r in rows if isinstance(r.get("score"), (int, float))]
    return {
        "documents": len(rows),
        "mean_readability": _mean([r.get("readability") for r in rows]),
        "mean_score": _mean([r["score"] for r in scored]),
        "grades": {g: sum(r.get("grade") == g for r in scored) for g in "ABCDF"},
        "mean_delta": _mean([r.get("delta") for r in rows]),
    }


def _row(doc: common.Doc, store: Path, label: str) -> dict:
    ref = read_json(store / "refs" / doc.corpus / f"{doc.key}.json") or {}
    ours = read_json(store / "ours" / label / doc.corpus / f"{doc.key}.json") or {}
    cmp_ = read_json(store / "cmp" / label / doc.corpus / f"{doc.key}.json") or {}
    row: dict = {"corpus": doc.corpus, "key": doc.key, "ident": doc.ident}
    if ref.get("status") == "ok":
        row["ref_fallback"] = _ref_fallback(store, doc)
    # An RTF is imported before it is rendered: how that went is part of it.
    if "import_status" in ours:
        row["import_status"] = ours.get("import_status")
        row["import_losses"] = ours.get("import_losses")
    if ref.get("status") != "ok":
        row["status"] = f"wps-{ref.get('status', 'missing')}"
        row["error"] = ref.get("error", "")
        return row
    if ours.get("status") != "ok":
        row["status"] = f"ours-{ours.get('status', 'missing')}"
        row["error"] = ours.get("stderr_tail", "")[-300:]
        row["score"] = 0.0
        row["grade"] = "F"
        row["pages_ref"] = ref.get("pages")
        return row
    if cmp_.get("status") != "ok":
        row["status"] = f"cmp-{cmp_.get('status', 'missing')}"
        row["error"] = cmp_.get("error", "")
        return row
    raster = cmp_.get("raster", {})
    row.update(
        {
            "status": "ok",
            "score": cmp_["score"],
            "grade": cmp_["grade"],
            "pages_ref": cmp_["pages"]["ref"],
            "pages_ours": cmp_["pages"]["ours"],
            "text_f1": cmp_["text"]["score"],
            "text_recall": cmp_["text"]["recall"],
            "same_page": cmp_["flow"]["same_page"],
            "line_break_f1": cmp_["flow"]["line_breaks"]["f1"],
            "geom_abs": cmp_["geometry"].get("absolute"),
            "geom_advance": cmp_["geometry"].get("advance"),
            "geom_leading": cmp_["geometry"].get("leading"),
            "geom_score": cmp_["geometry"]["score"],
            "median_dy_pt": cmp_["geometry"]["median_dy_pt"],
            "mean_ssim": raster.get("mean_ssim"),
            "row_corr": raster.get("mean_row_corr"),
            "worst_page": raster.get("worst_page"),
            "image": f"img/{doc.corpus}/{doc.key}.png" if cmp_.get("image") else None,
            "sub": {k: cmp_[k]["score"] for k in ("pages", "text", "flow", "geometry", "raster") if k in cmp_},
        }
    )
    read = cmp_.get("readability") or {}
    if read.get("score") is not None:
        row.update(
            {
                "readability": read["score"],
                "read_completeness": read.get("completeness"),
                "read_order": read.get("order"),
                "read_legibility": read.get("legibility"),
                "read_images": read.get("images"),
            }
        )
    ref_read = cmp_.get("ref_readability") or {}
    row["ref_readability"] = ref_read.get("score")
    return row


def _fmt(v) -> str:
    if v is None:
        return ""
    if isinstance(v, float):
        return f"{v:.3f}" if abs(v) < 2 else f"{v:.1f}"
    return str(v)


def _mean(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return round(statistics.fmean(xs), 3) if xs else None


def build(store: Path, data_root: Path, label: str, baseline: str | None, corpora: list[str] | None, limit: int = 0) -> Path:
    out_dir = store / "cmp" / label
    # The same documents the other steps took: the first `limit` of them.
    docs = list(common.iter_docs(data_root, corpora))
    if limit:
        docs = docs[:limit]
    rows = [_row(d, store, label) for d in docs]
    if baseline:
        base = {r["ident"]: r for r in (_row(d, store, baseline) for d in docs)}
        for r in rows:
            b = base.get(r["ident"], {})
            if isinstance(r.get("score"), (int, float)) and isinstance(b.get("score"), (int, float)):
                r["delta"] = round(r["score"] - b["score"], 1)
            if isinstance(r.get("readability"), (int, float)) and isinstance(b.get("readability"), (int, float)):
                r["readability_delta"] = round(r["readability"] - b["readability"], 1)

    by_corpus: dict[str, dict] = {}
    for c in common.corpora(data_root):
        rs = [r for r in rows if r["corpus"] == c.name]
        if not rs:
            continue
        ok = [r for r in rs if r["status"] == "ok"]
        scored = [r for r in rs if isinstance(r.get("score"), (int, float))]
        by_corpus[c.name] = {
            "documents": len(rs),
            "compared": len(ok),
            "wps_failed": sum(r["status"].startswith("wps-") for r in rs),
            "ours_failed": sum(r["status"].startswith("ours-") for r in rs),
            "cmp_failed": sum(r["status"].startswith("cmp-") for r in rs),
            "mean_readability": _mean([r.get("readability") for r in rs]),
            "mean_ref_readability": _mean([r.get("ref_readability") for r in rs]),
            "mean_readability_delta": _mean([r.get("readability_delta") for r in rs]),
            "mean_score": _mean([r["score"] for r in scored]),
            "median_score": round(statistics.median([r["score"] for r in scored]), 1) if scored else None,
            "page_count_exact": sum(r.get("pages_ref") == r.get("pages_ours") for r in ok),
            "mean_text_f1": _mean([r.get("text_f1") for r in ok]),
            "mean_same_page": _mean([r.get("same_page") for r in ok]),
            "mean_line_break_f1": _mean([r.get("line_break_f1") for r in ok]),
            "mean_geom_abs": _mean([r.get("geom_abs") for r in ok]),
            "mean_geom_advance": _mean([r.get("geom_advance") for r in ok]),
            "mean_geom_leading": _mean([r.get("geom_leading") for r in ok]),
            "mean_ssim": _mean([r.get("mean_ssim") for r in ok]),
            "grades": {g: sum(r.get("grade") == g for r in scored) for g in "ABCDF"},
            "mean_delta": _mean([r.get("delta") for r in rs]),
            "ref_fallback_docs": sum(_is_fallback(r) for r in rs),
            "mean_score_clean": _mean([r["score"] for r in scored if not _is_fallback(r)]),
        }
        imported = [r for r in rs if r.get("import_status")]
        if imported:
            by_corpus[c.name]["rtf_import"] = {
                status: sum(r["import_status"] == status for r in imported)
                for status in ("Clean", "Degraded", "Failed", "Timeout")
            }
            by_corpus[c.name]["rtf_import_losses"] = sum(r.get("import_losses") or 0 for r in imported)
    all_scored = [r for r in rows if isinstance(r.get("score"), (int, float))]
    total = {
        "documents": len(rows),
        "compared": sum(r["status"] == "ok" for r in rows),
        # The headline: how well the rendering carries the document to a
        # reader, scored against the source (see metrics.readability). The
        # WPS fidelity score below says how close the pages are to WPS's.
        "mean_readability": _mean([r.get("readability") for r in rows]),
        "mean_ref_readability": _mean([r.get("ref_readability") for r in rows]),
        "mean_readability_delta": _mean([r.get("readability_delta") for r in rows]),
        "mean_score": _mean([r["score"] for r in all_scored]),
        "grades": {g: sum(r.get("grade") == g for r in all_scored) for g in "ABCDF"},
        # References WPS drew mostly in its fallback face measure the host's
        # missing fonts, not our layout: apart, so they hide nothing.
        "clean": _group([r for r in rows if not _is_fallback(r)]),
        "ref_fallback": _group([r for r in rows if _is_fallback(r)]),
    }
    build_info = read_json(store / "ours" / label / "build.json") or {}
    summary = {
        "label": label,
        "baseline": baseline,
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "build": build_info,
        "total": total,
        "corpora": by_corpus,
    }
    write_json(out_dir / "summary.json", summary)
    with (out_dir / "results.csv").open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f)
        w.writerow([c for c, _ in COLUMNS] + ["error"])
        for r in rows:
            w.writerow([_fmt(r.get(c)) for c, _ in COLUMNS] + [r.get("error", "").replace("\n", " ")[:300]])
    path = out_dir / "report.html"
    path.write_text(_html(summary, rows), encoding="utf-8")
    return path


def _split_html(t: dict) -> str:
    import metrics

    e = html.escape
    lines = []
    for key, title in (("clean", "Без подмены шрифта в эталоне"), ("ref_fallback", "Эталон WPS в шрифте-подмене")):
        g = t.get(key)
        if not g:
            continue
        gr = g["grades"]
        lines.append(
            f"<tr><td>{e(title)}</td><td>{g['documents']}</td><td><b>{_fmt(g['mean_score'])}</b></td>"
            f"<td>{gr['A']}/{gr['B']}/{gr['C']}/{gr['D']}/{gr['F']}</td><td>{_fmt(g['mean_delta'])}</td></tr>"
        )
    fonts = ", ".join(metrics.WPS_FALLBACK_FONTS)
    return (
        f"<table><tr><th>группа</th><th>docs</th><th>mean</th><th>A/B/C/D/F</th><th>Δ</th></tr>{''.join(lines)}</table>"
        f"<p class='muted'>«Эталон в шрифте-подмене»: ≥{metrics.FALLBACK_SHARE:.0%} видимых символов эталона нарисованы {e(fonts)} — "
        "так WPS на этом хосте рисует любой отсутствующий шрифт; такой эталон меряет шрифты хоста, а не нашу раскладку.</p>"
    )


def _html(summary: dict, rows: list[dict]) -> str:
    e = html.escape
    corp_rows = []
    for name, s in summary["corpora"].items():
        g = s["grades"]
        imp = s.get("rtf_import")
        rtf = f"{imp['Clean']}/{imp['Degraded']}/{imp['Failed'] + imp['Timeout']} · {s['rtf_import_losses']}" if imp else "—"
        corp_rows.append(
            "<tr>"
            + f"<td>{e(name)}</td><td>{s['documents']}</td><td>{s['compared']}</td>"
            + f"<td>{s['wps_failed']}</td><td>{s['ours_failed']}</td><td>{s['cmp_failed']}</td>"
            + f"<td><b>{_fmt(s.get('mean_readability'))}</b></td><td>{_fmt(s.get('mean_ref_readability'))}</td>"
            + f"<td>{_fmt(s['mean_score'])}</td><td>{_fmt(s.get('mean_score_clean'))}</td><td>{s.get('ref_fallback_docs', 0)}</td><td>{_fmt(s['median_score'])}</td>"
            + f"<td>{s['page_count_exact']}/{s['compared']}</td>"
            + f"<td>{_fmt(s['mean_text_f1'])}</td><td>{_fmt(s['mean_same_page'])}</td>"
            + f"<td>{_fmt(s['mean_line_break_f1'])}</td><td>{_fmt(s['mean_geom_abs'])}</td><td>{_fmt(s['mean_geom_advance'])}</td><td>{_fmt(s['mean_geom_leading'])}</td><td>{_fmt(s['mean_ssim'])}</td>"
            + f"<td>{g['A']}/{g['B']}/{g['C']}/{g['D']}/{g['F']}</td><td>{_fmt(s['mean_delta'])}</td>"
            + f"<td>{e(rtf)}</td>"
            + "</tr>"
        )
    doc_rows = []
    # Worst readability first: that is where a reader is let down.
    ordered = sorted(
        rows,
        key=lambda r: (
            r.get("readability") if isinstance(r.get("readability"), (int, float)) else 1e9,
            r.get("score") if isinstance(r.get("score"), (int, float)) else 1e9,
            r["ident"],
        ),
    )
    for r in ordered:
        cells = []
        for col, _ in COLUMNS:
            v = r.get(col)
            if col == "key":
                inner = e(r["key"])
                if r.get("image"):
                    wp = r.get("worst_page") or {}
                    inner = f'<a href="{e(r["image"])}" target="_blank" title="worst page WPS {wp.get("ref_page")} / ours {wp.get("our_page")}">{inner}</a>'
                if r.get("error"):
                    inner += f'<div class="err">{e(r["error"][:200])}</div>'
                cells.append(f"<td class='doc'>{inner}</td>")
            elif col == "grade":
                cells.append(f"<td class='g{e(_fmt(v))}'>{e(_fmt(v))}</td>")
            else:
                cells.append(f"<td data-v='{e(_fmt(v))}'>{e(_fmt(v))}</td>")
        doc_rows.append("<tr>" + "".join(cells) + "</tr>")
    t = summary["total"]
    head = "".join(f"<th onclick='sortBy(this)'>{e(h)}</th>" for _, h in COLUMNS)
    build = summary.get("build", {})
    return f"""<!doctype html>
<html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>WPS QA {e(summary['label'])}</title>
<style>
:root {{ --bg:#fff; --fg:#1d1d1f; --muted:#666; --line:#ddd; --a:#2e7d32; --b:#7cb342; --c:#f9a825; --d:#e65100; --f:#b71c1c; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#141414; --fg:#e8e8e8; --muted:#999; --line:#333; }} }}
body {{ background:var(--bg); color:var(--fg); font:13px/1.4 system-ui, sans-serif; margin:16px; }}
table {{ border-collapse:collapse; margin:8px 0 24px; }} th,td {{ border-bottom:1px solid var(--line); padding:3px 7px; text-align:right; }}
th {{ cursor:pointer; position:sticky; top:0; background:var(--bg); }} td.doc {{ text-align:left; max-width:420px; overflow-wrap:anywhere; }}
td:first-child {{ text-align:left; }} .err {{ color:var(--muted); font-size:11px; }} .muted {{ color:var(--muted); }}
.gA {{ color:var(--a); font-weight:600 }} .gB {{ color:var(--b); font-weight:600 }} .gC {{ color:var(--c); font-weight:600 }} .gD {{ color:var(--d); font-weight:600 }} .gF {{ color:var(--f); font-weight:600 }}
</style></head><body>
<h1>WPS QA — {e(summary['label'])}</h1>
<p class="muted">generated {e(summary['generated'])} · CLI {e(str(build.get('cli', '')))} · baseline {e(str(summary.get('baseline') or '—'))}</p>
<p style="font-size:16px">Читаемость: <b>{_fmt(t.get('mean_readability'))}</b> (WPS: {_fmt(t.get('mean_ref_readability'))}{', Δ ' + _fmt(t.get('mean_readability_delta')) if t.get('mean_readability_delta') is not None else ''})</p>
<p class="muted">Читаемость 0–100 — главный показатель: насколько наш PDF доносит содержание документа читателю, сравнение с самим документом, а не с WPS. 40% полнота (доля слов документа на страницах, за вычетом символов, нарисованных квадратиком «нет глифа»), 25% порядок чтения (доля слов документа в правильном порядке), 20% разборчивость (слова не налезают друг на друга и не уходят за край страницы), 15% картинки (доля изображений документа на страницах). Тем же способом оценён PDF WPS.</p>
<p>Документов: <b>{t['documents']}</b>, сравнено: <b>{t['compared']}</b>, сходство с WPS: <b>{_fmt(t['mean_score'])}</b>,
оценки A/B/C/D/F: {t['grades']['A']}/{t['grades']['B']}/{t['grades']['C']}/{t['grades']['D']}/{t['grades']['F']}</p>
{_split_html(t)}
<p class="muted">Сходство с WPS 0–100 = 15% страницы + 25% текст (F1 слов) + 25% поток (та же страница, F1 переносов строк) + 20% геометрия (среднее: абсолютное положение слов на той же странице ≤4 pt, шаг между словами в строке и шаг строк — оба ≤ max(1 pt, 5%)) + 15% растр (SSIM 36 dpi, корреляция профилей). F = наш рендер не отработал.</p>
<h2>По корпусам</h2>
<table><tr><th>corpus</th><th>docs</th><th>compared</th><th>WPS fail</th><th>ours fail</th><th>cmp fail</th><th>readability</th><th>WPS readability</th><th>fidelity</th><th title="без эталонов в шрифте-подмене">fidelity clean</th><th title="эталонов в шрифте-подмене">fallback refs</th><th>median</th><th>pages =</th><th>text F1</th><th>same page</th><th>line breaks</th><th>abs ≤4pt</th><th>advance</th><th>leading</th><th>SSIM</th><th>A/B/C/D/F</th><th>Δ</th><th title="RTF import: Clean/Degraded/Failed · import losses">RTF C/D/F · losses</th></tr>
{''.join(corp_rows)}</table>
<h2>Документы (худшие сверху; ссылка — худшая страница: WPS | наш | наложение, красный = только WPS, синий = только наш)</h2>
<table id="docs"><thead><tr>{head}</tr></thead><tbody>
{''.join(doc_rows)}
</tbody></table>
<script>
function sortBy(th) {{
  const table = th.closest('table'), idx = [...th.parentNode.children].indexOf(th);
  const body = table.tBodies[0], rows = [...body.rows], dir = th.dataset.dir === 'asc' ? -1 : 1;
  th.dataset.dir = dir === 1 ? 'asc' : 'desc';
  const val = r => {{ const t = r.cells[idx].dataset.v ?? r.cells[idx].innerText; const n = parseFloat(t); return isNaN(n) ? t : n; }};
  rows.sort((a, b) => {{ const x = val(a), y = val(b); return (x > y ? 1 : x < y ? -1 : 0) * dir; }});
  rows.forEach(r => body.appendChild(r));
}}
</script>
</body></html>
"""
