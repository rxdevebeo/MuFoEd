import hashlib,json,subprocess
from pathlib import Path
root=Path('.').resolve()
paths=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z']).decode().split('\0')
files={}
for name in sorted(set(paths)):
    p=root/name
    if not p.is_file() or name.startswith('docs/'):
        continue
    if p.suffix in {'.rs','.py','.toml','.yml','.yaml'} or p.name=='Cargo.lock':
        files[name]=hashlib.sha256(p.read_bytes()).hexdigest()
encoded=json.dumps(files,sort_keys=True,separators=(',',':')).encode()
rust={k:v for k,v in files.items() if Path(k).suffix in {'.rs','.toml'} or Path(k).name=='Cargo.lock'}
report={'head':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'content_sha256':hashlib.sha256(encoded).hexdigest(),'rust_input_sha256':hashlib.sha256(json.dumps(rust,sort_keys=True,separators=(',',':')).encode()).hexdigest(),'algorithm':'sha256 of sorted JSON path-to-byte-sha256 mapping; docs excluded; includes untracked code','files':files}
out=root/'target/remediation-2026-10-06/source-manifest.json'
out.write_text(json.dumps(report,indent=2),encoding='utf-8')
print(report['head'],report['content_sha256'],len(files))
