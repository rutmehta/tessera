#!/usr/bin/env python3
"""Run one frozen native-preview gate and retain its exact exit/output."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
OUT = Path('/Volumes/betterSSD/tessera-validation/native-preview-cancellation/cd07b435')
MANIFEST = OUT / 'manifest.json'
COMMANDS = {
    'private': ['cargo', 'test', '-p', 'tessera-ffi', '--lib', 'request_cancellation_tests', '--', '--nocapture'],
    'document_filters': ['cargo', 'test', '-p', 'tessera-ffi', '--test', 'document_filters', '--', '--nocapture'],
    'strict': ['cargo', 'clippy', '-p', 'tessera-ffi', '--all-targets', '--', '-D', 'warnings'],
}

def verify():
    m = json.loads(MANIFEST.read_text())
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if head != m['head']:
        raise RuntimeError(f'HEAD changed: {head}')
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip():
        raise RuntimeError('worktree is not clean')
    for item in m['files']:
        b = (ROOT / item['path']).read_bytes()
        if len(b) != item['bytes'] or hashlib.sha256(b).hexdigest() != item['sha256']:
            raise RuntimeError(f'source changed: {item["path"]}')
    return m

def main():
    if len(sys.argv) != 2 or sys.argv[1] not in COMMANDS:
        raise SystemExit(f'usage: {sys.argv[0]} {{{"|".join(COMMANDS)}}}')
    gate = sys.argv[1]
    m = verify()
    cmd = COMMANDS[gate]
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS='2', RAYON_NUM_THREADS='2', CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/main')
    logfile = OUT / f'{gate}.log'
    resultfile = OUT / f'{gate}.json'
    if resultfile.exists():
        raise RuntimeError(f'result already exists: {resultfile}')
    start = time.monotonic()
    timed_out = False
    with logfile.open('wb') as log:
        p = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            exit_code = p.wait(timeout=600)
        except subprocess.TimeoutExpired:
            timed_out = True
            p.terminate()
            try:
                exit_code = p.wait(timeout=15)
            except subprocess.TimeoutExpired:
                p.kill()
                exit_code = p.wait()
    result = {'cmd':cmd,'exit_code':exit_code,'timed_out':timed_out,'elapsed_s':round(time.monotonic()-start,3),'pid':p.pid,'source_manifest':str(MANIFEST),'head':m['head'],'CARGO_BUILD_JOBS':'2','RAYON_NUM_THREADS':'2','CARGO_TARGET_DIR':env['CARGO_TARGET_DIR']}
    resultfile.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
    raise SystemExit(124 if timed_out else exit_code)

if __name__ == '__main__':
    main()
