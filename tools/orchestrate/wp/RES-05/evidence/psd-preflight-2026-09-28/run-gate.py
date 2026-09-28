#!/usr/bin/env python3
"""Run one frozen RES05a gate with a process-group watchdog and direct exit."""
import argparse
import json
import os
import pathlib
import signal
import subprocess
import sys

parser = argparse.ArgumentParser()
parser.add_argument('--out', required=True)
parser.add_argument('--seconds', type=int, default=600)
parser.add_argument('command', nargs=argparse.REMAINDER)
args = parser.parse_args()
command = args.command[1:] if args.command[:1] == ['--'] else args.command
if not command:
    parser.error('missing command')
repo = pathlib.Path('/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera')
out = pathlib.Path(args.out)
out.mkdir(parents=True, exist_ok=False)
env = os.environ.copy()
env.update({
    'CARGO_TARGET_DIR': '/Volumes/betterSSD/tessera-cache/target/main',
    'CARGO_BUILD_JOBS': '2',
    'RAYON_NUM_THREADS': '2',
    'MACOSX_DEPLOYMENT_TARGET': '15.0',
})
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
status = subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=repo, text=True)
(out / 'source-head.txt').write_text(head + '\n')
(out / 'source-status.txt').write_text(status)
tracked = subprocess.check_output(['git', 'ls-files', '-z', 'Cargo.toml', 'Cargo.lock', 'crates/compositor', 'crates/psd', 'crates/tessera-ffi'], cwd=repo)
files = [str(repo / os.fsdecode(path)) for path in tracked.split(b'\0') if path]
with (out / 'source-sha256.txt').open('w') as manifest:
    subprocess.run(['shasum', '-a', '256', *files], check=True, stdout=manifest)
(out / 'command.json').write_text(json.dumps({'argv': command, 'env': {k: env[k] for k in ('CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'RAYON_NUM_THREADS', 'MACOSX_DEPLOYMENT_TARGET')}, 'watchdog_seconds': args.seconds}, indent=2) + '\n')
with (out / 'run.log').open('wb') as log:
    child = subprocess.Popen(command, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        exit_code = child.wait(timeout=args.seconds)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        exit_code = 124
(out / 'direct-exit.txt').write_text(str(exit_code) + '\n')
(out / 'post-head.txt').write_text(subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True))
(out / 'post-status.txt').write_text(subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=repo, text=True))
with (out / 'post-source-sha256.txt').open('w') as manifest:
    subprocess.run(['shasum', '-a', '256', *files], check=True, stdout=manifest)
sys.exit(exit_code)
