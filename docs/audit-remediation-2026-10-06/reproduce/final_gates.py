import os, subprocess, json
from pathlib import Path
root=Path('.').resolve()
dest=root/'target/remediation-2026-10-06'
env=os.environ.copy(); env['RUSTUP_TOOLCHAIN']='1.92.0'
matrix=[]
base=['cargo','check','-p','strict-ooxml','--locked']
matrix.append(base+['--no-default-features'])
for feature in ['svg','pdf','write','convert']:
    matrix.append(base+['--no-default-features','--features',feature])
matrix += [base,base+['--all-features'],['cargo','run','-p','strict-ooxml','--locked','--no-default-features','--features','pdf','--example','render_pdf'],['cargo','check','-p','strict-ooxml-cli','--locked']]
for feature in [None,'save','visual']:
    args=['cargo','test','-p','strict-ooxml-edit','--locked','--no-default-features']
    if feature: args+=['--features',feature]
    matrix.append(args)
matrix.append(['cargo','test','-p','strict-ooxml','--locked','--no-default-features','--features','edit','--test','editing'])
groups=[('feature-matrix-accepted',matrix),('f21-accepted',[[os.sys.executable,'xtool/audit-fixes/run.py','--task','F21','--phase','green','--receipt-dir',str(dest/'f21-accepted')]]),('text-metric-accepted',[[os.sys.executable,'xtool/audit-fixes/text_metric.py']]),('element-coverage-accepted', [['cargo','run','-p','xtool','--locked','--','coverage','--file','coverage/wml-elements.toml','--min','89'],['cargo','run','-p','xtool','--locked','--','coverage','--file','coverage/stage5-scenarios.toml','--min','85']]),('lint-eof-accepted',[['cargo','run','-p','xtool','--locked','--','lint-eof']])]
for name,commands in groups:
    print('START',name,flush=True)
    results=[]
    with (dest/(name+'.log')).open('wb') as log:
        for args in commands:
            code=subprocess.call(args,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
            results.append({'command':args,'exit':code})
    code=0 if all(x['exit']==0 for x in results) else 1
    (dest/(name+'.commands.json')).write_text(json.dumps(results,indent=2))
    (dest/(name+'.exit')).write_text(str(code))
    print(name,'exit',code,flush=True)
