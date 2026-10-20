"""SAM 2.1 Hiera Large via native Transformers, pinned official safetensors weights."""
import argparse
from contextlib import nullcontext
import hashlib
import json
import math
import os
from pathlib import Path
import urllib.request

REPO = "facebook/sam2.1-hiera-large"
REVISION = "665f8e2ad61cf5f53d65644ff27c8ee525124610"
FILES = {
    "config.json": (5705, "00446988cf4d617118d2d347eabe2c46aebed744628facdd540508be30b69ec3"),
    "preprocessor_config.json": (683, "6ebf229ee259368ce4a8d4f2fe893a72b053023710853e257253939e601f583d"),
    "processor_config.json": (95, "f8a68e865cfad115c1c2763f3d93eca7b1c622da06da2a9273eb437fb2389b6d"),
    "model.safetensors": (897897416, "dc407dce21301fd94abb395c5099b4f2c455fdc8a8f261ac3d0ea6d4cd197230"),
}


def download(args):
    staging = args.staging / "model"
    staging.mkdir()
    if args.model.exists():
        raise ValueError("Destination already exists; choose the existing local model or another folder")
    received = 0
    total = sum(size for size, _ in FILES.values())
    for filename, (size, expected) in FILES.items():
        url = f"https://huggingface.co/{REPO}/resolve/{REVISION}/{filename}"
        value = hashlib.sha256()
        count = 0
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "MuFoEd-ImageStudio/0.1"}), timeout=30) as response, (staging / filename).open("wb") as stream:
            while chunk := response.read(1024 * 1024):
                count += len(chunk)
                if count > size:
                    raise ValueError("Unexpected model file size")
                stream.write(chunk)
                value.update(chunk)
                progress = args.staging / "progress.tmp"
                progress.write_text(json.dumps({"received": received + count, "total": total}), encoding="utf-8")
                progress.replace(args.staging / "progress.json")
        if count != size or value.hexdigest() != expected:
            raise ValueError(f"Integrity verification failed: {filename}")
        received += count
    (staging / "receipt.json").write_text(json.dumps({"repo": REPO, "revision": REVISION, "files": FILES}, indent=2), encoding="utf-8")
    # Complete folder is made visible only after every file passes SHA256.
    staging.rename(args.model)
    print("SAM 2.1 Hiera Large downloaded; all SHA256 hashes verified", flush=True)


def segment(args):
    import numpy as np
    from PIL import Image
    import torch
    from transformers import Sam2Config, Sam2Model, Sam2Processor
    torch.set_num_threads(min(8, os.cpu_count() or 2))
    points = json.loads(args.points)
    image = Image.open(args.input).convert("RGB")
    if not 1 <= len(points) <= 128 or not any(p[2] == 1 for p in points):
        raise ValueError("Place at least one positive point; maximum 128 points")
    for x, y, label in points:
        if not math.isfinite(x) or not math.isfinite(y) or not 0 <= x < image.width or not 0 <= y < image.height or label not in [0, 1]:
            raise ValueError("Invalid segmentation point")
    if args.device == "auto" and not torch.cuda.is_available():
        raise ValueError("CUDA is unavailable; explicitly select CPU")
    device = "cuda" if args.device == "auto" else "cpu"
    config = json.loads((args.model / "config.json").read_text(encoding="utf-8"))
    if config.get("model_type") not in ("sam2", "sam2_video"):
        raise ValueError("Expected native SAM2 Transformers model config")
    # The official checkpoint config includes video-memory fields. The image predictor
    # uses the same vision/prompt/mask components; verify that none are left uninitialized.
    import worker_cache
    def load_model():
        model, loading = Sam2Model.from_pretrained(str(args.model), config=Sam2Config.from_dict(config),
            local_files_only=True, use_safetensors=True, output_loading_info=True)
        if loading.get("missing_keys") or loading.get("mismatched_keys") or loading.get("error_msgs"):
            raise ValueError(f"Incomplete SAM2 image weights: {loading}")
        return model.eval(), Sam2Processor.from_pretrained(str(args.model), local_files_only=True)
    worker_cache.stage("loading", "SAM 2.1: weights / placement")
    model, processor = worker_cache.cached(("sam2", worker_cache.key(args.model / "model.safetensors")), load_model)
    model.to(device)
    inputs = processor(images=image, input_points=[[[[x, y] for x, y, _ in points]]], input_labels=[[[int(p[2]) for p in points]]], return_tensors="pt").to(device)
    with torch.inference_mode(), (torch.autocast("cuda", dtype=torch.bfloat16) if device == "cuda" else nullcontext()):
        image_key = hashlib.sha256(image.tobytes() + str(image.size).encode()).hexdigest()
        if image_key not in worker_cache.EMBEDDINGS:
            worker_cache.stage("running", "SAM 2.1: image encoder")
            worker_cache.EMBEDDINGS.clear()
            worker_cache.EMBEDDINGS[image_key] = [v.cpu() for v in model.get_image_embeddings(inputs["pixel_values"])]
        else:
            worker_cache.EMBED_HITS += 1
        embeddings = [v.to(device) for v in worker_cache.EMBEDDINGS[image_key]]
        inputs.pop("pixel_values")
        worker_cache.stage("running", "SAM 2.1: mask decoder")
        output = model(**inputs, image_embeddings=embeddings)
    masks = processor.post_process_masks(output.pred_masks.float().cpu(), inputs["original_sizes"].cpu())[0]
    index = int(output.iou_scores[0, 0].argmax().item())
    mask = masks[0, index].numpy().astype(np.uint8) * 255
    Image.fromarray(mask).save(args.output, format="PNG")
    print(f"SAM 2.1 Hiera Large: {len(points)} points, device={device}, mask={index}", flush=True)


def parse_args(arguments=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["download", "segment"])
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--device", choices=["cpu", "auto"], default="auto")
    parser.add_argument("--points", default="[]")
    if arguments is not None:
        def invalid(message):
            raise ValueError(message)
        parser.error = invalid
    args = parser.parse_args(arguments)
    return args


def main():
    args = parse_args()
    if args.action == "download":
        download(args)
    else:
        segment(args)


if __name__ == "__main__":
    main()
