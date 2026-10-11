"""Summarize a bounded viewer scan and its worker memory log without reading SVG."""
import collections
import json
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else "render_out/math-layout-fixes-verified")


def records(path):
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            yield json.loads(line)


rows = [r for r in records(root / "math-scan.jsonl") if r["event"] == "scan.end"]
assert len(rows) == 100 and len({r["name"] for r in rows}) == 100, "incomplete or duplicate scan"
counts = collections.Counter()
documents = collections.defaultdict(set)
for row in rows:
    for issue in row.get("issues", []):
        if issue["stage"] == "render":
            counts[issue["id"]] += issue.get("count", 1)
            documents[issue["id"]].add(row["name"][:3])
workers = [r for r in records(root / "viewer-metrics.jsonl") if r["event"] == "render.end"]
peak = max(workers, key=lambda r: r.get("memory", {}).get("peak_rss_bytes", 0))
result = {
    "documents": len(rows),
    "pages": sum(r["pages"] for r in rows),
    "outcomes": dict(collections.Counter(r["outcome"] for r in rows)),
    "notes": [{"name": r["name"], "note": r["note"]} for r in rows if r.get("note")],
    "render_issue_counts": dict(counts),
    "render_issue_documents": {k: sorted(v) for k, v in documents.items()},
    "peak_worker_rss_mib": round(peak["memory"]["peak_rss_bytes"] / 1024**2, 1),
    "peak_worker_document": peak["document"],
    "peak_scanner_rss_mib": round(max(r["scanner_rss_bytes"] for r in rows) / 1024**2, 1),
}
(root / "scan-summary.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(result, ensure_ascii=False, indent=2))
