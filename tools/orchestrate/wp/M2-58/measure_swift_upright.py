"""Repeat the already-built real Swift Auto Upright acceptance check, without a window."""
import datetime
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent
COMMAND = ['swift', 'test', '--package-path', str(ROOT / 'apps/mac'), '-c', 'release',
           '--skip-build', '--filter', 'DevelopTests.testAutoUprightMainSetterAndFinalIdentity']
rows = []
for index in range(1, 4):
    row = {'run': index, 'at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
           'command': COMMAND, 'load_before': os.getloadavg()}
    result = subprocess.run(COMMAND, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (OUT / f'swift-upright-run-{index}.log').write_text(result.stdout)
    row.update(exit_code=result.returncode, load_after=os.getloadavg())
    match = re.search(r'Auto Upright Swift setter: count=(\d+) median_ms=([\d.eE+-]+) p95_ms=([\d.eE+-]+) max_ms=([\d.eE+-]+)', result.stdout)
    if match:
        row.update(count=int(match[1]), median_ms=float(match[2]), p95_ms=float(match[3]), max_ms=float(match[4]))
    rows.append(row)
    (OUT / 'swift-upright-runs.json').write_text(json.dumps(rows, indent=2) + '\n')
    print(json.dumps(row), flush=True)
raise SystemExit(0 if all(row['exit_code'] == 0 and row.get('count') == 101 for row in rows) else 1)
