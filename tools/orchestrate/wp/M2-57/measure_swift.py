"""Measure the bounded synthetic 20k reversal harness, not UI frame latency."""
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
root = Path(__file__).resolve().parents[4]
out = Path(__file__).resolve().parent
rows = []
for i in range(5):
    load = os.getloadavg()
    run = subprocess.run(['swift', 'test', '--package-path', 'apps/mac', '-c', 'release', '--skip-build',
                          '--filter', 'ThumbnailQueueTests.testTwentyThousandReversal'],
                         cwd=root, capture_output=True, text=True)
    text = run.stdout + run.stderr
    (out / f'swift-reversal-{i}.log').write_text(text)
    if run.returncode:
        raise RuntimeError(text)
    m = re.search(r'peak pending=(\d+), active=(\d+), subscribers=(\d+), delivered=(\d+), traceMs=([\d.]+)', text)
    if not m:
        raise RuntimeError(text)
    rows.append(dict(pending=int(m[1]), active=int(m[2]), subscribers=int(m[3]), delivered=int(m[4]),
                     trace_ms=float(m[5]), load=load))
result = {'configuration': 'release Swift, release Rust archive', 'samples': rows,
          'median_trace_ms': statistics.median(r['trace_ms'] for r in rows)}
(out / 'swift-measurements.json').write_text(json.dumps(result, indent=2))
print(json.dumps(result))
