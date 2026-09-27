#!/usr/bin/env python3
"""Background app diagnostics. Exit 2 means the full P01 oracle is not satisfied, NOT a latency pass."""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]


def percentile(values, p):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * p) - 1)] if values else None


def summarize(trace):
    events = trace['events']
    spans = [e['durationMs'] for e in events if e.get('mainThread') and 'durationMs' in e]
    enqueue, presented = {}, {}
    for e in sorted(events, key=lambda e: e.get('time', 0)):
        key = (e.get('session'), e.get('generation'), e.get('level'))
        if e['name'] == 'callback_enqueue':
            enqueue.setdefault(key, e['time'])
        if e['name'] == 'drawable_presented':
            presented.setdefault(key, e['time'])
    delays = [(end - enqueue[key]) * 1000 for key, end in presented.items()
              if key in enqueue and end >= enqueue[key]]
    by_span = {}
    for e in events:
        if e.get('mainThread') and 'durationMs' in e:
            by_span.setdefault(e['name'], []).append(e['durationMs'])
    return {
        'input_updates': sum(e['name'] == 'input' for e in events),
        'presented_generations': len({key[:2] for key in presented}),
        'main_instrumented_span_p95_ms': percentile(spans, .95),
        'main_span_p95_by_name_ms': {k: percentile(v, .95) for k, v in by_span.items()},
        'callback_to_present_p50_ms': percentile(delays, .5),
        'callback_to_present_p95_ms': percentile(delays, .95),
        'input_to_present_p50_ms': None,
        'input_to_present_p95_ms': None,
        'engine_sink_p95_ms': percentile([e['engineSinkMs'] for e in events
                                        if e['name'] == 'callback_enqueue' and 'engineSinkMs' in e], .95),
        'loupe_resource_spans': [e for e in events if e['name'] == 'loupe_resources_end'],
        'dropped': trace['dropped'],
        'p01_complete': False,
        'limitations': ['FFI does not expose causal input/generation mapping, job dequeue or actual residency.',
                        'Instrumented main spans include nested spans, not all main-thread tasks.',
                        'Occluded background windows may not present. Sink time is not input-to-display.'],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=ROOT / 'apps/mac/build/Tessera.app')
    parser.add_argument('--output', type=Path, required=True, help='new run directory inside this worktree')
    parser.add_argument('--fixture', type=Path, help='one RAW file (copied before edits)')
    parser.add_argument('--grid-only', action='store_true')
    parser.add_argument('--analyze', type=Path)
    args = parser.parse_args()
    if args.analyze:
        print(json.dumps(summarize(json.loads(args.analyze.read_text())), indent=2))
        return 2
    out = args.output.resolve()
    out.relative_to(ROOT)
    out.mkdir(parents=True, exist_ok=False)
    for flag in ('TESSERA_DOC_FRAME_LOG', 'TESSERA_SELFTEST_VERBOSE'):
        if flag in os.environ:
            raise RuntimeError(f'{flag} must be unset for timing')
    subprocess.run([sys.executable, str(ROOT / 'apps/mac/Support/provenance.py'),
                    'verify', '--app', str(args.app.resolve())], check=True)
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(args.app.resolve())], check=True)
    provenance = json.loads((args.app / 'Contents/Resources/build-provenance.json').read_text())
    if not args.grid_only and not args.fixture:
        raise RuntimeError('--fixture RAW is required (or --grid-only)')
    probe = out / 'foreground-probe'
    subprocess.run(['swiftc', str(Path(__file__).with_name('ForegroundProbe.swift')), '-o', str(probe)], check=True)
    def foreground():
        return int(subprocess.check_output([str(probe)], text=True).strip())
    initial = foreground()
    if initial <= 0:
        raise RuntimeError('Cannot determine foreground application; refusing launch')
    trace_path = out / 'trace.json'
    launch = ['open', '-g', '-n', '-a', str(args.app.resolve()),
              '--stdout', str(out / 'app-stdout.log'), '--stderr', str(out / 'app-stderr.log'),
              '--args', '--nonactivating',
              '--timing-output', str(trace_path), '--app-dir', str(out / 'state')]
    if args.grid_only:
        launch += ['--timing-grid-only', '--stub', '1000']
    else:
        fixture_dir = out / 'fixture'
        fixture_dir.mkdir()
        shutil.copy2(args.fixture, fixture_dir / args.fixture.name)
        launch += ['--folder', str(fixture_dir), '--timing-selftest', '--develop-selftest']
    (out / 'run.json').write_text(json.dumps({'command': launch, 'foreground_before': initial,
                                            'frame_logging': 'off'}, indent=2))
    subprocess.run(launch, check=True)
    samples = [initial]
    deadline = time.monotonic() + 150
    while time.monotonic() < deadline and not trace_path.exists():
        samples.append(foreground())
        time.sleep(.25)
    samples.append(foreground())
    (out / 'foreground.json').write_text(json.dumps(samples))
    if not trace_path.exists():
        raise RuntimeError('Timed out without trace; no latency result is claimed')
    trace = json.loads(trace_path.read_text())
    result = summarize(trace)
    result['provenance'] = {key: provenance[key] for key in
                            ('commit', 'configuration', 'source_sha256', 'archive_sha256', 'bindings')}
    result['foreground_unchanged'] = all(pid == initial for pid in samples)
    result['selftest_complete'] = any(e['name'] == 'selftest_complete' for e in trace['events'])
    result['grid_only'] = args.grid_only
    result['grid_appeared'] = any(e['name'] == 'grid_appeared' for e in trace['events'])
    result['frame_logging_enabled'] = any(e['name'] == 'frame_logging_enabled' for e in trace['events'])
    (out / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    if (not result['foreground_unchanged'] or not result['selftest_complete']
            or not result['grid_appeared'] or result['dropped'] or result['frame_logging_enabled']):
        return 1
    if args.grid_only:
        return 0 if not result['loupe_resource_spans'] else 1
    return 2  # refuse a false P01 pass while its Rust instrumentation seam is unavailable


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f'Timing run failed: {error}', file=sys.stderr)
        sys.exit(1)
