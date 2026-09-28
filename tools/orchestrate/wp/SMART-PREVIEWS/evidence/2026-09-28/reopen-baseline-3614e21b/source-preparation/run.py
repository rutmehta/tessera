#!/usr/bin/env python3
"""Source-only baseline runner. Execute only after independent review and lane grant."""
import argparse, array, hashlib, json, os, pathlib, statistics, subprocess, sys
p = argparse.ArgumentParser()
p.add_argument('--execute', action='store_true')
p.add_argument('--checkout', required=True)
p.add_argument('--fixture', required=True)
p.add_argument('--out', required=True)
a = p.parse_args()
if not a.execute:
    sys.exit('No workload started. --execute requires coordinator runtime lane grant.')
root = pathlib.Path(a.checkout).resolve()
fixture = pathlib.Path(a.fixture).resolve()
out = pathlib.Path(a.out)
out.mkdir(parents=True, exist_ok=False)
manifest_path = pathlib.Path(__file__).with_name('thresholds.json')
manifest = json.loads(manifest_path.read_text())
def sha(path):
    with open(path, 'rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()
def freeze():
    names = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard',
                                     'crates', 'Cargo.toml', 'Cargo.lock'], cwd=root).decode().split('\0')
    return {'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root).decode().strip(),
            'files': {n: sha(root / n) for n in names if n and (root / n).is_file()},
            'fixture_sha256': sha(fixture), 'runner_sha256': sha(pathlib.Path(__file__)),
            'thresholds_sha256': sha(manifest_path)}
before = freeze()
assert before['fixture_sha256'] == manifest['fixture_sha256'], 'unexpected fixture; amend preregistration before execution'
(out / 'before.json').write_text(json.dumps(before, indent=2))
(out / 'thresholds.json').write_text(json.dumps(manifest, indent=2))
summary = []
# Fixed baseline order, not candidate comparison. Each process has one Engine and six opens.
for fmt, route in [('sdr', 'proxy-auto'), ('sdr', 'proxy-cpu'), ('edr', 'proxy-auto'), ('edr', 'proxy-cpu')]:
    dest = out / f'{route}-{fmt}'
    dest.mkdir()
    env = dict(os.environ)
    env.update(CARGO_TARGET_DIR='/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated',
               CARGO_BUILD_JOBS='2', MACOSX_DEPLOYMENT_TARGET='15.0', TESSERA_QUALIFY_ROUTE=route,
               TESSERA_QUALIFY_FORMAT=fmt, TESSERA_QUALIFY_OUT=str(dest.resolve()),
               TESSERA_SMART_PREVIEW_RAW=str(fixture), TESSERA_RENDER_BACKEND='cpu' if route=='proxy-cpu' else '')
    env.pop('TESSERA_SMART_PREVIEW_GPU', None)
    cmd = ['cargo', 'test', '-p', 'tessera-ffi', '--lib', '--release',
           'engine_same_engine_unchanged_proxy_reopen_baseline', '--', '--ignored', '--nocapture', '--test-threads=1']
    (dest / 'command.json').write_text(json.dumps({'argv':cmd, 'env':{k:v for k,v in env.items() if k.startswith(('CARGO_', 'TESSERA_', 'MACOSX_'))}}, indent=2))
    with open(dest / 'run.log', 'w') as log:
        result = subprocess.run(cmd, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
    (dest / 'exit').write_text(f'{result.returncode}\n')
    after = freeze()
    (dest / 'after.json').write_text(json.dumps(after, indent=2))
    (out / 'after.json').write_text(json.dumps(after, indent=2))
    (dest / 'freeze.json').write_text(json.dumps({'equal': before==after}))
    assert before == after, 'source, runner, thresholds or read-only fixture changed'
    assert result.returncode == 0, f'direct failure preserved: {dest}'
    data = json.loads((dest / 'reopen-results.json').read_text())
    rows = data['rows']
    assert data['engine_count'] == 1 and len(rows) == 6
    assert [r['cycle'] for r in rows] == list(range(6))
    assert all(r['route']==route and r['format']==fmt and r['released'] and r['pixel_readback_bytes']==0 for r in rows)
    assert all(r['settings']==rows[0]['settings'] for r in rows)
    samples = [r['open_to_final_callback_ms'] for r in rows[1:]]
    cv = statistics.pstdev(samples) / statistics.fmean(samples)
    summary.append({'route':route, 'format':fmt, 'reopen_median_ms':statistics.median(samples),
                    'initial_ms':rows[0]['open_to_final_callback_ms'], 'reopen_cv':cv,
                    'baseline_stable':cv <= manifest['performance']['baseline_cv_max'],
                    'selected_backends':[r['backend'] for r in rows],
                    'geometry':[[r['level'],r['dimensions']] for r in rows]})
    (out / 'summary.json').write_text(json.dumps(summary, indent=2))
# Pixel comparison is outside all timed intervals. Never compare unlike geometry.
comparisons = []
for fmt in ('sdr', 'edr'):
    auto_dir, cpu_dir = out / f'proxy-auto-{fmt}', out / f'proxy-cpu-{fmt}'
    auto_rows = json.loads((auto_dir / 'reopen-results.json').read_text())['rows']
    cpu_rows = json.loads((cpu_dir / 'reopen-results.json').read_text())['rows']
    for ar, cr in zip(auto_rows, cpu_rows, strict=True):
        comparable = ar['settings']==cr['settings'] and ar['dimensions']==cr['dimensions'] and ar['level']==cr['level']
        record = {'format':fmt, 'cycle':ar['cycle'], 'comparable':comparable}
        if comparable:
            def pixels(directory, row):
                values = array.array('f')
                values.frombytes((directory / row['pixel_file']).read_bytes())
                if sys.byteorder != 'little': values.byteswap()
                assert len(values)==row['dimensions'][0]*row['dimensions'][1]*3
                return values
            ap, cp = pixels(auto_dir, ar), pixels(cpu_dir, cr)
            errors = [abs(a-c) for a,c in zip(ap,cp,strict=True)]
            record['max_absolute_error'] = max(errors)
            record['passed'] = all(error <= (manifest['fidelity']['sdr_max_absolute_per_channel'] if fmt=='sdr' else
                manifest['fidelity']['edr_absolute'] + manifest['fidelity']['edr_relative']*abs(ref))
                for error,ref in zip(errors,cp,strict=True))
        comparisons.append(record)
(out / 'fidelity.json').write_text(json.dumps(comparisons, indent=2))
assert all(r.get('passed', True) for r in comparisons), 'fidelity failed; raw pixels preserved'
assert all(any(r['format']==fmt and r['comparable'] for r in comparisons) for fmt in ('sdr','edr')), 'no comparable geometry'
assert all(r['baseline_stable'] for r in summary), 'Baseline unstable: no candidate timing authorized by this run'
print('Baseline captured; no cache, speedup, cold-filesystem or physical-screen claim.')
