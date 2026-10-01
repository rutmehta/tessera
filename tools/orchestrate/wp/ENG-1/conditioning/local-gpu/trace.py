#!/usr/bin/env python3
"""Capture exact active pre-floor seeds in a disposable source tree.
Uses the ENG-1b tracer; neither workspace source nor references are modified.
Run with the lane Cargo environment and --output /tmp/eng1c-trace.
"""
import argparse
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('conditioning', HERE.parent / 'audit.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--candidate', action='store_true', help='validate the proposed local shader fix, without modifying this worktree')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='eng1c-trace-') as temp:
    tree = Path(temp)
    archive = subprocess.check_output(['git', 'archive', 'HEAD'], cwd=audit.ROOT)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(tree, filter='data')
    (tree / 'fixtures/raw').symlink_to((audit.ROOT / 'fixtures/raw').resolve())
    source = (tree / audit.TONE).read_text()
    # Bands and bypass decisions precede the only ENG-1 changed expression.
    # Restore just that expression, preserving all other current CPU code.
    legacy = source.replace('decode(out) / y[i].abs().max(PRESENCE_LUMA_FLOOR)',
                            'decode(out) / y[i]')
    assert legacy != source
    legacy = legacy.replace('const PRESENCE_LUMA_FLOOR: f32 = 1e-3;', '')
    if args.candidate:
        shader = tree / 'crates/pipeline-gpu/src/tone_local.wgsl'
        shader.write_text(shader.read_text().replace(
            'fn finite(v:f32)',
            '// Match CPU tone_extra.rs and resident presence.wgsl: 0.1% of white.\nconst PRESENCE_LUMA_FLOOR: f32 = 1e-3;\nfn finite(v:f32)').replace(
            'decode(adjusted)/lum', 'decode(adjusted)/max(abs(lum), PRESENCE_LUMA_FLOOR)'))
    else:
        (tree / audit.TONE).write_text(audit.instrument(legacy))
    env = dict(os.environ, ENG1_CAPTURE=str(out), ENG1C_CAPTURE=str(out))
    (out / 'trace.tsv').unlink(missing_ok=True)
    subprocess.run(['cargo', 'clean', '--release', '-p', 'pipeline-cpu', '-p', 'pipeline-gpu', '-p', 'filters'], cwd=tree, env=env, check=True)
    with (out / 'run.log').open('w') as log:
        result = subprocess.run(['cargo', 'test', '--locked', '--release', '-p', 'pipeline-gpu', '--test', 'fixtures', 'fixture_level3_tolerance_per_operator_and_output', '--', '--nocapture'], cwd=tree, env=env, stdout=log, stderr=subprocess.STDOUT)
    print('fixture exit:', result.returncode, flush=True)
    if args.candidate:
        assert result.returncode == 0
        failed = []
        for name, command in [
            ('release', ['cargo', 'test', '--locked', '-p', 'pipeline-cpu', '-p', 'pipeline-gpu', '-p', 'filters', '--release']),
            ('clippy', ['cargo', 'clippy', '--locked', '--all-targets', '-p', 'pipeline-cpu', '-p', 'pipeline-gpu', '-p', 'filters', '--', '-D', 'warnings']),
            ('fmt', ['cargo', 'fmt', '--all', '--', '--check']),
        ]:
            with (out / f'{name}.log').open('w') as log:
                gate = subprocess.run(command, cwd=tree, env=os.environ, stdout=log, stderr=subprocess.STDOUT)
            print(name, gate.returncode, flush=True)
            if gate.returncode:
                failed.append(name)
        assert not failed, f'failed candidate gates: {failed}'
    else:
        assert (out / 'trace.tsv').is_file()
