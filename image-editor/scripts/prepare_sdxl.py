"""Convert local SDXL safetensors for unmodified Plakat, using offline diffusers.
Original checkpoint is read-only. No pickle loading and no weight downloads.
"""
import argparse
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--vae", type=Path, required=True)
    args = parser.parse_args()
    if args.checkpoint.suffix != ".safetensors" or not args.checkpoint.is_file():
        parser.error("An existing .safetensors checkpoint is required")
    import torch
    from diffusers import AutoencoderKL, StableDiffusionXLPipeline
    from transformers import CLIPTokenizerFast

    with args.checkpoint.open("rb") as file:
        header_size = int.from_bytes(file.read(8), "little")
        if not 1 <= header_size <= 64 * 1024 * 1024:
            parser.error("Invalid safetensors header")
        header_hash = hashlib.sha256(file.read(header_size)).hexdigest()
    revision = hashlib.sha256((str(args.checkpoint.resolve()) + header_hash).encode()).hexdigest()[:40]
    repo_root = args.cache / "models--local--image-studio-sdxl"
    snapshot = repo_root / "snapshots" / revision
    prepared = (snapshot / "receipt.json").exists()
    if snapshot.exists() and not prepared:
        parser.error(f"Incomplete conversion at {snapshot}; choose a fresh cache")
    if not prepared:
        print(f"Converting {args.checkpoint.name} on CPU with local config only", flush=True)
        pipe = StableDiffusionXLPipeline.from_single_file(
            str(args.checkpoint), config=str(args.config), local_files_only=True,
            torch_dtype=torch.float16, use_safetensors=True,
        )
        snapshot.mkdir(parents=True)
        pipe.save_pretrained(str(snapshot), safe_serialization=True, variant="fp16")
        for folder in ("tokenizer", "tokenizer_2"):
            CLIPTokenizerFast.from_pretrained(str(args.config / folder), local_files_only=True).save_pretrained(str(snapshot / folder))
        del pipe
    vae_root = args.cache / "models--madebyollin--sdxl-vae-fp16-fix"
    vae_snapshot = vae_root / "snapshots" / revision
    vae_snapshot.mkdir(parents=True, exist_ok=True)
    # Convert standalone LDM VAE tensor names to the diffusers layout Candle expects.
    vae = AutoencoderKL.from_single_file(str(args.vae), config=str(args.config / "vae"), local_files_only=True, torch_dtype=torch.float16)
    vae.save_pretrained(str(vae_snapshot), safe_serialization=True)
    receipt = {"source": str(args.checkpoint.resolve()), "source_size": args.checkpoint.stat().st_size,
               "safetensors_header_sha256": header_hash, "repo": "local/image-studio-sdxl", "revision": revision,
               "vae_source": str(args.vae.resolve()), "local_files_only": True, "preparation_version": 2}
    (snapshot / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    for root in (repo_root, vae_root):
        (root / "refs").mkdir(exist_ok=True)
        (root / "refs" / "main").write_text(revision, encoding="utf-8")
    print(json.dumps(receipt, indent=2), flush=True)


if __name__ == "__main__":
    main()
