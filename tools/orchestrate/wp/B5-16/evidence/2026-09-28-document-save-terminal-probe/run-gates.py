#!/usr/bin/env python3
"""Explicitly invoked after compiler-slot handoff; never runs at import time."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time

EVIDENCE = Path(__file__).resolve().parent
MANIFEST = json.loads((EVIDENCE / 'source-manifest.json').read_text())
ROOT = Path(MANIFEST['worktree'])
SCRATCH = Path('/Volumes/betterSSD/tessera-cache/swift/document-save-settlement/scratch')
STAGES = (
    ('save-settlement', 'DocumentSaveSettlementTests', 30),
    ('sheet-probe', 'DocumentSaveSheetProbeTests', 3),
    ('sheet-attachment', 'DocumentSaveSheetAttachmentTests', 3),
    ('load-settlement', 'DocumentLoadSettlementTests', 4),
    ('save-adjacent', 'testSaveAsNames|testSaveOpenAndExport|testEngineSessionThroughTheAdapter', 3),
)

def verify_source():
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if head != MANIFEST['candidate_head']:
        raise SystemExit(f'HEAD changed: {head}')
    status = subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=ROOT, text=True)
    if status != MANIFEST['status_porcelain']:
        raise SystemExit(f'worktree status changed: {status!r}')
    for relative, record in MANIFEST['files'].items():
        actual = hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()
        if actual != record['sha256']:
            raise SystemExit(f'input changed: {relative} {actual}')

def run_stage(label, filter_expression, expected_min):
    cmd = ['swift', 'test', '--jobs', '2', '--package-path', str(ROOT / 'apps/mac'),
           '--scratch-path', str(SCRATCH), '-c', 'release', '-Xswiftc', '-enable-testing',
           '--filter', filter_expression]
    log_path = EVIDENCE / f'{label}.log'
    result_path = EVIDENCE / f'{label}.json'
    started = time.time()
    with log_path.open('wb') as log:
        process = subprocess.Popen(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        timed_out = False
        try:
            returncode = process.wait(timeout=900)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGTERM)
            try:
                returncode = process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                returncode = process.wait()
    output = log_path.read_text(errors='replace')
    counts = [int(n) for n in re.findall(r'Executed (\d+) tests?', output)]
    result = {'command': cmd, 'direct_exit_code': returncode, 'timed_out': timed_out,
              'started_unix': started, 'elapsed_seconds': time.time()-started,
              'executed_counts_observed': counts, 'expected_min': expected_min,
              'raw_log': str(log_path), 'candidate_head': MANIFEST['candidate_head']}
    result_path.write_text(json.dumps(result, indent=2)+'\n')
    if timed_out or returncode != 0 or not counts or max(counts) < expected_min:
        raise SystemExit(f'{label} gate failed or ran too few tests; see {result_path}')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--run', action='store_true', help='requires explicit slot grant')
    args = parser.parse_args()
    if not args.run:
        raise SystemExit('No tests run. Invoke with --run after compiler-slot grant.')
    verify_source()
    for stage in STAGES:
        run_stage(*stage)
