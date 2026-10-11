// Sequential, bounded metadata scan. Persist each result before opening the next file.
import { mkdirSync, openSync, writeSync, fsyncSync, closeSync } from 'node:fs';
import { dirname } from 'node:path';
const args = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, i, all) => {
  if (value.startsWith('--')) pairs.push([value.slice(2), all[i + 1]]);
  return pairs;
}, []));
const base = args.url || 'http://127.0.0.1:8741';
const corpus = args.corpus || 'cc0-docx-math';
const report = args.report || 'render_out/graphics-gallery/math-scan.jsonl';
mkdirSync(dirname(report), {recursive:true});
const fd = openSync(report, 'a');
function record(value) {
  writeSync(fd, JSON.stringify({time:new Date().toISOString(),...value}) + '\n');
  fsyncSync(fd);
}
async function json(url) {
  const response = await fetch(url, {signal:AbortSignal.timeout(45000)});
  if (!response.ok) throw new Error('HTTP ' + response.status);
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > 16 * 1024 * 1024) {
      throw new Error('metadata response exceeded 16 MiB');
    }
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
let count = 0;
const outcomes = {};
try {
  const documents = await json(base + '/api/documents?corpus=' + encodeURIComponent(corpus));
  for (const document of documents) {
    record({event:'scan.begin',corpus,name:document.name});
    const start = Date.now();
    let result;
    try {
      const view = await json(base + '/api/document?metadata=1&corpus=' + encodeURIComponent(corpus) + '&name=' + encodeURIComponent(document.name));
      result = {outcome:view.pipeline.outcome,pages:view.pages.length,note:view.note,
        issues:view.pipeline.issues,stages:view.pipeline.stages};
    } catch (error) { result = {outcome:'request_error',pages:0,note:String(error)}; }
    const memory = process.memoryUsage();
    record({event:'scan.end',corpus,name:document.name,...result,elapsed_ms:Date.now()-start,scanner_rss_bytes:memory.rss});
    outcomes[result.outcome] = (outcomes[result.outcome] || 0) + 1;
    count++;
    if (count % 10 === 0) console.log(JSON.stringify({count,outcomes,scanner_rss_bytes:memory.rss}));
  }
  record({event:'scan.complete',corpus,count,outcomes});
  console.log(JSON.stringify({complete:true,count,outcomes,report}));
} finally { closeSync(fd); }
