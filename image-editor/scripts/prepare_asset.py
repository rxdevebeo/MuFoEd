"""Offline safetensors conversion for the pinned Plakat adapter; no pickle or downloads."""
import argparse
import hashlib
import json
import os
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--kind", choices=["model", "vae"], required=True)
    parser.add_argument("--family", choices=["sd15", "sdxl"], required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--result", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    if source.suffix.lower() != ".safetensors":
        parser.error("Only safetensors are supported")
    with source.open("rb") as stream:
        length = int.from_bytes(stream.read(8), "little")
        if not 2 <= length <= 16 * 1024 * 1024:
            parser.error("Invalid safetensors header")
        header_hash = hashlib.sha256(stream.read(length)).hexdigest()
    fingerprint = hashlib.sha256((str(source) + str(source.stat().st_size) + str(source.stat().st_mtime_ns) + header_hash + args.family).encode()).hexdigest()[:40]
    cache = args.cache.resolve()
    hf = Path(os.environ.get("HF_HUB_CACHE", Path(os.environ.get("HF_HOME", Path.home() / ".cache/huggingface")) / "hub"))
    config_repo = "models--stabilityai--stable-diffusion-xl-base-1.0" if args.family == "sdxl" else "models--stable-diffusion-v1-5--stable-diffusion-v1-5"
    configs = sorted((hf / config_repo / "snapshots").glob("*/model_index.json"))
    if not configs:
        parser.error(f"Local diffusers config/tokenizers missing in {hf / config_repo}; install them before offline conversion")
    config = configs[-1].parent
    import torch
    from diffusers import AutoencoderKL, StableDiffusionPipeline, StableDiffusionXLPipeline
    from transformers import CLIPTokenizerFast
    repo = f"local/image-studio-{args.family}-{fingerprint[:16]}"
    root = cache / ("models--" + repo.replace("/", "--"))
    snapshot = root / "snapshots" / fingerprint
    if args.kind == "vae":
        snapshot = cache / "image-editor-vaes" / fingerprint
        if not (snapshot / "receipt.json").exists():
            vae = AutoencoderKL.from_single_file(str(source), config=str(config / "vae"), local_files_only=True, torch_dtype=torch.float16)
            snapshot.mkdir(parents=True, exist_ok=True)
            vae.save_pretrained(str(snapshot), safe_serialization=True)
        result = str(snapshot / "diffusion_pytorch_model.safetensors")
    else:
        if not (snapshot / "receipt.json").exists():
            pipeline = StableDiffusionXLPipeline if args.family == "sdxl" else StableDiffusionPipeline
            kwargs = {} if args.family == "sdxl" else {"safety_checker": None, "feature_extractor": None, "requires_safety_checker": False}
            pipe = pipeline.from_single_file(str(source), config=str(config), local_files_only=True, torch_dtype=torch.float16, use_safetensors=True, **kwargs)
            snapshot.mkdir(parents=True, exist_ok=True)
            pipe.save_pretrained(str(snapshot), safe_serialization=True, variant="fp16")
            for folder in (["tokenizer", "tokenizer_2"] if args.family == "sdxl" else ["tokenizer"]):
                CLIPTokenizerFast.from_pretrained(str(config / folder), local_files_only=True).save_pretrained(str(snapshot / folder))
            # Pinned upstream resolves this namespace for SDXL's default VAE.
            # Bootstrap only when absent; explicit VAE selection uses its own isolated file.
            vae_root = cache / "models--madebyollin--sdxl-vae-fp16-fix"
            if args.family == "sdxl" and not (vae_root / "refs/main").exists():
                vae_snapshot = vae_root / "snapshots" / fingerprint
                pipe.vae.save_pretrained(str(vae_snapshot), safe_serialization=True)
                (vae_root / "refs").mkdir(exist_ok=True)
                (vae_root / "refs/main").write_text(fingerprint, encoding="utf-8")
        (root / "refs").mkdir(parents=True, exist_ok=True)
        # Commit the reference only after the receipt below has been written.
        result = repo
    receipt = {"source": str(source), "source_size": source.stat().st_size,
               "safetensors_header_sha256": header_hash, "repo": repo,
               "family": args.family, "revision": fingerprint,
               "local_files_only": True, "preparation_version": 3}
    (snapshot / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    if args.kind == "model":
        (root / "refs/main").write_text(fingerprint, encoding="utf-8")
    args.result.write_text(json.dumps({"result": result}), encoding="utf-8")
    print("Preparation complete", flush=True)


if __name__ == "__main__":
    main()
