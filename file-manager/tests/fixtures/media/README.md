# Generated media regression fixtures

These synthetic images (FFmpeg testsrc2) and silent audio were generated locally
for decoder regression tests; they contain no third-party recordings.

- h264-aac.mp4: 160x96, 10 fps, H.264 High with B frames and AAC-LC stereo.
- vp8.webm / vp9.webm: 160x96, 10 fps, VP8 / VP9, no audio.
- mpeg4.mp4: 160x96, 10 fps, MPEG-4 Part 2, no audio.
- audio.mp3 / audio.flac: three seconds of mono silence at 16 kHz.

The adapter tests decode the whole file, including delayed frames at EOF.
