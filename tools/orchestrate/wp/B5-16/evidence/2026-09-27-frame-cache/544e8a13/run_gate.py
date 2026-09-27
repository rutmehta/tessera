#!/usr/bin/env python3
"""Run one frozen integration gate, with source check and raw evidence."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
OUT = Path('/Volumes/betterSSD/tessera-validation/frame-cache-integration/544e8a13')
MANIFEST = OUT / 'manifest.json'
COMMANDS = {
    'frame': ['cargo','test','-p','tessera-ffi','--lib','frame_cancellation_tests','--','--nocapture'],
    'cache': ['cargo','test','-p','tessera-ffi','--lib','image_cache_tests','--','--nocapture'],
    'preview': ['cargo','test','-p','tessera-ffi','--lib','request_cancellation_tests','--','--nocapture'],
    'filters': ['cargo','test','-p','tessera-ffi','--test','document_filters','--','--nocapture'],
    'ring': ['cargo','test','-p','tessera-ffi','--test','document','frames_are_coalesced_and_straight_alpha','--','--nocapture'],
    'replaced_ring': ['cargo','test','-p','tessera-ffi','--test','document_viewport','frames_for_a_replaced_ring_are_dropped','--','--nocapture'],
    'strict': ['cargo','clippy','-p','tessera-ffi','--all-targets','--','-D','warnings'],
}

def verify():
    m=json.loads(MANIFEST.read_text())
    head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    if head!=m['head']: raise RuntimeError(f'HEAD changed: {head}')
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():
        raise RuntimeError('worktree is not clean')
    for item in m['files']:
        b=(ROOT/item['path']).read_bytes()
        if len(b)!=item['bytes'] or hashlib.sha256(b).hexdigest()!=item['sha256']:
            raise RuntimeError(f'source changed: {item["path"]}')
    return m

def main():
    if len(sys.argv)!=2 or sys.argv[1] not in COMMANDS:
        raise SystemExit(f'usage: {sys.argv[0]} {{{"|".join(COMMANDS)}}}')
    gate=sys.argv[1]
    m=verify()
    cmd=COMMANDS[gate]
    env=os.environ.copy()
    env.update(CARGO_BUILD_JOBS='2',RAYON_NUM_THREADS='2',CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/main')
    log=OUT/f'{gate}.log'
    result=OUT/f'{gate}.json'
    if result.exists(): raise RuntimeError(f'result already exists: {result}')
    started=time.monotonic();timed_out=False
    with log.open('wb') as stream:
        p=subprocess.Popen(cmd,cwd=ROOT,env=env,stdout=stream,stderr=subprocess.STDOUT)
        try: code=p.wait(timeout=600)
        except subprocess.TimeoutExpired:
            timed_out=True;p.terminate()
            try: code=p.wait(timeout=15)
            except subprocess.TimeoutExpired: p.kill();code=p.wait()
    data={'cmd':cmd,'exit_code':code,'timed_out':timed_out,'elapsed_s':round(time.monotonic()-started,3),'pid':p.pid,'source_manifest':str(MANIFEST),'head':m['head'],'CARGO_BUILD_JOBS':'2','RAYON_NUM_THREADS':'2','CARGO_TARGET_DIR':env['CARGO_TARGET_DIR']}
    result.write_text(json.dumps(data,indent=2)+'\n')
    print(json.dumps(data,indent=2))
    raise SystemExit(124 if timed_out else code)

if __name__=='__main__': main()
