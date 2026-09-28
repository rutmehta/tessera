#!/usr/bin/env python3
"""Prepare-only until coordinator grants exclusive compiler/GPU lane. No implementation."""
import argparse, hashlib, json, os, pathlib, re, subprocess, sys, time

EXPECTED_HEAD = '480b272efb3c0d864085442abe531969693d8516'
CHECKOUT = pathlib.Path('/Users/rutmehta/.codex/worktrees/export-integration/tessera')
FIXTURE = pathlib.Path('/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW')
EXPECTED_FIXTURE = 'bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8'
TARGET = '/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated'
TESTS = {
    'unchanged_public_open_reuses_cpu_and_metal_decisions_in_sdr_and_hdr',
    'cold_public_open_rejects_invalid_sources_without_cache',
    'cached_decision_cannot_bypass_real_source_or_sidecar_validation',
    'validated_full_identity_is_transported_and_rebuild_or_edit_misses',
    'overrides_external_geometry_and_device_failures_do_not_reuse_or_poison',
    'cache_does_not_override_active_editor_or_retain_failed_open_lease',
}
CONTROL = 'cold_public_open_rejects_invalid_sources_without_cache'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--execute', action='store_true')
p.add_argument('--attempt', required=True, help='new evidence folder name; never overwrite')
a = p.parse_args()
if not a.execute:
    sys.exit('No workload started: --execute requires an explicit exclusive runtime grant.')
if not re.fullmatch(r'[0-9][a-zA-Z0-9_-]*', a.attempt):
    sys.exit('Attempt must start with a digit and contain only letters, digits, _ or -.')
OUT = pathlib.Path(__file__).resolve().parent / a.attempt
OUT.mkdir(exist_ok=False)

def save(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
def sha(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()
def git(*args):
    return subprocess.check_output(['git', *args], cwd=CHECKOUT).decode()
def freeze():
    names = git('ls-files', '-z', '--cached', '--others', '--exclude-standard').split('\0')
    return {
        'head': git('rev-parse', 'HEAD').strip(),
        'status': git('status', '--porcelain'),
        'sources': {name: sha(CHECKOUT / name) for name in names if name and (CHECKOUT / name).is_file()},
        'fixture': {'path': str(FIXTURE), 'bytes': FIXTURE.stat().st_size, 'sha256': sha(FIXTURE)},
        'runner_sha256': sha(pathlib.Path(__file__).resolve()),
    }

before = freeze()
save(OUT / 'before.json', before)
if before['head'] != EXPECTED_HEAD or before['status'] or before['fixture']['sha256'] != EXPECTED_FIXTURE:
    save(OUT / 'status.json', {'phase': 'preflight', 'verdict': 'blocked source/fixture mismatch',
                              'expected_head': EXPECTED_HEAD, 'expected_fixture': EXPECTED_FIXTURE})
    sys.exit('Source/fixture mismatch retained; amend runner under review before a new attempt.')
# No stale profiling/preference/model/cache flags survive. Do not print unrelated
# inherited environment because it may contain credentials.
env = {k: v for k, v in os.environ.items() if not k.startswith('TESSERA_')}
env.pop('RAW_DECODE_FIXTURES', None)
env.update(CARGO_TARGET_DIR=TARGET, CARGO_BUILD_JOBS='2', MACOSX_DEPLOYMENT_TARGET='15.0',
           TESSERA_SMART_PREVIEW_RAW=str(FIXTURE), TESSERA_RENDER_BACKEND='')
contract = {k: v for k, v in env.items() if k.startswith(('TESSERA_', 'CARGO_', 'MACOSX_'))
            or k in ('RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTUP_TOOLCHAIN', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER')}
# Cargo environment can contain registry tokens: record only build knobs.
contract = {k: v for k, v in contract.items() if k.startswith(('TESSERA_', 'MACOSX_'))
            or k in ('CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_ENCODED_RUSTFLAGS',
                     'RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTUP_TOOLCHAIN', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER')}
save(OUT / 'environment.json', contract)
base = ['cargo', 'test', '-p', 'tessera-ffi', '--lib', '--release', 'proxy_cache_contracts']

def run_phase(name, argv):
    dest = OUT / name
    dest.mkdir()
    phase_before = freeze()
    save(dest / 'before.json', phase_before)
    save(dest / 'command.json', {'argv': argv, 'cwd': str(CHECKOUT), 'environment': contract})
    started = time.monotonic()
    returncode = None
    try:
        with (dest / 'run.log').open('w') as log:
            result = subprocess.run(argv, cwd=CHECKOUT, env=env, stdout=log, stderr=subprocess.STDOUT)
        returncode = result.returncode
        (dest / 'exit').write_text(str(returncode) + '\n')
    finally:
        after = freeze()
        save(dest / 'after.json', after)
        save(OUT / 'after.json', after)
        save(dest / 'freeze.json', {'equal': before == phase_before == after,
                                   'elapsed_seconds': time.monotonic() - started, 'direct_exit': returncode})
    if not before == phase_before == after:
        save(OUT / 'status.json', {'phase': name, 'verdict': 'source/fixture/runner drift'})
        sys.exit('Frozen input violation; all outputs retained, no next phase.')
    return returncode, (dest / 'run.log').read_text(errors='replace')

code, log = run_phase('01-compile', base + ['--no-run'])
if code:
    save(OUT / 'status.json', {'phase': 'compile', 'verdict': 'compile failure; NOT behavioral RED', 'direct_exit': code})
    sys.exit(code)
code, log = run_phase('02-behavior', base + ['--', '--ignored', '--nocapture', '--test-threads=1'])
# --test-threads=1 test output can contain logging between test name and result.
# Rust's failure inventory plus total result provides an independent named check.
prefix = 'develop::proxy_cache_contracts::'
failed = set(re.findall(r'^    ' + re.escape(prefix) + r'([A-Za-z0-9_]+)\s*$', log, re.MULTILINE))
result = re.search(r'test result: (FAILED|ok)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;', log)
control_pass = re.search(r'test ' + re.escape(prefix + CONTROL) + r' \.\.\. ok', log) is not None
named_started = set(re.findall(r'test ' + re.escape(prefix) + r'([A-Za-z0-9_]+) \.\.\.', log))
counts = None if result is None else {'passed': int(result[2]), 'failed': int(result[3]), 'ignored': int(result[4])}
expected_red = (code == 101 and counts == {'passed': 1, 'failed': 5, 'ignored': 0}
                and failed == TESTS - {CONTROL} and named_started == TESTS and control_pass)
save(OUT / 'status.json', {'phase': 'behavior', 'verdict': 'observed expected behavioral RED' if expected_red else 'unexpected outcome; inspect before implementation',
                         'direct_exit': code, 'counts': counts, 'cold_validation_control_passed': control_pass,
                         'named_tests_started': sorted(named_started), 'failed_tests': sorted(failed),
                         'no_implementation_or_performance_claim': True})
print(json.dumps({'attempt': str(OUT), 'expected_behavioral_red': expected_red, 'direct_exit': code, 'counts': counts}))
# Outer runner succeeds only after the specifically reviewed RED outcome. Direct
# cargo101 is retained in02-behavior/exit; never represented as a passing test gate.
sys.exit(0 if expected_red else 1)
