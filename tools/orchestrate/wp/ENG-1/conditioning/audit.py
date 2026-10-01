#!/usr/bin/env python3
"""Rebuild pre-fix and fixed CPU Develop outputs in a disposable source tree.

Never writes goldens. Captures exact f32 RGBA, active pre-fix divisors, and
image-global dehaze state. Only instrumentation is injected into production
code, in the disposable tree; the legacy operator is read from d0172365.
Run with the lane Cargo environment: python3 .../audit.py --output /tmp/eng1b
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[5]
TONE = Path('crates/pipeline-cpu/src/tone_extra.rs')
BASE = 'd01723659a7a6193ad1e323b311a56f72f7411f7'
RADIUS = 11


def replace_once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new)


def instrument(text):
    text = replace_once(text, '\n        let gain = finite(', '''
        if y[i].abs() < 1e-3 && out != z[i] {
            eng1_trace(&format!("seed\\t{w}\\t{h}\\t{i}\\t{:08x}", y[i].to_bits()));
        }
        let gain = finite(''')
    text = replace_once(text, '    let normalized: Vec<_> = rgb', '''    eng1_trace(&format!("global\\t{:08x}\\t{:08x}\\t{:08x}\\t{:08x}",
        air[0].to_bits(), air[1].to_bits(), air[2].to_bits(), confidence.to_bits()));
    let normalized: Vec<_> = rgb''')
    return text + '''
fn eng1_trace(line: &str) {
    use std::io::Write;
    if let Some(dir) = std::env::var_os("ENG1_CAPTURE") {
        let mut f = std::fs::OpenOptions::new().create(true).append(true)
            .open(std::path::PathBuf::from(dir).join("trace.tsv")).unwrap();
        writeln!(f, "{line}").unwrap();
    }
}
'''


def compare(before, after, seeds, width, radius=RADIUS):
    assert len(before) == len(after)
    changed = []
    outside = []
    evidence = []
    max_delta = 0.0
    for i, (a, b) in enumerate(zip(before, after)):
        assert all(math.isfinite(v) for v in (*a, *b))
        assert struct.pack("<f", a[3]) == struct.pack("<f", b[3]), "BLOCKER: alpha changed"
        if struct.pack("<4f", *a) == struct.pack("<4f", *b):
            continue
        changed.append(i)
        max_delta = max(max_delta, max(abs(x-y) for x, y in zip(a, b)))
        distance = min((max(abs(i % width - s % width), abs(i // width - s // width)) for s in seeds), default=None)
        allowed = distance is not None and distance <= radius
        evidence.append(dict(pixel=i, distance=distance, predicate=allowed))
        if not allowed:
            outside.append(i)
    return dict(changed_pixels=len(changed), max_encoded_delta=max_delta,
                outside_predicate=outside, changed_pixel_evidence=evidence)


def read_rgba(path):
    return list(struct.iter_unpack('<4f', path.read_bytes()))


def audit_captures(out):
    rows = []
    for old in sorted((out / 'before').glob('*.rgba')):
        new = out / 'after' / old.name
        trace = old.with_suffix('.tsv').read_text().splitlines()
        assert trace == new.with_suffix('.tsv').read_text().splitlines(), 'BLOCKER: seed/global state differs'
        trace_source = old.stem
        # camera_raw::evaluate caches the developed image independently of opacity.
        # The immediately preceding full-amount evaluation owns the shared trace.
        if old.stem.endswith('-0.35') and not trace:
            trace_source = old.stem.removesuffix('-0.35') + '-1'
            trace = old.with_name(trace_source + '.tsv').read_text().splitlines()
            assert trace == new.with_name(trace_source + '.tsv').read_text().splitlines()
        seeds = []
        divisors = []
        globals_ = []
        for line in trace:
            fields = line.split('\t')
            if fields[0] == 'global':
                globals_.append(fields[1:])
            if fields[0] == 'seed':
                _, w, h, i, bits = fields
                assert (int(w), int(h)) == (259, 17)
                luminance = struct.unpack('<f', struct.pack('<I', int(bits, 16)))[0]
                assert 0 < abs(luminance) < 1e-3
                seeds.append(int(i))
                divisors.append(dict(pixel=int(i), luminance=luminance, bits=bits))
        row = dict(golden=old.stem,
                   before_sha256=hashlib.sha256(old.read_bytes()).hexdigest(),
                   after_sha256=hashlib.sha256(new.read_bytes()).hexdigest(), pixels=4403, seed_pixels=seeds, pre_fix_divisors=divisors,
                   unchanged_dehaze_global_bits=globals_, support_radius=RADIUS, trace_source=trace_source,
                   **compare(read_rgba(old), read_rgba(new), seeds, 259))
        if not old.stem.endswith('-0'):
            assert len(seeds) == 1 and len(globals_) == 1, 'missing active path trace'
            assert row['changed_pixels'] > 0, 'missing conditioning delta'
        rows.append(row)
    assert len(rows) == 9
    (out / 'report.json').write_text(json.dumps(rows, indent=2) + '\n')
    print(json.dumps([{k:v for k,v in r.items() if k != "changed_pixel_evidence"} for r in rows], indent=2))
    assert all(not r['outside_predicate'] for r in rows), 'BLOCKER: changed pixels outside support'


def audit_raw_captures(out):
    raw_rows = []
    for old in sorted((out / 'before').glob('*.rgb8')):
        new = out / 'after' / old.name
        a, b = old.read_bytes(), new.read_bytes()
        assert len(a) == len(b)
        changed = sum(a[i:i+3] != b[i:i+3] for i in range(0, len(a), 3))
        delta = max(abs(x-y) for x,y in zip(a,b))
        raw_rows.append(dict(golden=old.stem, pixels=len(a)//3, changed_pixels=changed,
                             max_encoded_delta=delta, outside_predicate=changed,
                             stored_png_changed_pixels=0, stored_png_max_delta=0))
    (out / 'raw-report.json').write_text(json.dumps(raw_rows, indent=2) + '\n')
    assert len(raw_rows) == 5
    assert all(r['changed_pixels'] == 0 for r in raw_rows), 'BLOCKER: RAW neutral presence has empty support'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--verify-captures', action='store_true', help='recheck existing captures and successful Cargo logs without rebuilding')
    args = parser.parse_args()
    out = args.output.resolve()
    if args.verify_captures:
        for label in ['before', 'after']:
            for test, count in [('camera_raw', 3), ('golden', 1)]:
                log = (out / f'{label}-{test}.log').read_text()
                assert f'test result: ok. {count} passed; 0 failed' in log
                assert 'skipping' not in log
        audit_captures(out)
        audit_raw_captures(out)
        return
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env.pop('PIPELINE_RAW_FIXTURES', None)
    env.pop('PIPELINE_GPU_ALL_FIXTURES', None)
    with tempfile.TemporaryDirectory(prefix='tessera-eng1b-') as temp:
        tree = Path(temp)
        archive = subprocess.check_output(['git', 'archive', 'HEAD'], cwd=ROOT)
        import io
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(tree, filter='data')
        # Include the working test harness, allowing the test-first run before commit.
        for rel in ['crates/filters/tests/camera_raw.rs',
                    'crates/filters/tests/support/conditioning_golden.rs',
                    'crates/pipeline-cpu/tests/golden.rs']:
            dest = tree / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / rel, dest)
        (tree / 'fixtures/raw').symlink_to((ROOT / 'fixtures/raw').resolve())
        legacy = subprocess.check_output(['git', 'show', f'{BASE}:{TONE}'], cwd=ROOT).decode()
        fixed = (ROOT / TONE).read_text()
        # Verify exactly the authorized production delta, not unrelated historical code.
        expected = legacy.replace('        let gain = finite(decode(out) / y[i]);',
                                  '''        let gain = finite(if y[i] >= PRESENCE_LUMA_FLOOR {
            decode(out) / y[i]
        } else {
            1.0 + (decode(out) - y[i]) / PRESENCE_LUMA_FLOOR
        });''')
        expected = expected.replace('if out == z[i] {', 'if y[i] >= PRESENCE_LUMA_FLOOR && out == z[i] {')
        start = fixed.index('// Scene-linear Rec.2020 luminance (white = 1). Signed RGB')
        end = fixed.index('fn presence(', start)
        assert fixed[:start] + fixed[end:] == expected
        for label, source in [('before', legacy), ('after', fixed)]:
            capture = out / label
            capture.mkdir()
            (tree / TONE).write_text(instrument(source))
            runenv = dict(env, ENG1_CAPTURE=str(capture))
            for package, test, name in [('filters', 'camera_raw', 'full_develop_matches_rgb_decode_golden'),
                                        ('pipeline-cpu', 'golden', 'raw_fixture_goldens')]:
                command = ['cargo', 'test', '--locked', '--release', '-p', package, '--test', test,
                           name, '--', '--nocapture', '--test-threads=1']
                with (out / f'{label}-{test}.log').open('w') as log:
                    result = subprocess.run(command, cwd=tree, env=runenv, stdout=log, stderr=subprocess.STDOUT)
                print(label, test, result.returncode, flush=True)
                assert result.returncode == 0, f'BLOCKER: {label} {test}; see log'
        audit_captures(out)
        audit_raw_captures(out)


if __name__ == '__main__':
    main()
