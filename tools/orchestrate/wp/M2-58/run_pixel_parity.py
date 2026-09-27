"""Prepare (default) or explicitly build/run the pinned, headless P10 A/B capture.

Only --run creates temporary test files or invokes Cargo. It requires a serialized
heavy slot. Existing tracked files are never overwritten; only this harness's
unique file is removed, and only if its SHA-256 still matches.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
HARNESS = HERE / 'parity_capture.rs'
TEST = 'm2_58_upright_parity_capture'
PRODUCT = ['Cargo.toml', 'Cargo.lock', 'crates', '.cargo']
TREES = [
    ('before', Path('/Users/rutmehta/Developer/tessera'), 'c0d4535', Path('/Volumes/betterSSD/tessera-cache/target/main')),
    ('after', HERE.parents[3], '5ff2679', Path('/Volumes/betterSSD/tessera-cache/target/M2-58')),
]
COMMAND = ['cargo', 'test', '--locked', '--release', '-p', 'tessera-ffi', '--test', TEST,
           '--no-run', '--message-format=json']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_state(root, pin):
    subprocess.run(['git', 'diff', '--exit-code', pin, '--', *PRODUCT], cwd=root,
                   check=True, stdout=subprocess.DEVNULL)
    paths = subprocess.check_output(['git', 'ls-files', '-z', '--', *PRODUCT], cwd=root).decode().split('\0')
    return {p: digest(root / p) for p in paths if p and (root / p).is_file()}


def compare(before, after):
    left = json.loads((before / 'metadata.json').read_text())
    right = json.loads((after / 'metadata.json').read_text())
    metadata_differences = [key for key in sorted(set(left) | set(right)) if left.get(key) != right.get(key)]
    a, b = (before / 'pixels.rgba').read_bytes(), (after / 'pixels.rgba').read_bytes()
    changed = sum(x != y for x, y in zip(a, b)) + abs(len(a) - len(b))
    return {'pass': not metadata_differences and a == b, 'metadata_differences': metadata_differences,
            'changed_bytes': changed, 'before_bytes': len(a), 'after_bytes': len(b),
            'before_sha256': hashlib.sha256(a).hexdigest(), 'after_sha256': hashlib.sha256(b).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='store_true', help='Requires parent-granted heavy slot; compile and execute')
    parser.add_argument('--output', type=Path, default=HERE / 'parity-capture')
    args = parser.parse_args()
    fixture = TREES[0][1] / 'fixtures/raw/sony-arw.ARW'
    harness_hash = digest(HARNESS)
    states = {name: source_state(root, pin) for name, root, pin, _ in TREES}
    plan = {'status': 'prepared_only', 'harness_sha256': harness_hash, 'fixture_sha256': digest(fixture),
            'settings': {'exposures': [0, 1], 'upright': 'auto', 'process': {'family': 'native', 'revision': 2},
                         'viewport': [1280, 900], 'output': 'RGBA8', 'interactive': False},
            'trees': [{'name': name, 'root': str(root), 'pin': pin,
                       'resolved_pin': subprocess.check_output(['git', 'rev-parse', pin], cwd=root).decode().strip(),
                       'observed_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root).decode().strip(),
                       'target': str(target),
                       'temporary_test': str(root / f'crates/tessera-ffi/tests/{TEST}.rs'),
                       'build_command': COMMAND} for name, root, pin, target in TREES],
            'limits': ['actual baseline per-frame residency unavailable', 'fresh processes and fresh engine/cache state; OS/driver caches not purged',
                       'settled output pixel parity only; no presentation or P11 latency measurement']}
    if not args.run:
        print(json.dumps(plan, indent=2))
        return
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    plan.update(status='running', source_sha256=states, executions=[])
    report = output / 'report.json'
    installed = []
    try:
        executables = {}
        for name, root, pin, target in TREES:
            test_path = root / f'crates/tessera-ffi/tests/{TEST}.rs'
            with test_path.open('xb') as file:
                file.write(HARNESS.read_bytes())
            installed.append(test_path)
            env = dict(os.environ, CARGO_TARGET_DIR=str(target), MACOSX_DEPLOYMENT_TARGET='15.0', RUST_TEST_THREADS='1')
            with (output / f'{name}-build.jsonl').open('w') as stdout, (output / f'{name}-build.stderr.log').open('w') as stderr:
                subprocess.run(COMMAND, cwd=root, env=env, stdout=stdout, stderr=stderr, check=True)
            for line in (output / f'{name}-build.jsonl').read_text().splitlines():
                artifact = json.loads(line)
                if artifact.get('reason') == 'compiler-artifact' and artifact['target']['name'] == TEST and artifact.get('executable'):
                    executables[name] = artifact['executable']
            if name not in executables:
                raise RuntimeError(f'{name}: no test executable reported')
            if source_state(root, pin) != states[name]:
                raise RuntimeError(f'{name}: tracked source changed during compilation')
        for name, root, pin, _ in TREES:
            for exposure in (0, 1):
                case = output / f'{name}-exposure-{exposure}'
                env = dict(os.environ, TESSERA_RENDER_BACKEND='gpu', RUST_TEST_THREADS='1',
                           M2_58_PARITY_FIXTURE=str(fixture), M2_58_PARITY_OUTPUT=str(case),
                           M2_58_PARITY_EXPOSURE=str(exposure))
                command = [executables[name], '--exact', 'capture_settled_upright_pixels', '--nocapture']
                start = time.monotonic()
                load = os.getloadavg()
                with (output / f'{name}-exposure-{exposure}.log').open('w') as log:
                    result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=600)
                plan['executions'].append({'name': name, 'exposure': exposure, 'command': command,
                    'executable_sha256': digest(Path(executables[name])), 'exit_code': result.returncode,
                    'elapsed_seconds': time.monotonic() - start, 'load_before': load, 'load_after': os.getloadavg()})
                report.write_text(json.dumps(plan, indent=2) + '\n')
                result.check_returncode()
            if source_state(root, pin) != states[name]:
                raise RuntimeError(f'{name}: tracked source changed during execution')
        plan['comparisons'] = {str(exposure): compare(output / f'before-exposure-{exposure}', output / f'after-exposure-{exposure}')
                               for exposure in (0, 1)}
        plan['status'] = 'pass' if all(item['pass'] for item in plan['comparisons'].values()) else 'fail'
    except BaseException as error:
        plan.update(status='incomplete', error=str(error))
        raise
    finally:
        for path in installed:
            try:
                if digest(path) != harness_hash:
                    raise RuntimeError(f'Changed temporary file retained: {path}')
                path.unlink()
            except Exception as error:
                plan.update(status='incomplete')
                plan.setdefault('cleanup_errors', []).append(str(error))
        plan['tracked_source_unchanged'] = {}
        for name, root, pin, _ in TREES:
            try:
                unchanged = source_state(root, pin) == states[name]
                plan['tracked_source_unchanged'][name] = unchanged
                if not unchanged:
                    plan.update(status='incomplete')
            except Exception as error:
                plan.update(status='incomplete')
                plan['tracked_source_unchanged'][name] = False
                plan.setdefault('verification_errors', []).append(str(error))
        report.write_text(json.dumps(plan, indent=2) + '\n')
    print(json.dumps({'status': plan['status'], 'comparisons': plan['comparisons']}, indent=2))
    if plan['status'] != 'pass':
        raise SystemExit(1)


if __name__ == '__main__':
    main()
