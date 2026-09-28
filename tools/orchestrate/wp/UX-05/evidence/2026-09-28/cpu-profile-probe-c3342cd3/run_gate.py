#!/usr/bin/env python3
import hashlib, json, os, pathlib, subprocess, sys, time

repo = pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
evidence = pathlib.Path('/Volumes/betterSSD/tessera-validation/ux05-cpu-profile-probe/run-c3342cd3')
target = pathlib.Path('/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated')
command = ['cargo', 'test', '--jobs', '2', '-p', 'image-core', '--release', '--test', 'pinned_raw_cpu_profile', '--', '--test-threads=1']

def run_readonly(args):
    return subprocess.run(args, cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True).stdout

def tracked_hashes():
    paths = subprocess.run(['git', 'ls-files', '-z'], cwd=repo, check=True, stdout=subprocess.PIPE).stdout.split(b'\0')
    result = {}
    for raw in paths:
        if not raw:
            continue
        rel = os.fsdecode(raw)
        path = repo / rel
        if path.is_file():
            result[rel] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result

os.chdir(repo)
head = run_readonly(['git', 'rev-parse', 'HEAD']).strip()
status = run_readonly(['git', 'status', '--porcelain=v1'])
if status:
    raise SystemExit('refusing to run with a dirty checkout: ' + status)
if head != 'c3342cd3fc5a4e7fb550d108818d90dc8dfe6c3e':
    raise SystemExit('unexpected source HEAD: ' + head)
evidence.mkdir(parents=True, exist_ok=True)
inputs_before = tracked_hashes()
(evidence / 'inputs-before.json').write_text(json.dumps({'head': head, 'tracked_sha256': inputs_before}, sort_keys=True, indent=2) + '\n')
meta = {
    'head': head,
    'status_before': status,
    'command': command,
    'cwd': str(repo),
    'cargo_target_dir': str(target),
    'rustc_version': run_readonly(['rustc', '--version', '--verbose']),
    'cargo_version': run_readonly(['cargo', '--version']),
    'started_unix': time.time(),
}
(evidence / 'run-metadata.json').write_text(json.dumps(meta, sort_keys=True, indent=2) + '\n')
env = os.environ.copy()
env['CARGO_TARGET_DIR'] = str(target)
with (evidence / 'cargo-raw.log').open('wb') as log:
    proc = subprocess.Popen(command, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
    (evidence / 'child.pid').write_text(str(proc.pid) + '\n')
    rc = proc.wait()
# Record the direct child code before source post-snapshot work.
tmp_exit = evidence / '.direct-exit.json.tmp'
tmp_exit.write_text(json.dumps({'direct_child_returncode': rc, 'pid': proc.pid, 'completed_unix': time.time()}, sort_keys=True, indent=2) + '\n')
os.replace(tmp_exit, evidence / 'direct-exit.json')
inputs_after = tracked_hashes()
(evidence / 'inputs-after.json').write_text(json.dumps({'head': run_readonly(['git', 'rev-parse', 'HEAD']).strip(), 'tracked_sha256': inputs_after}, sort_keys=True, indent=2) + '\n')
(evidence / 'input-comparison.json').write_text(json.dumps({'identical': inputs_before == inputs_after, 'before_count': len(inputs_before), 'after_count': len(inputs_after)}, sort_keys=True, indent=2) + '\n')
(evidence / 'status-after.txt').write_text(run_readonly(['git', 'status', '--porcelain=v1']))
sys.exit(rc)
