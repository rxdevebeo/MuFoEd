# Independent PSD fixtures

Generated with psd-tools 1.25.0/Pillow by `scripts/validate_psd.py --fixtures
image-editor/tests/fixtures/psd`. No model weights or third-party artwork.

- external-rgb8.psd: opaque 4x3 background, 2x1 RGBA offset at (1,1), opacity 173;
  RLE background / ZIP foreground, neutral Blend If ranges.
- external-multiply.psd: same content with unsupported multiply foreground.
- external-mask.psd: real Photoshop pixel mask, unsupported and explicitly refused.
- external-rgb16.psd: unsupported RGB16 document.

Rust tests read the checked-in binaries without Python installed.
