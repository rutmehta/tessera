#!/usr/bin/env python3
"""Acquire pinned segmentation ONNX files; never silently regenerate exports."""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
IDS = ("segment/u2net", "segment/sam-encoder", "segment/sam-decoder")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def acquire(spec, directory, download):
    path = directory / spec["download_url"].rsplit("/", 1)[1]
    if not path.exists():
        if not download:
            raise FileNotFoundError(f"{path}: run fetch first")
        directory.mkdir(parents=True, exist_ok=True)
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(dir=directory, suffix=".partial", delete=False) as out:
                temporary = Path(out.name)
                with urllib.request.urlopen(spec["download_url"], timeout=120) as response:
                    while chunk := response.read(1024 * 1024):
                        out.write(chunk)
            if digest(temporary) != spec["sha256"]:
                raise ValueError(f"SHA-256 mismatch: {spec['id']}")
            temporary.replace(path)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    if digest(path) != spec["sha256"]:
        raise ValueError(f"SHA-256 mismatch: {path}; remove corrupt file explicitly")
    print(json.dumps({"id": spec["id"], "sha256": spec["sha256"], "bytes": path.stat().st_size}))
    return path


def verify(spec, path):
    import onnx
    import onnxruntime as ort
    onnx.checker.check_model(str(path))
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    session = ort.InferenceSession(str(path), sess_options=options, providers=["CPUExecutionProvider"])
    for key, values in (("inputs", session.get_inputs()), ("outputs", session.get_outputs())):
        actual = [(v.name, v.shape, v.type) for v in values]
        print(json.dumps({"id": spec["id"], key: actual}))
        if [v.name for v in values] != [v["name"] for v in spec[key]]:
            raise ValueError(f"{spec['id']} {key}: name mismatch")
        for graph, declared in zip(values, spec[key]):
            if graph.type != "tensor(float)" or len(graph.shape) != len(declared["shape"]):
                raise ValueError(f"{graph.name}: dtype/rank mismatch")
            for actual_dim, declared_dim in zip(graph.shape, declared["shape"]):
                if isinstance(actual_dim, int) and actual_dim != declared_dim:
                    raise ValueError(f"{graph.name}: static dimension mismatch")
    return session


def smoke(sessions):
    import numpy as np
    # Synthetic input verifies execution/contracts, not segmentation quality.
    u2 = sessions[IDS[0]].run(None, {"input.1": np.zeros((1, 3, 320, 320), np.float32)})
    assert len(u2) == 7 and all(x.shape == (1, 1, 320, 320) for x in u2)
    assert all(np.isfinite(x).all() for x in u2)
    encoder, decoder = (sessions[k] for k in IDS[1:])
    for h, w in ((512, 1024), (1024, 512)):
        embeddings = encoder.run(None, {"input_image": np.full((h, w, 3), 127, np.float32)})[0]
        assert embeddings.shape == (1, 256, 64, 64) and np.isfinite(embeddings).all()
        for labels in ((1, -1), (1, 0, -1), (2, 3)):
            points = np.array([[[w / 4, h / 4]] * len(labels)], np.float32)
            feed = {"image_embeddings": embeddings, "point_coords": points,
                    "point_labels": np.array([labels], np.float32),
                    "mask_input": np.zeros((1, 1, 256, 256), np.float32),
                    "has_mask_input": np.zeros(1, np.float32),
                    "orig_im_size": np.array([h // 2, w // 2], np.float32)}
            results = decoder.run(None, feed)
            assert [x.shape for x in results] == [(1, 1, h // 2, w // 2), (1, 1), (1, 1, 256, 256)]
            assert all(np.isfinite(x).all() for x in results)
            feed["mask_input"] = results[2]
            feed["has_mask_input"][:] = 1
            assert all(np.isfinite(x).all() for x in decoder.run(None, feed))
            print(json.dumps({"smoke": "passed", "size": [h, w], "labels": labels,
                              "outputs": [list(x.shape) for x in results]}))
    print("PASS: U2Net + MobileSAM portrait/landscape, dynamic points, boxes, refinement")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("fetch", "verify"))
    parser.add_argument("--directory", type=Path, default=ROOT / "tools/orchestrate/wp/M3-04/.cache/segment-models")
    parser.add_argument("--registry-cache", type=Path,
                        help="also atomically install SHA-named files for the Rust registry")
    parser.add_argument("--smoke", action="store_true")
    args = parser.parse_args()
    if args.smoke and args.command != "verify":
        parser.error("--smoke requires verify")
    specs = {m["id"]: m for m in tomllib.loads((ROOT / "crates/ml-runtime/models.toml").read_text())["models"]}
    sessions = {}
    for model_id in IDS:
        spec = specs[model_id]
        path = acquire(spec, args.directory, args.command == "fetch")
        if args.registry_cache:
            args.registry_cache.mkdir(parents=True, exist_ok=True)
            target = args.registry_cache / (spec["sha256"] + ".onnx")
            if target.exists():
                if digest(target) != spec["sha256"]:
                    raise ValueError(f"corrupt registry cache: {target}")
            else:
                import shutil
                with tempfile.NamedTemporaryFile(dir=args.registry_cache, delete=False) as out:
                    temporary = Path(out.name)
                    try:
                        with path.open("rb") as source:
                            shutil.copyfileobj(source, out)
                        out.flush()
                        if digest(temporary) != spec["sha256"]:
                            raise ValueError("copy checksum mismatch")
                        temporary.replace(target)
                    finally:
                        temporary.unlink(missing_ok=True)
        if args.command == "verify":
            sessions[model_id] = verify(spec, path)
    if args.smoke:
        smoke(sessions)


if __name__ == "__main__":
    main()
