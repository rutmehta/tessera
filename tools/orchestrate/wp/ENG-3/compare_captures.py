#!/usr/bin/env python3
"""Exact ENG-3 before/after audit. Any change requires a separate seed audit.

Capture using ENG1_CAPTURE with camera_raw's three full_develop tests (one
thread), then pipeline-cpu --test golden. No stored golden is modified.
The zero-change acceptance predicate is stricter than the allowed nonzero
change predicate: every changed output must have an active pre-fix |L|<1e-3
seed at the same pixel or within a separately justified downstream support
radius. A nonempty diff is deliberately a blocker here; this tool
does not invent unrecorded seeds or accept differences using a tolerance.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct


def compare(before, after):
    names = [f"{profile}-{amount}.rgba" for profile in ("Srgb", "DisplayP3", "AdobeRgb")
             for amount in ("1", "0.35", "0")]
    names += [f"{raw}.rgb8" for raw in ("canon-cr3", "fuji-raf", "nikon-nef", "sample", "sony-arw")]
    result = []
    for name in names:
        a, b = (root.joinpath(name).read_bytes() for root in (before, after))
        assert len(a) == len(b), name
        step = 16 if name.endswith("rgba") else 3
        assert len(a) % step == 0, name
        changed = sum(a[i:i+step] != b[i:i+step] for i in range(0, len(a), step))
        if step == 16:
            va, vb = (struct.unpack(f"<{len(v)//4}f", v) for v in (a, b))
            assert all(math.isfinite(v) for v in (*va, *vb)), name
            assert all(a[i:i+4] == b[i:i+4] for i in range(12, len(a), 16)), name
        else:
            va, vb = a, b
        delta = max(abs(x-y) for x, y in zip(va, vb))
        result.append(dict(golden=name, pixels=len(a)//step, changed_pixels=changed,
                           max_encoded_delta=delta, max_distance=None,
                           predicate="empty change set" if changed == 0 else "BLOCKED: seed attribution required",
                           before_sha256=hashlib.sha256(a).hexdigest(),
                           after_sha256=hashlib.sha256(b).hexdigest()))
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    rows = compare(args.before, args.after)
    args.output.write_text(json.dumps(rows, indent=2) + "\n")
    for row in rows:
        print(f"{row['golden']}: {row['changed_pixels']}/{row['pixels']}, max delta {row['max_encoded_delta']}")
    if any(row["changed_pixels"] for row in rows):
        raise SystemExit("BLOCKED: changed pixels require pre-fix seed attribution")
