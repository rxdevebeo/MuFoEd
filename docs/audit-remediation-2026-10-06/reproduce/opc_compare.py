"""Compare source and output OPC diagnostics with multiplicity preserved."""
import collections,json,sys
from pathlib import Path
sys.path.insert(0,'xtool/xsd-gate')
import opc_gate as gate
root=Path('.').resolve()
dest=root/'target/remediation-2026-10-06'
config=gate.load_config()
schemas=gate.compile_schemas(gate.resolve_schema_dir(config))
groups=[('docx',root/'strict-ooxml-core/tests/docx'),('samples',root/'strict-ooxml-core/tests/samples'),('cc0',root/'testdata/CC0_DOCX')]
report={'documents':0,'missing':[],'source_violations':0,'output_violations':0,'introduced_violations':0,'documents_with_violations':[]}
for group,source in groups:
    written=dest/'census-writer-final-written'/group
    for output in sorted(written.glob('*.docx')):
        candidates=list(source.rglob(output.name))
        if len(candidates)!=1:
            report['missing'].append(str(output)); continue
        a=collections.Counter(gate.validate_package(str(candidates[0]),schemas))
        b=collections.Counter(gate.validate_package(str(output),schemas))
        # Paths before :: are diagnostics labels, not part of the violation.
        a=collections.Counter({k.split('::',1)[-1]:v for k,v in a.items()})
        b=collections.Counter({k.split('::',1)[-1]:v for k,v in b.items()})
        new=b-a
        report['documents']+=1
        report['source_violations']+=sum(a.values())
        report['output_violations']+=sum(b.values())
        report['introduced_violations']+=sum(new.values())
        if a or b: report['documents_with_violations'].append({'document':output.name,'source':dict(a),'output':dict(b),'introduced':dict(new)})
report['status']='PASS_NO_INTRODUCED_VIOLATIONS' if not report['missing'] and not report['introduced_violations'] and report['documents']==221 else 'FAIL'
(dest/'opc-comparison.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print({k:v for k,v in report.items() if k!='documents_with_violations'})
