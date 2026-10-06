import hashlib,json,zipfile
from pathlib import Path
from xml.etree import ElementTree as ET
root=Path('.').resolve(); dest=root/'docs/audit-remediation-2026-10-06'; dest.mkdir(parents=True,exist_ok=True)
source=root/'strict-ooxml-core/tests/docx/Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx'
fonts=set(); parts={}
with zipfile.ZipFile(source) as archive:
    for name in archive.namelist():
        if name.startswith('word/') and name.endswith('.xml'):
            raw=archive.read(name); parts[name]=hashlib.sha256(raw).hexdigest()
            for elem in ET.fromstring(raw).iter():
                if elem.tag.rsplit('}',1)[-1]=='rFonts':
                    fonts.update(value for key,value in elem.attrib.items() if key.rsplit('}',1)[-1] in ['ascii','hAnsi','eastAsia','cs'])
payload={'status':'NOT_RUN_NO_WORD_REFERENCE','matrix_row':'F16-complex-word-schemes','source':str(source.relative_to(root)).replace('\\','/'),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'source_bytes':source.stat().st_size,'pages':[54,56,104],'source_xml_parts_sha256':parts,'declared_font_families':sorted(fonts),'reference_requirements':{'application':'Microsoft Word','version_build':None,'host_os':None,'export_method':None,'export_settings':None,'font_file_versions_and_sha256':None,'source_sha256_must_equal':hashlib.sha256(source.read_bytes()).hexdigest(),'pdf_sha256':None,'page_dimensions_pt':None,'page_mapping':'physical document pages 54,56,104; record PDF page indices and displayed page numbers separately','page_selection':'prefer full-document PDF; selected-page export must retain document page identity','scale':'unscaled page coordinates; pt to px uses 96/72','reference_geometry':'absolute glyph baselines/origins and component topology; bbox top is not baseline'},'acceptance':{'complete_ledger_required':True,'tolerance_px':0.25,'fonts_must_be_identified':True,'missing_components_allowed':0,'ambiguous_matches':'UNMEASURABLE, not PASS','word_gate_cannot_be_substituted_by':['WPS','LibreOffice','unproven screenshot']},'after_reference':['Verify matching DOCX byte hash and export provenance.','Map pages by content plus physical page identity.','Measure every required component against the frozen implementation.','Record new differences as Word-specific failures; preserve previous WPS receipts.']}
(dest/'word-export-request.json').write_text(json.dumps(payload,ensure_ascii=False,indent=2),encoding='utf-8')
print('prepared Word request:',len(parts),'XML part hashes,',len(fonts),'declared font families')
