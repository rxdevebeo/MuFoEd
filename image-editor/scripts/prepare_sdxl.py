"""Offline SDXL preparation through the shared checkpoint-provenance converter.
An optional external VAE is prepared separately; pass its returned path as Request.vae.
Original checkpoint and existing user caches remain read-only.
"""
import argparse
import json
from pathlib import Path
import tempfile
from prepare_asset import main as prepare


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--vae", type=Path, help="Prepare a separate optional VAE override")
    args = parser.parse_args(argv)
    common = ["--family", "sdxl", "--cache", str(args.cache), "--config", str(args.config)]
    with tempfile.TemporaryDirectory(prefix="image-studio-prepare-") as folder:
        result = Path(folder) / "result.json"
        prepare([str(args.checkpoint), "--kind", "model", "--result", str(result), *common])
        model = json.loads(result.read_text(encoding="utf-8"))["result"]
        output = {"model": model, "vae_policy": "explicit_override_or_checkpoint", "vae": None}
        if args.vae is not None:
            prepare([str(args.vae), "--kind", "vae", "--result", str(result), *common])
            output["vae"] = json.loads(result.read_text(encoding="utf-8"))["result"]
    print(json.dumps(output, indent=2), flush=True)


if __name__ == "__main__":
    main()
