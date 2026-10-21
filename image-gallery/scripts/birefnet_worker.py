"""BiRefNet background removal worker for image-gallery.

The gallery does the image work (compositing, resizing, normalization, the
mask); this worker only runs the ONNX graph, so it needs nothing beyond
numpy and onnxruntime. It is the same BiRefNet model and the same download
receipt as Image Studio's birefnet.py, so either tool can use the other's
weights.

Modes:

  serve --model M [--device cpu|auto]
      Prints one JSON line: {"ready": true, "width": W, "height": H,
      "provider": P} (or {"error": ...} and exits). Then, for every request
      line {"input": IN, "output": OUT} on stdin, reads IN (float32,
      little-endian, 1x3xHxW), runs the graph and writes OUT (float32,
      little-endian, HxW logits of the last output), answering {"ok": true}
      or {"error": ...}. Ends at end of input.

  download --model M --variant tiny|general|matting
      Fetches the official release asset over HTTPS, checks its size,
      validates the graph, writes the receipt (M with .json) and publishes M.
      Prints {"received": n, "total": t} lines, then {"done": true}, or
      {"error": ...}. Never overwrites an existing file.
"""
import argparse
import hashlib
import json
import os
import sys
import urllib.request
from pathlib import Path

MODELS = {
    "tiny": ("BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx", 224005088),
    "general": ("BiRefNet-general-epoch_244.onnx", 972666916),
    "matting": ("BiRefNet-matting-epoch_100.onnx", 972667742),
}
RELEASE = "https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/"


def say(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            value.update(chunk)
    return value.hexdigest()


def session_for(model, device):
    import onnxruntime as ort

    receipt = model.with_suffix(".json")
    if receipt.exists():
        expected = json.loads(receipt.read_text(encoding="utf-8")).get("sha256")
        if expected and digest(model) != expected:
            raise ValueError("the model file differs from its download receipt")
    options = ort.SessionOptions()
    options.intra_op_num_threads = min(8, os.cpu_count() or 2)
    options.enable_mem_pattern = False
    providers = ["CPUExecutionProvider"]
    if device == "auto":
        if "CUDAExecutionProvider" not in ort.get_available_providers():
            raise ValueError("the CUDA provider of onnxruntime is not available; choose the CPU")
        providers.insert(0, "CUDAExecutionProvider")
    return ort.InferenceSession(str(model), sess_options=options, providers=providers)


def serve(args):
    import numpy as np

    try:
        session = session_for(args.model, args.device)
        tensor = session.get_inputs()[0]
        if len(tensor.shape) != 4:
            raise ValueError("unexpected BiRefNet input")
        height = tensor.shape[2] if isinstance(tensor.shape[2], int) else 1024
        width = tensor.shape[3] if isinstance(tensor.shape[3], int) else 1024
        if not (64 <= height <= 4096 and 64 <= width <= 4096):
            raise ValueError("unsupported model resolution")
        dtype = np.float16 if tensor.type == "tensor(float16)" else np.float32
    except Exception as error:  # reported to the gallery, which shows it
        say({"error": str(error)})
        return 1
    say({"ready": True, "width": width, "height": height, "provider": session.get_providers()[0]})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
            data = np.fromfile(request["input"], dtype="<f4")
            if data.size != 3 * height * width:
                raise ValueError("input tensor of the wrong size")
            batch = data.reshape(1, 3, height, width).astype(dtype)
            prediction = session.run(None, {tensor.name: batch})[-1]
            logits = np.asarray(prediction, dtype=np.float32).reshape(-1)[-(height * width):]
            logits.astype("<f4").tofile(request["output"])
            say({"ok": True})
        except Exception as error:
            say({"error": str(error)})
    return 0


def download(args):
    filename, size = MODELS[args.variant]
    model = args.model
    if model.exists():
        say({"error": "the model file already exists"})
        return 1
    model.parent.mkdir(parents=True, exist_ok=True)
    partial = model.with_name(model.name + ".part")
    try:
        request = urllib.request.Request(RELEASE + filename, headers={"User-Agent": "MuFoEd-ImageGallery/0.1"})
        received = 0
        with urllib.request.urlopen(request, timeout=30) as response, partial.open("wb") as stream:
            while chunk := response.read(1 << 20):
                received += len(chunk)
                if received > size:
                    raise ValueError("unexpected model size")
                stream.write(chunk)
                say({"received": received, "total": size})
        if received != size:
            raise ValueError("incomplete model download")
        session_for(partial, "cpu")
        receipt = {
            "source": RELEASE + filename,
            "release": "v1",
            "variant": args.variant,
            "size": size,
            "sha256": digest(partial),
            "integrity": "official HTTPS asset + graph validation; digest recorded at download",
        }
        model.with_suffix(".json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
        try:
            os.link(partial, model)
        except OSError:  # no hard links on this volume
            os.replace(partial, model)
    except Exception as error:
        say({"error": str(error)})
        return 1
    finally:
        if partial.exists():
            partial.unlink()
    say({"done": True})
    return 0


def main():
    parser = argparse.ArgumentParser(description="BiRefNet worker for image-gallery")
    parser.add_argument("mode", choices=["serve", "download"])
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--device", choices=["cpu", "auto"], default="cpu")
    parser.add_argument("--variant", choices=sorted(MODELS), default="tiny")
    args = parser.parse_args()
    return serve(args) if args.mode == "serve" else download(args)


if __name__ == "__main__":
    sys.exit(main())
