"""Bounded acceptance probe; all 100 manifest inputs are measured, never skipped."""
from pathlib import Path
import concurrent.futures
import hashlib
import json
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'target/cc0-acceptance-probe'
CORPUS = ROOT / 'testdata/CC0_DOCX'
CLI = ROOT / 'target/release/strict-ooxml.exe'
OUT.mkdir(exist_ok=True)
for folder in ('written', 'rewrite', 'logs', 'svg'):
    (OUT / folder).mkdir(exist_ok=True)
manifest = json.loads((CORPUS / 'manifest.json').read_text(encoding='utf-8-sig'))

def invoke(args, log, timeout=90):
    started = time.monotonic()
    with log.open('wb') as handle:
        try:
            process = subprocess.run([str(CLI), *map(str, args)], cwd=ROOT, stdout=handle, stderr=subprocess.STDOUT, timeout=timeout)
            code = process.returncode
        except subprocess.TimeoutExpired:
            code = 'TIMEOUT'
    return {'exit': code, 'seconds': round(time.monotonic()-started, 3)}

def parts(path):
    with zipfile.ZipFile(path) as archive:
        return {name: hashlib.sha256(archive.read(name)).hexdigest() for name in archive.namelist()}

def probe(entry):
    name = entry['filename']
    source = CORPUS / name
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    result = {'filename': name, 'input_sha256': digest, 'manifest_hash_match': digest == entry['sha256'], 'size_match': source.stat().st_size == entry['byte_size']}
    if not result['manifest_hash_match'] or not result['size_match']:
        return result
    written = OUT / 'written' / name
    rewritten = OUT / 'rewrite' / name
    written.unlink(missing_ok=True)
    rewritten.unlink(missing_ok=True)
    result['write'] = invoke(['write', source, '--transitional', '--out', written, '--report-out', OUT/'logs'/f'{name}.pipeline.json'], OUT/'logs'/f'{name}.write.log')
    if written.exists() and result['write']['exit'] in (0, 1):
        result['reopen'] = invoke(['check', written], OUT/'logs'/f'{name}.check.log')
        result['rewrite'] = invoke(['write', written, '--out', rewritten], OUT/'logs'/f'{name}.rewrite.log')
        if rewritten.exists() and result['rewrite']['exit'] in (0, 1):
            first, second = parts(written), parts(rewritten)
            result['fixed_point'] = first == second
            result['changed_parts'] = sorted(key for key in first.keys() | second.keys() if first.get(key) != second.get(key))
        result['render_first_page'] = invoke(['render', written, '--pages', '1', '--out', OUT/'svg'/source.stem], OUT/'logs'/f'{name}.render.log')
    return result

rows=[]
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    for row in pool.map(probe, manifest):
        rows.append(row)
        (OUT/'results.json').write_text(json.dumps({'manifest_sha256': hashlib.sha256((CORPUS/'manifest.json').read_bytes()).hexdigest(), 'rows': rows}, ensure_ascii=False, indent=2), encoding='utf-8')
        if len(rows)%10 == 0:
            print(f'CC0 {len(rows)}/{len(manifest)}', flush=True)
assert len(rows) == len(manifest) == 100
summary = {'documents': len(rows), 'hash_matches': sum(r['manifest_hash_match'] for r in rows), 'size_matches': sum(r['size_match'] for r in rows)}
for step in ('write','reopen','rewrite','render_first_page'):
    values = [r.get(step,{}).get('exit','NOT_RUN') for r in rows]
    summary[step] = {str(value): values.count(value) for value in set(values)}
summary['fixed_point'] = {'pass': sum(r.get('fixed_point') is True for r in rows), 'fail': sum(r.get('fixed_point') is False for r in rows), 'not_run': sum('fixed_point' not in r for r in rows)}
(OUT/'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2),encoding='utf-8')
print(json.dumps(summary,ensure_ascii=False),flush=True)
