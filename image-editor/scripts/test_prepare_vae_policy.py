"""Converter provenance, independently faking heavy ML modules and using temporary caches."""
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import prepare_asset
import prepare_sdxl
import contextlib
import io

class FakePipe:
    def __init__(self, marker): self.marker = marker
    @classmethod
    def from_single_file(cls, path, **kwargs):
        assert kwargs["local_files_only"] and kwargs["use_safetensors"]
        return cls(Path(path).read_bytes())
    def save_pretrained(self, path, **kwargs):
        folder = Path(path)/"vae"
        folder.mkdir(parents=True)
        (folder/"diffusion_pytorch_model.fp16.safetensors").write_bytes(self.marker)
    @property
    def vae(self): raise AssertionError("checkpoint VAE must never be exported to a global alias")
class FakeVAE(FakePipe):
    def save_pretrained(self, path, **kwargs):
        Path(path).mkdir(parents=True, exist_ok=True)
        (Path(path)/"diffusion_pytorch_model.safetensors").write_bytes(self.marker)
class FakeTokenizer:
    @classmethod
    def from_pretrained(cls, *args, **kwargs): return cls()
    def save_pretrained(self, folder):
        Path(folder).mkdir(parents=True)
        (Path(folder)/"tokenizer.json").write_text("{}")

class VaePolicyTests(unittest.TestCase):
    def test_legacy_entrypoint_prepares_external_vae_separately_and_never_publishes_a_global_alias(self):
        modules = {"torch":types.SimpleNamespace(float16="half"),
                   "diffusers":types.SimpleNamespace(AutoencoderKL=FakeVAE, StableDiffusionPipeline=FakePipe, StableDiffusionXLPipeline=FakePipe),
                   "transformers":types.SimpleNamespace(CLIPTokenizerFast=FakeTokenizer)}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root/"config"
            config.mkdir()
            (config/"model_index.json").write_text("{}")
            sources = []
            for name in ["checkpoint", "external"]:
                path = root/(name+".safetensors")
                path.write_bytes((2).to_bytes(8,"little")+b"{}"+name.encode())
                sources.append(path)
            stream = io.StringIO()
            with patch.dict(sys.modules, modules), contextlib.redirect_stdout(stream):
                prepare_sdxl.main([str(sources[0]), "--config", str(config), "--cache", str(root/"cache"), "--vae", str(sources[1])])
            output = json.loads(stream.getvalue()[stream.getvalue().index("{"):])
            self.assertEqual(Path(output["vae"]).read_bytes(), sources[1].read_bytes())
            repo = root/"cache"/("models--"+output["model"].replace("/","--"))
            snap = repo/"snapshots"/(repo/"refs/main").read_text()
            self.assertEqual((snap/"vae/diffusion_pytorch_model.fp16.safetensors").read_bytes(), sources[0].read_bytes())
            self.assertFalse((root/"cache/models--madebyollin--sdxl-vae-fp16-fix").exists())
            self.assertEqual(json.loads((Path(output["vae"]).parent/"receipt.json").read_text())["vae_policy"], "explicit_override")

    def test_two_checkpoints_in_either_order_keep_their_own_vae_and_leave_old_cache_untouched(self):
        modules = {"torch":types.SimpleNamespace(float16="half"),
                   "diffusers":types.SimpleNamespace(AutoencoderKL=FakeVAE, StableDiffusionPipeline=FakePipe, StableDiffusionXLPipeline=FakePipe),
                   "transformers":types.SimpleNamespace(CLIPTokenizerFast=FakeTokenizer)}
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            config=root/"hf/models--stabilityai--stable-diffusion-xl-base-1.0/snapshots/config"
            config.mkdir(parents=True)
            (config/"model_index.json").write_text("{}")
            sources={}
            for name in ["A","B"]:
                p=root/(name+".safetensors")
                p.write_bytes((2).to_bytes(8,"little")+b"{}"+name.encode())
                sources[name]=p
            for order in [("A","B"),("B","A")]:
                cache=root/("cache-"+"".join(order))
                old=cache/"models--madebyollin--sdxl-vae-fp16-fix/refs/main"
                old.parent.mkdir(parents=True)
                old.write_bytes(b"user-cache-must-stay")
                for name in order:
                    result=root/"result.json"
                    argv=["prepare_asset.py",str(sources[name]),"--kind","model","--family","sdxl","--cache",str(cache),"--result",str(result)]
                    with patch.dict(sys.modules,modules),patch.dict(os.environ,{"HF_HUB_CACHE":str(root/"hf")}),patch.object(sys,"argv",argv):
                        prepare_asset.main()
                    repo=json.loads(result.read_text())["result"]
                    folder=cache/("models--"+repo.replace("/","--"))
                    snapshot=folder/"snapshots"/(folder/"refs/main").read_text()
                    self.assertEqual((snapshot/"vae/diffusion_pytorch_model.fp16.safetensors").read_bytes(),sources[name].read_bytes())
                    receipt=json.loads((snapshot/"receipt.json").read_text())
                    self.assertEqual(receipt["vae_repository"],repo)
                    self.assertEqual(receipt["preparation_version"],4)
                self.assertEqual(old.read_bytes(),b"user-cache-must-stay")
                self.assertEqual(len(list(old.parent.parent.rglob("*"))),2)

if __name__ == "__main__": unittest.main()
