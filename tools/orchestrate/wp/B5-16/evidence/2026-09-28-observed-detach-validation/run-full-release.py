#!/usr/bin/env python3
"""Run one frozen full Release gate after explicit compiler-slot handoff."""
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time

HERE = Path(__file__).resolve().parent
MANIFEST = json.loads((HERE / 'source-manifest.json').read_text())
ROOT = Path(MANIFEST['worktree'])
SCRATCH = Path('/Volumes/betterSSD/tessera-cache/swift/document-save-settlement/scratch')
LOG = HERE / 'full-release.log'
RESULT = HERE / 'full-release.json'

def verify_source():
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    assert head == MANIFEST['candidate_head'], (head, MANIFEST['candidate_head'])
    status = subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=ROOT, text=True)
    assert status == MANIFEST['status_porcelain'], repr(status)
    for relative, record in MANIFEST['files'].items():
        actual = hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()
        assert actual == record['sha256'], (relative, actual, record['sha256'])

verify_source()
cmd = ['swift', 'test', '--jobs', '2', '--package-path', str(ROOT / 'apps/mac'),
       '--scratch-path', str(SCRATCH), '-c', 'release', '-Xswiftc', '-enable-testing']
started = time.time()
with LOG.open('wb') as log:
    process = subprocess.Popen(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                               start_new_session=True)
    timed_out = False
    try:
        returncode = process.wait(timeout=1500)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(process.pid, signal.SIGTERM)
        try:
            returncode = process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            returncode = process.wait()
output = LOG.read_text(errors='replace')
counts = [int(n) for n in re.findall(r'Executed (\d+) tests?', output)]
record = {'command': cmd, 'direct_exit_code': returncode, 'timed_out': timed_out,
          'started_unix': started, 'elapsed_seconds': time.time()-started,
          'executed_counts_observed': counts, 'candidate_head': MANIFEST['candidate_head'],
          'raw_log': str(LOG)}
RESULT.write_text(json.dumps(record, indent=2) + '\n')
verify_source()
if timed_out or returncode != 0 or not counts or max(counts) < 500:
    raise SystemExit(f'Full gate failed or undercounted: {RESULT}')
