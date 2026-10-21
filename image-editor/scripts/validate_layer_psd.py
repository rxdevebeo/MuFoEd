"""Independent E03.2 PSD fixtures and re-export oracle (psd-tools 1.25)."""
import argparse
from pathlib import Path
import numpy as np
from PIL import Image
from psd_tools import PSDImage
from psd_tools.api.layers import PixelLayer
from psd_tools.constants import BlendMode, Compression

CASES = ("normal", "multiply", "screen", "overlay", "disabled", "offset")

def composite(psd):
    return psd.composite(force=True, ignore_preview=True, apply_icc=False).convert("RGBA")

def fixtures(root):
    root.mkdir(parents=True, exist_ok=True)
    for case in CASES:
        psd = PSDImage.new("RGBA", (4, 3))
        PixelLayer.frompil(Image.new("RGBA", (4, 3), (40, 160, 220, 255)), psd, name="Background", compression=Compression.RLE)
        layer = PixelLayer.frompil(Image.new("RGBA", (4, 3), (200, 80, 40, 128)), psd, name=case, compression=Compression.ZIP)
        layer.opacity = 173
        layer.blend_mode = getattr(BlendMode, case.upper(), BlendMode.NORMAL)
        mask = Image.frombytes("L", (4, 3), bytes([0, 64, 128, 255] * 3))
        if case == "offset":
            mask = Image.frombytes("L", (2, 1), bytes([0, 128]))
            layer.create_mask(mask, top=1, left=1)
            layer._record.mask_data.background_color = 255
        else:
            layer.create_mask(mask)
        layer.mask.disabled = case == "disabled"
        psd.save(root / f"layer-{case}.psd")
        composite(psd).save(root / f"layer-{case}.png")
    print("Independent PSD fixtures: six blend/mask cases PASS")

def check(root, outputs):
    for case in CASES:
        source = PSDImage.open(root / f"layer-{case}.psd")
        exported = PSDImage.open(outputs / f"layer-{case}.psd")
        assert len(exported) == len(source) == 2
        for a, b in zip(source, exported):
            assert (a.name, a.opacity, a.visible, a.blend_mode) == (b.name, b.opacity, b.visible, b.blend_mode)
            assert a.topil().convert("RGBA").tobytes() == b.topil().convert("RGBA").tobytes(), case
        a, b = source[1].mask, exported[1].mask
        assert (a.disabled, a.background_color) == (b.disabled, b.background_color), case
        normalized = Image.new("L", source.size, a.background_color)
        normalized.paste(a.topil(), (a.left, a.top))
        assert normalized.tobytes() == b.topil().tobytes(), case
        stored = np.asarray(exported.topil().convert("RGBA"), dtype=int)
        ours = np.asarray(Image.open(outputs / f"layer-{case}.png").convert("RGBA"), dtype=int)
        oracle = np.asarray(composite(exported), dtype=int)
        reference = np.asarray(composite(source), dtype=int)
        assert np.array_equal(stored, ours), case
        assert np.array_equal(oracle, reference), case
        error = np.abs(ours - oracle).max()
        assert error <= 1, (case, int(error), ours.tolist(), oracle.tolist())
        print(f"{case}: standard blend/mask/raw/merged PASS, independent composite max error={error}")
    print("Independent E03.2 PSD acceptance PASS")

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--fixtures", type=Path)
    p.add_argument("--check", type=Path)
    p.add_argument("--outputs", type=Path)
    args = p.parse_args()
    if args.fixtures:
        fixtures(args.fixtures)
    if args.check:
        check(args.check, args.outputs)
