"""Local 4x-UltraSharp and AuraSR v2, with pinned safetensors downloads."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import urllib.request

MODELS = {
    "ultrasharp": ("Kim2091/UltraSharp", "920fe218c211f831b43cb30327f203e2b59f5dab", {
        "4x-UltraSharp.safetensors": (66864028, "36a340b5509b699d2c06cb445ddc1d3d39199ac734d889ed6d7915f60e05bcbc")}),
    "aurasr": ("fal/AuraSR-v2", "ff452185a7c8b51206dd62c21c292e7baad5c3a3", {
        "config.json": (222, "5009aca786759dc78369abc666878ec7dd99167beaada28bf473fbaf1a68ac9a"),
        "model.safetensors": (2470247916, "8a80b4546f3f49b9095733cb9699030409339e2f4eb0ae9c1ada14c7d21d4dde")}),
}


def download(args):
    repo, revision, files = MODELS[args.variant]
    if args.model.exists():
        raise ValueError("Download folder already exists; use its weights or select another destination")
    staging = args.staging / "model"
    staging.mkdir()
    received = 0
    for filename, (size, expected) in files.items():
        url = f"https://huggingface.co/{repo}/resolve/{revision}/{filename}"
        value = hashlib.sha256()
        count = 0
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "MuFoEd-ImageStudio/0.1"}), timeout=30) as response, (staging / filename).open("wb") as stream:
            while chunk := response.read(1024 * 1024):
                count += len(chunk)
                if count > size:
                    raise ValueError("Unexpected weight file size")
                stream.write(chunk)
                value.update(chunk)
                progress = args.staging / "progress.tmp"
                progress.write_text(json.dumps({"received": received + count, "total": sum(v[0] for v in files.values())}), encoding="utf-8")
                progress.replace(args.staging / "progress.json")
        if count != size or value.hexdigest() != expected:
            raise ValueError(f"SHA256 verification failed: {filename}")
        received += count
    (staging / "receipt.json").write_text(json.dumps({"repo": repo, "revision": revision, "files": files}, indent=2), encoding="utf-8")
    staging.rename(args.model)
    print(f"{args.variant} downloaded; SHA256 verified", flush=True)


def ultrasharp(image, path, tile_size, device):
    import numpy as np
    import torch
    from PIL import Image
    from spandrel import ModelLoader, ImageModelDescriptor
    if path.is_dir():
        path = path / "4x-UltraSharp.safetensors"
    if path.suffix.lower() == ".safetensors":
        from safetensors.torch import load_file
        state = load_file(str(path), device="cpu")
    else:
        state = torch.load(str(path), map_location="cpu", weights_only=True)
    model = ModelLoader().load_from_state_dict(state)
    if not isinstance(model, ImageModelDescriptor) or model.scale != 4 or model.input_channels != 3 or model.output_channels != 3:
        raise ValueError("Expected RGB 4x-UltraSharp architecture")
    model = model.to(device).eval()
    tensor = torch.from_numpy(np.asarray(image, np.float32).copy() / 255).permute(2, 0, 1).unsqueeze(0)
    result = torch.zeros((1, 3, image.height * 4, image.width * 4))
    padding = 32
    with torch.inference_mode():
        for y in range(0, image.height, tile_size):
            for x in range(0, image.width, tile_size):
                x1, y1 = max(0, x - padding), max(0, y - padding)
                x2, y2 = min(image.width, x + tile_size + padding), min(image.height, y + tile_size + padding)
                output = model(tensor[:, :, y1:y2, x1:x2].to(device)).float().cpu()
                height, width = min(tile_size, image.height - y), min(tile_size, image.width - x)
                result[:, :, y*4:(y+height)*4, x*4:(x+width)*4] = output[:, :, (y-y1)*4:(y-y1+height)*4, (x-x1)*4:(x-x1+width)*4]
    array = result[0].clamp(0, 1).permute(1, 2, 0).numpy()
    return Image.fromarray(np.rint(array * 255).astype(np.uint8))


def aurasr(image, path, device):
    import torch
    torch.manual_seed(0)
    from aura_sr import AuraSR
    from safetensors.torch import load_file
    from PIL import Image
    root = path if path.is_dir() else path.parent
    model = AuraSR(json.loads((root / "config.json").read_text(encoding="utf-8")), device=device)
    model.upsampler.load_state_dict(load_file(str(root / "model.safetensors"), device="cpu"), strict=True)
    model.upsampler.eval()
    padded = image
    if image.width < 64 or image.height < 64:
        import numpy as np
        array = np.asarray(image)
        padded = Image.fromarray(np.pad(array, ((0, max(0, 64-image.height)), (0, max(0, 64-image.width)), (0, 0)), mode="edge"))
    return model.upscale_4x_overlapped(padded, max_batch_size=1).crop((0, 0, image.width*4, image.height*4))


def upscale(args):
    import torch
    import math
    from PIL import Image
    torch.set_num_threads(min(8, os.cpu_count() or 2))
    torch.manual_seed(0)
    device = "cuda" if args.device == "auto" and torch.cuda.is_available() else "cpu"
    image = Image.open(args.input).convert("RGBA")
    if image.width * image.height * 16 > 16777216:
        raise ValueError("4x output exceeds editor's 16,777,216-pixel limit; crop/resize the source first")
    rgb = Image.alpha_composite(Image.new("RGBA", image.size, "white"), image).convert("RGB")
    if args.variant == "ultrasharp":
        result = ultrasharp(rgb, args.model, args.tile, device)
    elif args.variant == "aurasr":
        result = aurasr(rgb, args.model, device)
    else:
        weights = [args.ultrasharp_weight, args.aura_weight]
        if not all(math.isfinite(w) and 0 <= w <= 1 for w in weights) or sum(weights) <= 0:
            raise ValueError("Mix weights must be in 0..1 with a positive sum")
        first = ultrasharp(rgb, args.model, args.tile, device) if weights[0] > 0 else None
        if device == "cuda":
            torch.cuda.empty_cache()
        second = aurasr(rgb, args.aura_model, device) if weights[1] > 0 else None
        result = second if first is None else first if second is None else Image.blend(first, second, weights[1] / sum(weights))
    if result.size != (image.width*4, image.height*4):
        raise ValueError("Upscaler returned an unexpected output size")
    result = result.convert("RGBA")
    result.putalpha(image.getchannel("A").resize(result.size, Image.Resampling.LANCZOS))
    result.save(args.output, format="PNG")
    print(f"{args.variant}: 4x on {device}; weights={args.ultrasharp_weight}/{args.aura_weight}, alpha=Lanczos, output {result.width}x{result.height}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["download", "upscale"])
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--variant", choices=["ultrasharp", "aurasr", "blend"], required=True)
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--aura-model", type=Path)
    parser.add_argument("--ultrasharp-weight", type=float, default=0.65)
    parser.add_argument("--aura-weight", type=float, default=0.35)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--device", choices=["auto", "cpu"], default="auto")
    parser.add_argument("--tile", type=int, choices=[128, 256, 512], default=256)
    args = parser.parse_args()
    if args.action == "download":
        if args.variant == "blend":
            parser.error("Download the two models individually")
        download(args)
    else:
        upscale(args)


if __name__ == "__main__":
    main()
