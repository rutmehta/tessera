#!/usr/bin/env python3
"""Explicit opt-in download of MIT Florence-2-base-ft ONNX weights. No remote code."""
import argparse
import hashlib
from pathlib import Path
import tempfile
import urllib.request

REVISION = "e88a44eaf3791a35eae0c5a47b3dbcd36e67eb6f"
FILES = {
    "vision_encoder_fp16": "a7abcd77199c5d0089cf985ede4dd8089acd84f30fb3fb1462d5930345c688b3",
    "embed_tokens_fp16": "da2607930eea5e21e4a2bd5fd069de550f1acc30316a4e8f824551a95232ba39",
    "encoder_model_fp16": "0d1d929f282963e983b8ac5ac4957f19a8fa48233eab41166951b769e5cf2fd2",
    "decoder_model_fp16": "ce583853b630f230eaa1ef201e35001cdda968c84749d67c18ce3707171cfa0c",
    "tokenizer": "d69dcdb2323e124ac4f800cb9863ddccea0d7bb11e16125e8df3bd60f2f8aeac",
}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch(cache):
    cache.mkdir(parents=True, exist_ok=True)
    for name, sha in FILES.items():
        source = "tokenizer.json" if name == "tokenizer" else f"onnx/{name}.onnx"
        dest = cache / ("florence-tokenizer.json" if name == "tokenizer" else f"{sha}.onnx")
        if dest.exists():
            if digest(dest) != sha:
                raise ValueError(f"SHA-256 mismatch: {dest}")
            print(f"verified {dest}", flush=True)
            continue
        url = f"https://huggingface.co/onnx-community/Florence-2-base-ft/resolve/{REVISION}/{source}"
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as out:
                temporary = Path(out.name)
                with urllib.request.urlopen(url, timeout=180) as response:
                    while chunk := response.read(1024 * 1024):
                        out.write(chunk)
            if digest(temporary) != sha:
                raise ValueError(f"SHA-256 mismatch downloading {source}")
            temporary.replace(dest)
            print(f"verified {dest}", flush=True)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    fetch(parser.parse_args().cache)
