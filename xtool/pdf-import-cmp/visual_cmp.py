"""Summarise a full_visual.py result: mean ink/SSIM per corpus, and the largest
changes either way between two builds.

    python visual_cmp.py <visual.json> [base-label] [new-label]
"""
import json
import statistics
import sys

sys.stdout.reconfigure(encoding="utf-8")
v = json.load(open(sys.argv[1], encoding="utf8"))
old = sys.argv[2] if len(sys.argv) > 2 else "base"
new = sys.argv[3] if len(sys.argv) > 3 else "head"
both = {k: r for k, r in v.items() if r.get(old) and r.get(new)}
failed = [k for k, r in v.items() if not (r.get(old) and r.get(new))]
print("docs measured:", len(both), "of", len(v), "| export failed:", failed[:8])
for group in ("pdf__", "CC0_PDF__", ""):
    ks = [k for k in both if k.startswith(group)]
    if not ks:
        continue
    for m in ("ink", "ssim"):
        b = [both[k][old][m] for k in ks]
        h = [both[k][new][m] for k in ks]
        print(f"{group or 'all':10} {m:5} mean {statistics.mean(b):.4f} -> {statistics.mean(h):.4f}  (n={len(ks)})")
deltas = [(r[new]["ink"] - r[old]["ink"], k) for k, r in both.items()]
up = sorted((d for d in deltas if d[0] > 0.02), reverse=True)
down = sorted(d for d in deltas if d[0] < -0.02)  # most negative first
print(f"ink up >0.02: {len(up)}, down <-0.02: {len(down)}, unchanged: {len(deltas) - len(up) - len(down)}")


def line(d, k):
    a, b = both[k][old], both[k][new]
    return (f"  {d:+.3f} {k}  {a['ink']:.3f}->{b['ink']:.3f}  "
            f"pages src/{old}/{new} {a['src_pages']}/{a['pages']}/{b['pages']}")


for d, k in up[:12]:
    print(line(d, k))
print("  ...")
for d, k in down:
    print(line(d, k))
