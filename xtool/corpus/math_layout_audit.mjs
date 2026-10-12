// Collect bounded evidence from the isolated viewer; never retain all SVG in memory.
import {readFileSync,writeFileSync,mkdirSync,appendFileSync} from 'node:fs';
const base=process.argv[2] || 'http://127.0.0.1:8742';
const root=process.argv[3] || 'render_out/math-layout-audit';
const selected=JSON.parse(readFileSync('render_out/math-layout-audit/selection.json','utf8'));
selected.push({name:'015_dataverse_Impact_of_Fault_Normal_Stiffness_on_Fric.docx'});
const results=[];
for(const doc of selected) {
  const prefix=doc.name.slice(0,3);
  const dir=root+'/ours/'+prefix;
  mkdirSync(dir,{recursive:true});
  const query='?corpus=cc0-docx-math&name='+encodeURIComponent(doc.name);
  const response=await fetch(base+'/api/document'+query+'&metadata=1',{signal:AbortSignal.timeout(45000)});
  if(!response.ok)throw new Error('metadata HTTP '+response.status);
  const view=await response.json();
  const warnings=view.pipeline.issues.filter(x=>x.stage==='render');
  writeFileSync(dir+'/metadata.json',JSON.stringify(view,null,2));
  const texts=warnings.filter(x=>x.id==='render.line-overflow').flatMap(x=> {
    try{return [JSON.parse(x.detail.match(/text ("(?:\\.|[^"\\])*"), family/)[1])];}
    catch{return [];}
  });
  const saved=[];
  const fontCounts={};
  const ledger=dir+'/text-pages.jsonl';
  writeFileSync(ledger,'');
  for(const page of view.pages) {
    const r=await fetch(base+'/api/page'+query+'&page='+page.number,{signal:AbortSignal.timeout(45000)});
    if(!r.ok)throw new Error('page HTTP '+r.status);
    const svg=await r.text();
    const segments=[...svg.matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)].map(m=>({attributes:m[1],text:m[2]}));
    for(const s of segments) {
      const family=s.attributes.match(/font-family="([^"]+)"/)?.[1] || 'unknown';
      fontCounts[family]=(fontCounts[family]||0)+s.text.length;
    }
    appendFileSync(ledger,JSON.stringify({page:page.number,width:page.width,height:page.height,segments})+'\n');
    if(page.number<=2 || page.number===view.pages.length || ['015','018','019','073','081'].includes(prefix) || texts.some(t=>t.trim().length>1 && segments.some(s=>s.text.includes(t.trim())))) {
      writeFileSync(dir+'/page-'+page.number+'.svg',svg);
      saved.push(page.number);
    }
  }
  const row={name:doc.name,pages:view.pages.length,warnings,saved_pages:saved,font_characters:fontCounts};
  results.push(row);
  writeFileSync(root+'/layout-diagnostics.json',JSON.stringify(results,null,2));
  console.log(JSON.stringify({name:doc.name,pages:row.pages,warnings:warnings.length,saved:saved.length}));
}
