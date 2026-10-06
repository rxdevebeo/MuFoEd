import os,subprocess
from pathlib import Path
root=Path('/mnt/d/projects/StrictLib')
dest=root/'target/remediation-2026-10-06'
env=os.environ.copy()
env.update(CARGO_TARGET_DIR='target/llvm-cov-target',CARGO_PROFILE_DEV_CODEGEN_UNITS='1')
for suffix,floor in [('write',80),('pdf',75),('render-pdf',75)]:
    args=['cargo','+nightly','llvm-cov','-p','strict-ooxml-'+suffix,'--all-features','--locked','--ignore-filename-regex',r'(/\.cargo/registry|/\.cargo/git|/rustc-|/rustlib/|/build/|/deps/)','--fail-under-lines',str(floor),'--json','--summary-only','--output-path',str(dest/(suffix+'-coverage.json'))]
    print('START '+suffix,flush=True)
    with (dest/(suffix+'-coverage.log')).open('wb') as output:
        code=subprocess.call(args,cwd=root,env=env,stdout=output,stderr=subprocess.STDOUT)
    (dest/(suffix+'-coverage.exit')).write_text(str(code))
    print(suffix+' exit='+str(code),flush=True)
