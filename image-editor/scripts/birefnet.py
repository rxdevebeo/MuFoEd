"""Official BiRefNet ONNX download and local inference; no remote Python code execution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import urllib.request

MODELS = {
    "tiny": ("BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx", 224005088),
    "general": ("BiRefNet-general-epoch_244.onnx", 972666916),
    "matting": ("BiRefNet-matting-epoch_100.onnx", 972667742),
}


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def download(args):
    filename, size = MODELS[args.variant]
    url = f"https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/{filename}"
    if args.model.exists():
        raise ValueError("Model already exists; select it or choose a different destination")
    temporary = args.staging / "weights.part"
    count = 0
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "MuFoEd-ImageStudio/0.1"}), timeout=30) as response, temporary.open("wb") as stream:
        while chunk := response.read(1024 * 1024):
            count += len(chunk)
            if count > size:
                raise ValueError("Unexpected model size")
            stream.write(chunk)
            progress = args.staging / "progress.tmp"
            progress.write_text(json.dumps({"received": count, "total": size}), encoding="utf-8")
            progress.replace(args.staging / "progress.json")
    if count != size:
        raise ValueError("Incomplete model download")
    sha = digest(temporary)
    # Validate the graph before publishing. Dependencies are installed separately, never at runtime.
    import onnxruntime as ort
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    session = ort.InferenceSession(str(temporary), sess_options=options, providers=["CPUExecutionProvider"])
    if len(session.get_inputs()) != 1 or len(session.get_inputs()[0].shape) != 4:
        raise ValueError("Unexpected BiRefNet graph input")
    del session
    args.model.parent.mkdir(parents=True, exist_ok=True)
    receipt = {"source": url, "release": "v1", "variant": args.variant, "size": size, "sha256": sha,
               "integrity": "official HTTPS asset + graph validation; digest recorded at download"}
    args.model.with_suffix(".json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    # Staging and destination are on the same volume. Never overwrite an existing file.
    os.link(temporary, args.model)
    print("Model downloaded and validated", flush=True)


def remove_background(args):
    import numpy as np
    from PIL import Image
    import onnxruntime as ort
    receipt_path = args.model.with_suffix(".json")
    if receipt_path.exists():
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        if receipt.get("sha256") and digest(args.model) != receipt["sha256"]:
            raise ValueError("Local model digest differs from its download receipt")
    options = ort.SessionOptions()
    options.intra_op_num_threads = min(8, os.cpu_count() or 2)
    options.enable_mem_pattern = False
    available = ort.get_available_providers()
    providers = ["CPUExecutionProvider"]
    if args.device == "auto":
        if "CUDAExecutionProvider" in available:
            providers.insert(0, "CUDAExecutionProvider")
        elif "DmlExecutionProvider" in available:
            providers.insert(0, "DmlExecutionProvider")
            options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session = ort.InferenceSession(str(args.model), sess_options=options, providers=providers)
    tensor = session.get_inputs()[0]
    height = tensor.shape[2] if isinstance(tensor.shape[2], int) else 1024
    width = tensor.shape[3] if isinstance(tensor.shape[3], int) else 1024
    if not 64 <= height <= 4096 or not 64 <= width <= 4096:
        raise ValueError("Unsupported model resolution")
    image = Image.open(args.input).convert("RGBA")
    rgb = Image.alpha_composite(Image.new("RGBA", image.size, "white"), image).convert("RGB")
    array = np.asarray(rgb.resize((width, height), Image.Resampling.BILINEAR), dtype=np.float32) / 255.0
    array = (array - np.array([0.485, 0.456, 0.406], np.float32)) / np.array([0.229, 0.224, 0.225], np.float32)
    dtype = np.float16 if tensor.type == "tensor(float16)" else np.float32
    prediction = session.run(None, {tensor.name: array.transpose(2, 0, 1)[None].astype(dtype)})[-1]
    alpha = 1.0 / (1.0 + np.exp(-np.clip(prediction[0, 0].astype(np.float32), -40, 40)))
    mask = Image.fromarray(np.rint(alpha * 255).astype(np.uint8)).resize(image.size, Image.Resampling.BILINEAR)
    original = np.asarray(image, dtype=np.uint8).copy()
    original[:, :, 3] = ((original[:, :, 3].astype(np.uint16) * np.asarray(mask, dtype=np.uint16) + 127) // 255).astype(np.uint8)
    Image.fromarray(original).save(args.output, format="PNG")
    print(f"Background removed locally via {session.get_providers()[0]}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["download", "remove"])
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--variant", choices=MODELS, default="tiny")
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--device", choices=["auto", "cpu"], default="cpu")
    args = parser.parse_args()
    if args.action == "download":
        download(args)
    else:
        if not args.input or not args.output:
            parser.error("Input and output images are required")
        remove_background(args)


if __name__ == "__main__":
    main()
