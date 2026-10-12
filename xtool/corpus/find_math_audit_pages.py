import json, re
from pathlib import Path
import pdfplumber
root=Path('render_out/math-layout-audit')
for prefix, text in [('007','Source'),('081','Compounds')]:
    ledger=root/'ours'/prefix/'text-pages.jsonl'
    with ledger.open(encoding='utf8') as f:
        for line in f:
            page=json.loads(line)
            matches=[s for s in page['segments'] if text in s['text'] or
                (prefix=='081' and s['text']=='C' and '167.433' in s['attributes'])]
            if matches: print(prefix,'ours',page['page'],matches[:2])
    reference=next((root/'reference').glob(prefix+'_*.pdf'))
    with pdfplumber.open(reference) as pdf:
        for i,page in enumerate(pdf.pages):
            if text in (page.extract_text() or ''): print(prefix,'WPS',i+1)

page=json.loads((root/'ours/019/text-pages.jsonl').open(encoding='utf8').readline())
print('019 body fragments', [s for s in page['segments'] if any(word in s['text'] for word in ['with','chlor','embed','degra'])][:20])
