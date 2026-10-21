"""Check fractional tiled output against an independently resized full synthetic 4x result."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import numpy as np
import torch
from PIL import Image
import upscale_tool
import worker_cache

class Nearest4x:
    def to(self, device):
        return self
    def __call__(self, tensor):
        return tensor.repeat_interleave(4, 2).repeat_interleave(4, 3)

class TargetTests(unittest.TestCase):
    def test_fractional_tiles_follow_global_coordinates_without_seams(self):
        y, x = np.indices((129,257))
        array = np.stack((x%256, y%256, (x*7+y*3)%256),axis=2).astype(np.uint8)
        image = Image.fromarray(array)
        with patch.object(worker_cache, "cached", return_value=Nearest4x()), patch.object(worker_cache,"key",return_value="fixture"):
            for size in [(384,192),(514,258)]:
                actual = upscale_tool.ultrasharp(image,Path("fixture"),128,"cpu",target_size=size)
                full = Image.fromarray(array.repeat(4,0).repeat(4,1)).resize(size,Image.Resampling.LANCZOS)
                self.assertEqual(actual.size,size)
                self.assertLessEqual(np.abs(np.asarray(actual).astype(int)-np.asarray(full).astype(int)).max(),1)
    def test_target_alpha_and_legacy_keep_size(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            image=Image.new("RGBA",(257,129),(30,90,150,123))
            image.save(root/"in.png")
            with patch.object(worker_cache,"cached",return_value=Nearest4x()), patch.object(worker_cache,"key",return_value="fixture"):
                for flags,size in [(["--target-width","384","--target-height","192"],(384,192)),(["--keep-size"],(257,129))]:
                    args=upscale_tool.parse_args(["upscale","--model","fixture","--variant","ultrasharp","--staging",str(root),"--device","cpu","--input",str(root/"in.png"),"--output",str(root/"out.png"),*flags])
                    upscale_tool.upscale(args)
                    actual=Image.open(root/"out.png")
                    self.assertEqual(actual.size,size)
                    self.assertEqual(actual.getchannel("A").getextrema(),(123,123))
    def test_invalid_target_flags_fail_before_inference(self):
        for flags in [["--target-width","384"],["--target-width","384","--target-height","192","--keep-size"],["--target-width","0","--target-height","192"]]:
            with self.assertRaises(ValueError):
                upscale_tool.parse_args(["upscale","--model","fixture","--variant","ultrasharp","--staging","fixture",*flags])

if __name__ == "__main__":
    unittest.main()
