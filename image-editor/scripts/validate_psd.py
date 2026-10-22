"""Independent PSD QA. Run with optional psd-tools/Pillow; no runtime dependency."""
import argparse
import json
from pathlib import Path
from PIL import Image
from psd_tools import PSDImage
from psd_tools.api.layers import PixelLayer
from psd_tools.constants import BlendMode, Compression

def fixtures(directory):
    directory.mkdir(parents=True, exist_ok=True)
    psd = PSDImage.new("RGBA", (4, 3))
    PixelLayer.frompil(Image.new("RGBA", (4, 3), (10, 20, 30, 255)), psd, name="Background", compression=Compression.RLE)
    layer = PixelLayer.frompil(Image.new("RGBA", (2, 1), (100, 150, 200, 128)), psd, name="Offset", top=1, left=1, compression=Compression.ZIP)
    layer.opacity = 173
    psd.save(directory / "external-rgb8.psd")
    layer.blend_mode = BlendMode.MULTIPLY
    psd.save(directory / "external-multiply.psd")
    masked=PSDImage.new("RGB", (4,3))
    PixelLayer.frompil(Image.new("RGBA", (4,3), (10,20,30,128)), masked, name="Masked")
    masked.save(directory / "external-mask.psd")
    PSDImage.new("RGB", (2, 2), depth=16).save(directory / "external-rgb16.psd")
    print("Independent fixtures: RGB8/RLE+ZIP/offset, multiply, RGB16 PASS")

def check(path):
    receipt = json.loads(path.with_suffix(".receipt.json").read_text(encoding="utf-8"))
    psd = PSDImage.open(path)
    assert psd.size == (receipt["width"], receipt["height"])
    assert len(psd) == len(receipt["layers"])
    for layer, expected in zip(psd, receipt["layers"]):
        assert layer.name == expected["name"], (layer.name, expected["name"])
        assert layer.opacity == expected["opacity"]
        assert layer.visible == expected["visible"]
        actual = layer.topil().convert("RGBA")
        source = Image.open(expected["png"]).convert("RGBA")
        assert actual.size == source.size and actual.tobytes() == source.tobytes(), layer.name
    actual = psd.topil().convert("RGBA")
    merged = Image.open(path.with_suffix(".png")).convert("RGBA")
    assert actual.tobytes() == merged.tobytes(), "Merged PSD pixels differ"
    print(f"Independent PSD decode: {len(psd)} layers, exact RGBA/order/names/visibility/opacity/merged pixels PASS")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("psd", nargs="?", type=Path)
    parser.add_argument("--fixtures", type=Path)
    args = parser.parse_args()
    if args.fixtures:
        fixtures(args.fixtures)
    if args.psd:
        check(args.psd)
