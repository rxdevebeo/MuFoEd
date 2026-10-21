"""Build fast CLIP tokenizer JSON from local vocab/merges, without network or model code."""
import os
import sys
from pathlib import Path
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["USE_TF"] = "0"
from transformers import CLIPTokenizerFast
root = Path(sys.argv[1])
for name in ("tokenizer", "tokenizer_2"):
    folder = root / name
    if folder.is_dir() and not (folder / "tokenizer.json").exists():
        tokenizer = CLIPTokenizerFast.from_pretrained(str(folder), local_files_only=True)
        tokenizer.save_pretrained(str(folder))
