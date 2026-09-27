#!/usr/bin/env python3
"""Convert a decoded photograph to the latency benchmark's packed RGB8 input.

Usage: python3 prepare_photo.py photo.png /tmp/m5-34-photo.rgb
Requires Pillow. Decoding, resizing, and file I/O are outside benchmark timings.
"""
import argparse
from PIL import Image

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("photo")
parser.add_argument("output")
args = parser.parse_args()
with Image.open(args.photo) as image:
    print(f"Input: {args.photo}, {image.size}, {image.mode}")
    if image.width * image.height < 19_000_000:
        raise SystemExit("Use a real photograph with at least 19MP; do not upscale a thumbnail")
    image = image.convert("RGB").resize((5472, 3648), Image.Resampling.LANCZOS)
    with open(args.output, "wb") as output:
        output.write(image.tobytes())
print(f"Wrote {args.output}: 5472 x 3648 packed RGB8")
