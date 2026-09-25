"""Validate a real cargo-run report, including pair coverage and decision gate."""
import json
import math
from pathlib import Path

root = Path(__file__).resolve().parent
report = json.loads((root / "report.json").read_text())
rows = report["kernels"]
names = {"demosaic_bilinear_rggb", "guided_filter_r8", "lut3d_oklab_33"}
expected = {(name, backend) for name in names for backend in ("wgpu", "metal")}
assert len(rows) == 6
assert {(r["name"], r["backend"]) for r in rows} == expected
for row in rows:
    for key in ("gpu_ms", "wall_ms", "max_abs_err", "mean_abs_err"):
        assert math.isfinite(row[key]) and row[key] >= 0, (row, key)
    assert row["gpu_ms"] > 0 and row["wall_ms"] > 0
    assert row["mean_abs_err"] <= row["max_abs_err"]
    assert row["gpu_ms_min"] <= row["gpu_ms"] <= row["gpu_ms_max"]
by_key = {(r["name"], r["backend"]): r for r in rows}
fails = 0
for name in names:
    w, m = (by_key[name, backend] for backend in ("wgpu", "metal"))
    fails += w["gpu_ms"] > 1.5 * m["gpu_ms"] or w["max_abs_err"] > 1e-4
expected_rec = "go" if fails == 0 else "no-go" if fails == 3 else "go-with-msl-passthrough"
assert report["recommendation"] == expected_rec
assert report["environment"]["runs"] == 20
assert report["environment"]["warmup"] == 3
for field in ("extended_srgb_linear", "extended_display_p3"):
    assert isinstance(report["hdr"][field], bool)
assert "Surface::display_hdr_info" in report["hdr"]["notes"]
assert report["rationale"]
md = (root / "REPORT.md").read_text()
assert "Recommendation:" in md and "1.5x" in md.splitlines()[-1]
print("validated six measured kernel/backend results:", expected_rec)
