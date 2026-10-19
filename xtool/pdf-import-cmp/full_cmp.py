import sys, json, statistics, collections
sys.stdout.reconfigure(encoding="utf-8")
base = {r["key"]: r for r in json.load(open(sys.argv[1], encoding="utf8"))}
head = {r["key"]: r for r in json.load(open(sys.argv[2], encoding="utf8"))}
keys = sorted(set(base) | set(head))
def ok(r): return r and r.get("rc") in (0, 1) and "error" not in r
print("docs", len(keys))
for tag, d in (("base", base), ("head", head)):
    fails = [k for k in keys if not ok(d.get(k))]
    print(f"{tag}: converted {len(keys)-len(fails)}, failed/timeouts {len(fails)}", [ (k, d.get(k,{}).get('error')) for k in fails][:6])
both = [k for k in keys if ok(base.get(k)) and ok(head.get(k)) and "recall" in base[k] and "recall" in head[k]]
print("with text layer in both:", len(both))
for m in ("recall", "extra", "order"):
    b = [base[k][m] for k in both]; h = [head[k][m] for k in both]
    print(f"  {m:7} mean {statistics.mean(b):.4f} -> {statistics.mean(h):.4f}   median {statistics.median(b):.4f} -> {statistics.median(h):.4f}")
for m in ("paras", "avg_chars", "anchors", "lnto", "grad", "tables", "headers", "leaders", "kb", "seconds"):
    b = sum(base[k].get(m, 0) for k in keys if ok(base.get(k)) and ok(head.get(k)))
    h = sum(head[k].get(m, 0) for k in keys if ok(base.get(k)) and ok(head.get(k)))
    print(f"  total {m:9} {b:>10.1f} -> {h:>10.1f}")
def delta(m, thr):
    rows = [(head[k][m] - base[k][m], k) for k in both]
    return sorted([r for r in rows if r[0] > thr], reverse=True), sorted([r for r in rows if r[0] < -thr])
for m, thr in (("recall", 0.01), ("order", 0.02), ("extra", 0.01)):
    up, down = delta(m, thr)
    print(f"{m}: up {len(up)}, down {len(down)}")
    for dv, k in up[:8]: print(f"   + {dv:+.3f} {k}  {base[k][m]:.3f}->{head[k][m]:.3f}")
    for dv, k in down[:8]: print(f"   - {dv:+.3f} {k}  {base[k][m]:.3f}->{head[k][m]:.3f}")
ids_b, ids_h = collections.Counter(), collections.Counter()
for k in keys:
    if ok(base.get(k)): ids_b.update(base[k].get("ids", {}))
    if ok(head.get(k)): ids_h.update(head[k].get("ids", {}))
print("report ids (base -> head):")
for i in sorted(set(ids_b) | set(ids_h), key=lambda i: -(ids_b[i] + ids_h[i])):
    print(f"  {i:32} {ids_b[i]:>6} -> {ids_h[i]:>6}")
