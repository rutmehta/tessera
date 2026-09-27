"""Repeat the gated P09 release regression and retain actual latencies/load."""
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

root = Path(__file__).resolve().parents[4]
out = Path(__file__).resolve().parent
assert os.environ['CARGO_TARGET_DIR'] == '/Volumes/betterSSD/tessera-cache/target/M2-57'
rows = []
for _ in range(20):
    load = os.getloadavg()
    result = subprocess.run(['cargo', 'test', '-p', 'jobs', '--release',
                             'three_slow_previews_leave_viewport_capacity', '--', '--nocapture'],
                            cwd=root, capture_output=True, text=True)
    text = result.stdout + result.stderr
    with (out / 'scheduler-repeat.log').open('a') as log:
        log.write(text)
    if result.returncode:
        raise RuntimeError(text)
    match = re.search(r'viewport admission latency: ([\d.]+)(µs|ms|ns|s)', text)
    if not match:
        raise RuntimeError(text)
    ms = float(match[1]) * {'µs': .001, 'ms': 1, 'ns': .000001, 's': 1000}[match[2]]
    rows.append({'ms': ms, 'load': load})
report = {'samples': rows, 'median_ms': statistics.median(r['ms'] for r in rows),
          'max_ms': max(r['ms'] for r in rows)}
(out / 'scheduler-measurements.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report))
