"""Summarize synchronized wall-clock spans; nested spans are never added to root totals."""
import argparse
import json
from collections import defaultdict
from pathlib import Path
from statistics import mean


def seconds(items, stage):
    return sum(item["elapsed_ms"] for item in items if item["stage"] == stage) / 1000


def summarize(profile, receipt):
    events = [json.loads(line) for line in profile.read_text(encoding="utf-8").splitlines() if line.strip()]
    if any(event.get("sync_error") for event in events):
        raise ValueError("GPU synchronization failure in timing log")
    # Worker acknowledgement/timer drops precede pipeline completion. Completion
    # order avoids Windows wall-clock quantization when mixing QPC durations with epoch time.
    chunks = []
    current = []
    for event in events:
        current.append(event)
        if event["stage"] == "pipeline.total":
            chunks.append(current)
            current = []
    if current or len(chunks) != len(receipt["runs"]):
        raise ValueError("Incomplete log or run receipt")
    rows = []
    details = []
    for children, run in zip(chunks, receipt["runs"]):
        row = {"run": run["run"], "wall_s": run["elapsed_s"]}
        for name in ["pipeline.first", "pipeline.ultrasharp", "pipeline.refine", "pipeline.total"]:
            selected = [e for e in children if e["stage"] == name]
            if len(selected) != 1:
                raise ValueError(f"Expected one {name} in {run['run']}")
            row[name] = selected[0]["elapsed_ms"] / 1000
        row["other_s"] = row["wall_s"] - sum(row[n] for n in ["pipeline.first", "pipeline.ultrasharp", "pipeline.refine"])
        rows.append(row)
        native = sorted((e for e in children if e["stage"].startswith("native.") and ("TextToImage" in e["stage"] or "ImageToImage" in e["stage"])), key=lambda e: e["start_ms"])
        if len(native) != 2:
            raise ValueError("Expected two native inference spans")
        previous = 0
        for scope in native:
            stop = children.index(scope)
            selected = children[previous:stop + 1]
            previous = stop + 1
            groups = defaultdict(list)
            for e in selected:
                groups[e["stage"]].append(e["elapsed_ms"] / 1000)
            details.append({"run": run["run"], "pass": scope["stage"], "total_s": scope["elapsed_ms"] / 1000,
                            "stages": {key: {"total_s":sum(value),"count":len(value),"mean_s":mean(value)} for key,value in sorted(groups.items())}})
        python = [e for e in children if e["source"] == "python"]
        row["ultrasharp_prepare_s"] = seconds(python, "ultrasharp.model_prepare")
        row["ultrasharp_forward_s"] = seconds(python, "ultrasharp.forward_and_transfer")
        row["ultrasharp_resample_s"] = seconds(python, "ultrasharp.cpu_resample")
        row["ultrasharp_other_s"] = row["pipeline.ultrasharp"] - sum(row[n] for n in ["ultrasharp_prepare_s","ultrasharp_forward_s","ultrasharp_resample_s"])
    return {"runs":rows,"native_details":details,"event_count":len(events)}


def markdown(summary):
    runs = summary["runs"]
    lines = ["# Замеры генерации", "", "Все значения — секунды. Вложенные таймеры не суммируются с родительскими.", "",
             "| Этап | " + " | ".join(r["run"] for r in runs) + " |", "|---|" + "---:|" * len(runs)]
    for label, key in [("Первый проход", "pipeline.first"),("UltraSharp", "pipeline.ultrasharp"),("Img2Img", "pipeline.refine"),("Прочие расходы", "other_s"),("Весь цикл", "wall_s"),
                       ("UltraSharp: подготовка", "ultrasharp_prepare_s"),("UltraSharp: сеть и передачи", "ultrasharp_forward_s"),("UltraSharp: CPU ресайз", "ultrasharp_resample_s"),("UltraSharp: запуск/протокол/PNG и остальное", "ultrasharp_other_s")]:
        lines.append("| " + label + " | " + " | ".join(f"{r[key]:.3f}" for r in runs) + " |")
    for mode in ["TextToImage", "ImageToImage"]:
        selected = [d for d in summary["native_details"] if mode in d["pass"]]
        lines += ["", "## " + mode, "", "| Замер | " + " | ".join(d["run"] for d in selected) + " |", "|---|" + "---:|" * len(selected)]
        for stage in ["native.resolve_and_keys", "native.admission", "native.device_init", "weights.cold_load", "weights.cpu_to_device", "clip.tokenize", "clip_l.total", "clip_l.forward", "clip_g.total", "clip_g.forward", "vae.encode_total", "unet.model_prepare", "unet.step", "vae.decode_total", "vae.model_prepare"]:
            values = [d["stages"].get(stage, {"total_s":0,"count":0}) for d in selected]
            lines.append("| " + stage + " | " + " | ".join(f"{v['total_s']:.3f} ({v['count']})" for v in values) + " |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", type=Path, default=Path("image-editor/artifacts/generation-bench.jsonl"))
    parser.add_argument("--receipt", type=Path, default=Path("image-editor/artifacts/generation-bench-receipt.json"))
    parser.add_argument("--output", type=Path, default=Path("image-editor/artifacts/generation-bench-summary"))
    args = parser.parse_args()
    if args.receipt.stat().st_mtime_ns < args.profile.stat().st_mtime_ns:
        raise ValueError("Receipt predates timing log; the benchmark may have failed before completing")
    result = summarize(args.profile, json.loads(args.receipt.read_text(encoding="utf-8")))
    args.output.with_suffix(".json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    report = markdown(result)
    args.output.with_suffix(".md").write_text(report, encoding="utf-8")
    print(report)


if __name__ == "__main__":
    main()
