"""Summarize the completed bounded scan without loading SVG pages."""
import collections, json
from pathlib import Path
ROOT=Path('render_out/math-layout-audit')
rows=[]
with (ROOT/'math-scan-final.jsonl').open(encoding='utf8') as f:
    for line in f:
        value=json.loads(line)
        if value['event']=='scan.end': rows.append(value)
counts=collections.Counter()
docs=collections.defaultdict(list)
for row in rows:
    for issue in row.get('issues',[]):
        if issue['id'].startswith('render.'):
            counts[issue['id']]+=issue.get('count',1)
            if row['name'] not in docs[issue['id']]: docs[issue['id']].append(row['name'])
result={'documents':len(rows),'pages':sum(r['pages'] for r in rows),
        'outcomes':dict(collections.Counter(r['outcome'] for r in rows)),
        'render_issue_counts':dict(counts),'render_issue_documents':dict(docs)}
(ROOT/'scan-summary.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps({k:v for k,v in result.items() if k!='render_issue_documents'}))
for row in rows:
    if any(i['id'] in ['render.glyph-overflow','render.line-overflow'] for i in row.get('issues',[])):
        print(row['name'][:3], [(i['id'],i['detail']) for i in row['issues'] if i['id'] in ['render.glyph-overflow','render.line-overflow']][:3])
comparison=json.loads((ROOT/'reference-comparison.json').read_text(encoding='utf8'))
print('Page counts:',[(r['name'][:3],r['ours_pages'],r['wps_pages']) for r in comparison['selected']])
