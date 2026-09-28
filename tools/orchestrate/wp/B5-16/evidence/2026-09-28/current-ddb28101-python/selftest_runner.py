#!/usr/bin/env python3
"""Document acceptance runner. Owns only its direct child process."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

TESTS = {
    'document': (900, ['--new-document', '--document-selftest', '@OUT@', '--document-selftest-hold', '0.3']),
    'tools': (900, ['--new-document', '--tools-selftest', '@OUT@', '--tools-selftest-hold', '0.3']),
    'filter': (900, ['--new-document', '--filter-selftest', '@OUT@', '--filter-selftest-hold', '0.3']),
    'retouch': (1500, ['--new-document', '--retouch-selftest=@OUT@', '--retouch-selftest-hold', '0.3']),
    'styles': (900, ['--new-document', '--styles-selftest', '@OUT@', '--styles-selftest-hold', '0.3']),
    'channels': (900, ['--new-document', 'ENV:TESSERA_CHANNELS_SELFTEST=@OUT@', 'ENV:TESSERA_CHANNELS_SELFTEST_HOLD=0.3']),
    'text': (1500, ['--new-document', 'ENV:TESSERA_TEXT_SELFTEST=@OUT@']),
    'vector': (1800, ['--new-document', '--vector-selftest=@OUT@']),
    'transform': (1800, ['--new-document', '--transform-selftest=@OUT@']),
}


def validate_log(text, prefix, returncode, timed_out=False):
    errors = []
    if timed_out:
        errors.append('timeout: child did not finish within its deadline')
    if returncode != 0:
        errors.append(f'child exit {returncode}')
    lines = text.splitlines()
    done = [line for line in lines if line.startswith(prefix + ': done')]
    if done != [prefix + ': done, 0 failure(s)']:
        errors.append('missing, duplicate, malformed or failing completion line')
    for line in lines:
        if re.search(r'\bFAIL(?:ED)?\b', line) or (
            line.startswith(prefix + ':') and 'did not start' in line.lower()
        ):
            errors.append(line)
    return errors


def stop_owned_child(child):
    # Popen retains this child's identity; no pgrep, process-name matching or group kill.
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


def capture_steps(text, prefix, output, evidence, seen):
    errors = []
    pattern = re.compile(re.escape(prefix) + r': step ([0-9]+[^ ]*) .*?(?:window-id|sheet) ([0-9]+)(?:\s|$)')
    for step, window in pattern.findall(text):
        if step in seen:
            continue
        seen.add(step)
        image = evidence / f'{step}.png'
        result = subprocess.run(['screencapture', '-x', '-o', '-l', window, str(image)],
                                capture_output=True, text=True, timeout=15)
        if result.returncode:
            errors.append(f'capture {step}: {result.stderr.strip()}')
        else:
            (output / ('ack-' + step.split('-')[0])).touch()
    return errors


def run_test(binary, name, arguments, timeout, scratch, evidence, fixtures):
    prefix = name + '-selftest'
    folder = scratch / name
    folder.mkdir()
    for part in ['app', 'folder', 'out']:
        (folder / part).mkdir()
    for fixture in fixtures:
        shutil.copy2(fixture, folder / 'folder' / fixture.name)
    evidence.mkdir(parents=True)
    environment = os.environ.copy()
    args = []
    for argument in arguments:
        argument = argument.replace('@OUT@', str(folder / 'out'))
        if argument.startswith('ENV:'):
            key, value = argument[4:].split('=', 1)
            environment[key] = value
        else:
            args.append(argument)
    log = evidence / 'stderr.log'
    timed_out = False
    capture_errors = []
    seen = set()
    # Direct bundle executable gives us the actual child PID and exit status.
    # --nonactivating requests accessory launch; individual app self-tests may still
    # raise windows. Use only an authorized isolated desktop validation session.
    with log.open('w') as stderr, (evidence / 'stdout.log').open('w') as stdout:
        child = subprocess.Popen([str(binary), '--nonactivating', '--app-dir', str(folder / 'app'),
                                  '--folder', str(folder / 'folder'), *args],
                                 env=environment, stdout=stdout, stderr=stderr)
        deadline = time.monotonic() + timeout
        try:
            while child.poll() is None:
                text = log.read_text(errors='replace')
                capture_errors.extend(capture_steps(text, prefix, folder / 'out', evidence, seen))
                if time.monotonic() >= deadline:
                    timed_out = True
                    break
                time.sleep(0.1)
        finally:
            stop_owned_child(child)
    errors = validate_log(log.read_text(errors='replace'), prefix, child.returncode, timed_out)
    errors.extend(capture_errors)
    (evidence / 'result.txt').write_text(('FAIL\n' + '\n'.join(errors) if errors else 'PASS') + '\n')
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[4])
    parser.add_argument('tests', nargs='*', choices=list(TESTS))
    args = parser.parse_args(argv)
    root = args.root.resolve()
    binary = root / 'apps/mac/build/Tessera.app/Contents/MacOS/Tessera'
    if not binary.is_file():
        parser.error(f'build the intended app first: {binary}')
    fixtures = [p for p in (root / 'fixtures/raw').glob('*') if p.is_file()]
    if not (root / 'fixtures/raw/sample.dng').is_file():
        parser.error('missing fixtures/raw/sample.dng')
    parent = Path(os.environ.get('SP', tempfile.gettempdir()))
    parent.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix='tessera-b516-selftests-', dir=parent))
    evidence = Path(os.environ.get('EV', str(root / 'tools/orchestrate/wp/B5-16/evidence/selftests'))) / scratch.name
    failed = False
    for name in args.tests or TESTS:
        timeout, arguments = TESTS[name]
        errors = run_test(binary, name, arguments, timeout, scratch, evidence / name, fixtures)
        print(f'[{name}] {"FAIL" if errors else "PASS"}: {evidence / name}', flush=True)
        for error in errors:
            print(error, flush=True)
        failed = failed or bool(errors)
    return int(failed)


if __name__ == '__main__':
    raise SystemExit(main())
