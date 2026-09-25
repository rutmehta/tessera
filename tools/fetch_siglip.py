#!/usr/bin/env python3
"""Fetch verified Apache-2.0 SigLIP ONNX towers and matching tokenizer.

Python 3 stdlib only. Explicit opt-in network access; inference tests never fetch.
Usage: python3 tools/fetch_siglip.py --cache /path/to/model-cache
See crates/ml-embed/README.md for provenance and usage. Do not commit weights.
"""
import argparse
import hashlib
import os
from pathlib import Path
import tempfile
import urllib.request

REVISION = "4649052661e53c7000355844105f8a1792088239"
BASE = f"https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/{REVISION}/"
FILES = [
    ("onnx/vision_model.onnx", 371819850,
     "f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f"),
    ("onnx/text_model.onnx", 441332132,
     "3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553"),
    ("tokenizer.json", 2398744,
     "4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08"),
]


def verify(path, size, digest):
    if path.stat().st_size != size:
        raise ValueError(f"size mismatch: {path}")
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != digest:
        raise ValueError(f"SHA-256 mismatch: {path}")


def fetch(cache):
    cache.mkdir(parents=True, exist_ok=True)
    for remote, size, digest in FILES:
        target = cache / (digest + ".onnx" if remote.endswith(".onnx") else remote)
        if not target.exists():
            temporary = None
            try:
                with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
                    temporary = Path(output.name)
                    with urllib.request.urlopen(BASE + remote, timeout=120) as response:
                        copied = 0
                        while chunk := response.read(1024 * 1024):
                            copied += len(chunk)
                            if copied > size:
                                raise ValueError(f"oversized download: {remote}")
                            output.write(chunk)
                    output.flush()
                    os.fsync(output.fileno())
                verify(temporary, size, digest)
                os.replace(temporary, target)
            finally:
                if temporary is not None:
                    temporary.unlink(missing_ok=True)
        verify(target, size, digest)
        print(f"verified {target.name} bytes={size} sha256={digest}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    fetch(parser.parse_args().cache)
