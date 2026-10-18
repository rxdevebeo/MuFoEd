"""Export corpus audit receipts, per-case CSV and diagnostic frequencies."""
import argparse
import csv
import hashlib
import html
import json
import shutil
from collections import Counter
from pathlib import Path


def main():
    p=argparse.ArgumentParser()
    p.add_argument("results",type=Path)
    p.add_argument("report",type=Path)
    args=p.parse_args()
    args.report.mkdir(parents=True,exist_ok=True)
    results=json.loads((args.results/"summary.json").read_text(encoding="utf-8"))
    fields=["id","stage","exit_code","timeout","loss_count","info_count","included_files","paragraphs","math_expressions","tables","drawings","pages","overflow_examples","warnings","wall_seconds","input_sha256"]
    rows=[]
    loss,info,affected=Counter(),Counter(),Counter()
    for r in results:
        i,m,v=r.get("import",{}),r.get("model",{}),r.get("render",{})
        rows.append(dict(zip(fields,[r["case"]["id"],r["stage"],r["exit_code"],r["timeout"],i.get("loss_count"),i.get("info_count"),i.get("included_files"),m.get("paragraphs"),m.get("math_expressions"),m.get("tables"),m.get("drawings"),v.get("pages"),len(v.get("overflow_text",[])),len(v.get("warnings",[])),r["wall_seconds"],r["case"]["sha256"]])))
        for name,counts in i.get("features",{}).items():
            loss[name]+=counts["loss"]
            info[name]+=counts["info"]
            affected[name]+=int(counts["loss"]>0)
    with (args.report/"cases.csv").open("w",newline="",encoding="utf-8-sig")as f:
        w=csv.DictWriter(f,fieldnames=fields);w.writeheader();w.writerows(rows)
    with (args.report/"features.csv").open("w",newline="",encoding="utf-8-sig")as f:
        w=csv.writer(f);w.writerow(["feature","loss_events","info_events","cases_with_loss"])
        for name in sorted(loss,key=lambda k:(-loss[k],k)):
            w.writerow([name,loss[name],info[name],affected[name]])
    totals={"cases":len(results),"stages":dict(Counter(r["stage"]for r in results)),"timeouts":sum(r["timeout"]for r in results),"cases_with_import_losses":sum(r.get("import",{}).get("loss_count",0)>0 for r in results),"loss_events":sum(r.get("import",{}).get("loss_count",0)for r in results),"info_events":sum(r.get("import",{}).get("info_count",0)for r in results),"pages_placed":sum(r.get("render",{}).get("pages",0)for r in results),"math_expressions_in_main_document":sum(r.get("model",{}).get("math_expressions",0)for r in results),"cases_with_page_bounds_candidates":sum(bool(r.get("render",{}).get("overflow_text"))for r in results),"issues_detail_cap_reached":sum(r.get("import",{}).get("details_truncated",False)for r in results),"summary_sha256":hashlib.sha256((args.results/"summary.json").read_bytes()).hexdigest()}
    (args.report/"totals.json").write_text(json.dumps(totals,ensure_ascii=False,indent=2),encoding="utf-8")
    shutil.copyfile(args.results/"inventory.json",args.report/"inventory.json")
    links=[]
    for r in results:
        id=r["case"]["id"]
        imgs="".join(f'<a href="{id}/page-{n}.png"><img loading="lazy" width="220" src="{id}/page-{n}.png" alt="page {n}"></a>' for n in range(1,min(3,r.get("render",{}).get("pages",0))+1))
        links.append(f'<article><h2>{html.escape(id)}</h2><p>{html.escape(r["case"]["title"])}</p><p>stage: {r["stage"]}; losses: {r.get("import",{}).get("loss_count")}; pages: {r.get("render",{}).get("pages")}</p><p><a href="{id}/document.docx">DOCX</a> | <a href="{id}/text.txt">Text</a> | <a href="{id}/math.txt">Math text</a> | <a href="{id}/result.json">JSON</a></p>{imgs}</article>')
    (args.results/"index.html").write_text('<!doctype html><meta charset="utf-8"><title>CC0 TEX audit</title><style>body{font:15px system-ui;background:#eee;margin:30px}article{background:white;padding:20px;margin:15px 0}img{border:1px solid #aaa;margin:5px}h2{font-size:18px}</style><h1>CC0 TEX audit: smoke results, not fidelity acceptance</h1>'+"\n".join(links),encoding="utf-8")
    print(json.dumps(totals,ensure_ascii=False,indent=2))


if __name__ == "__main__":main()
