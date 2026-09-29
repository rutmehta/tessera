"""Validate audit evidence, not certify that Tessera's layout passes."""
from pathlib import Path
import json
import re
import struct

base = Path(__file__).resolve().parent
shots = base / "shots"
expected = [f"window-{mode}-{w}x{h}-{theme}.png"
            for mode in ["library", "raw", "document"]
            for w, h in [(1280, 800), (1440, 900), (1728, 1117)]
            for theme in ["dark", "light"]]
for name in expected:
    data = (shots / name).read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", name
    width, height = struct.unpack(">II", data[16:24])
    assert 0 < width <= 1400 and height > 0, (name, width, height)
log = (base / "window-probe.log").read_text()
assert len(re.findall(r"capture=0\b", log)) == len(expected)
assert log.count("frontmostIsProbe=false") == len(expected)
assert "frontmostIsProbe=true" not in log
assert "DATA library engine=true items=40 loading=false" in log
assert "DATA raw engine=true items=5 loading=false" in log
frames = json.loads((base / "frames-document-1280x800-dark.json").read_text())
split = next(row for row in frames if row["class"] == "NSSplitView")
numbers = [float(x) for x in re.findall(r"-?\d+(?:\.\d+)?", split["frameInHost"])]
assert numbers[0] < 0 and numbers[1] < 0, numbers
budget = (base / "budget-probe.log").read_text()
assert "documentStatus=true, fitting=(1318.0, 817.0)" in budget
assert "documentStatus=false, fitting=(960.0, 817.0)" in budget
font = json.loads((base / "font-metrics.json").read_text())
minimum_highlights = [row for row in font if row["title"] == "Highlights" and row["width"] == 80]
assert len(minimum_highlights) == 2 and all(row["gap"] <= 0 for row in minimum_highlights)
report = (base / "REPORT.md").read_text()
assert report.rstrip().endswith("RESULT: DONE")
findings = re.findall(r"^\| (D\d+) /", report, re.M)
risks = re.findall(r"^\| (R\d+) \|", report, re.M)
assert len(set(findings)) == 10 and len(set(risks)) == 12
result = {"matrix_captures": len(expected), "successful_captures": len(expected),
          "capture_checks_frontmost_false": len(expected), "maximum_width_px": 1400,
          "document_split_frame_in_host": numbers, "minimum_highlights_gap": minimum_highlights[0]["gap"],
          "confirmed_or_conditional_findings": len(findings), "additional_risks": len(risks),
          "scope": "Evidence consistency validation; not an application layout pass"}
(base / "validation.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2))
