"""Run manifest entry points in isolated processes; keep timeouts and failures.

python tools/latex-audit/run.py CORPUS OUTPUT --exe target/latex-audit-build/debug/latex-audit.exe
"""
import argparse
import concurrent.futures
import hashlib
import json
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
import zipfile
from collections import Counter
from pathlib import Path

W = "{http://purl.oclc.org/ooxml/wordprocessingml/main}"
M = "{http://purl.oclc.org/ooxml/officeDocument/math}"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def extract(out):
    package = out / "document.docx"
    if not package.exists():
        return {}
    with zipfile.ZipFile(package) as z:
        root = ET.fromstring(z.read("word/document.xml"))
        paragraphs = ["".join(t.text or "" for t in p.iter(W + "t")) for p in root.iter(W + "p")]
        math = ["".join(t.text or "" for t in p.iter(M + "t")) for p in root.iter(M + "oMath")]
        counts = Counter(n.tag[len(M):] for n in root.iter() if n.tag.startswith(M))
        (out / "text.txt").write_text("\n".join(paragraphs), encoding="utf-8")
        (out / "math.txt").write_text("\n".join(f"{i+1}: {t}" for i, t in enumerate(math)), encoding="utf-8")
        (out / "document.xml").write_bytes(z.read("word/document.xml"))
    return {"paragraphs":len(paragraphs),"plain_text_chars":sum(map(len,paragraphs)),"math_expressions":len(math),"math_node_counts":dict(counts),"tables":sum(1 for _ in root.iter(W+"tbl")),"drawings":sum(1 for _ in root.iter(W+"drawing")),"backslash_paragraphs":[{"paragraph":i+1,"text":t[:500]} for i,t in enumerate(paragraphs) if re.search(r"\\[A-Za-z@]+|#[1-9]|(?:width|height|linewidth|textwidth)\s*=",t)][:50]}


def run_one(case, args):
    out = args.output / case["id"]
    out.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    try:
        p = subprocess.run([str(args.exe),case["path"],str(out),str(args.pages)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=args.timeout)
        execution = {"exit_code":p.returncode,"timeout":False}
        (out/"stdout.txt").write_bytes(p.stdout)
        (out/"stderr.txt").write_bytes(p.stderr)
    except subprocess.TimeoutExpired as e:
        execution = {"exit_code":None,"timeout":True}
        (out/"stderr.txt").write_bytes(e.stderr or b"")
    result_path = out/"result.json"
    result = json.loads(result_path.read_text(encoding="utf-8")) if result_path.exists() else {"stage":"process_start"}
    result.update(execution)
    result["wall_seconds"] = round(time.monotonic()-start,3)
    result["case"] = case
    result["model"] = extract(out)
    result_path.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding="utf-8")
    print(case["id"],result["stage"], result.get("error",""),flush=True)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("corpus",type=Path)
    parser.add_argument("output",type=Path)
    parser.add_argument("--exe",type=Path,required=True)
    parser.add_argument("--pages",type=int,default=3)
    parser.add_argument("--timeout",type=int,default=120)
    parser.add_argument("--workers",type=int,default=2)
    args=parser.parse_args()
    args.exe=args.exe.resolve()
    args.output=args.output.resolve()
    args.output.mkdir(parents=True,exist_ok=True)
    if any(args.output.iterdir()):
        raise SystemExit("Choose an empty output directory; old artifacts must not be mistaken for new results.")
    if hasattr(sys.stdout,"reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    cases=[]
    manifests=[]
    for manifest in sorted(args.corpus.glob("*/manifest.json")):
        manifests.append({"path":str(manifest.resolve()),"sha256":sha(manifest)})
        for entry in json.loads(manifest.read_text(encoding="utf-8")):
            path=manifest.parent/entry["project_dir"]/entry["main_tex_in_project"]
            cases.append({"id":manifest.parent.name+"-"+Path(entry["project_dir"]).name,"path":str(path.resolve()),"title":entry["title"],"identifier":entry["identifier"],"sha256":sha(path) if path.exists() else None,"manifest_sha256":entry.get("sha256"),"main_exists":path.exists(),"project_dir":str((manifest.parent/entry["project_dir"]).resolve()),"engine_hint":entry.get("engine_hint"),"compile_status":entry.get("compile_status")})
    inventory={"corpus":str(args.corpus.resolve()),"manifests":manifests,"cases":cases,"git_head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"git_status":subprocess.check_output(["git","status","--short"],text=True),"pages_limit":args.pages,"exe_sha256":sha(args.exe),"tex_sources":[{"path":str(p.relative_to(args.corpus)),"sha256":sha(p),"size":p.stat().st_size} for p in sorted(args.corpus.rglob("*.tex"))]}
    (args.output/"inventory.json").write_text(json.dumps(inventory,ensure_ascii=False,indent=2),encoding="utf-8")
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        results=list(pool.map(lambda c:run_one(c,args),cases))
    (args.output/"summary.json").write_text(json.dumps(results,ensure_ascii=False,indent=2),encoding="utf-8")


if __name__ == "__main__":
    main()
