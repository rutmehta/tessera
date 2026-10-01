#!/usr/bin/env python3
"""Summarize the unchanged two-exports-per-fixture harness, including 1 Hz host load."""
import datetime
import json
import pathlib
import re
import statistics
import sys

root = pathlib.Path(__file__).resolve().parent / 'evidence'
anchor = json.loads((root / 'clock-anchor.json').read_text())
offset = anchor['epoch'] - anchor['mach_seconds']
pattern = re.compile(r'RESULT p16 (.*?) main-thread spans during Export Flat (\d+): n (\d+) p50 ([\d.]+) ms p95 ([\d.]+) ms max ([\d.]+) ms; export ([\d.]+) s')
runs = []
fixture_loads = {}
for name in sys.argv[1:]:
    folder = root / name
    trace = json.loads((folder / 'spans.json').read_text())
    events = sorted(trace['events'], key=lambda e: e['time'])
    starts = [e for e in events if e['name'] == 'export_flat_setup_start']
    ends = [e for e in events if e['name'] == 'export_flat_completion_end']
    loads = [json.loads(line) for line in (folder / 'load-during.jsonl').read_text().splitlines()]
    for sample in loads:
        sample['epoch'] = datetime.datetime.fromisoformat(sample['time']).timestamp()
    exports = []
    for i, match in enumerate(pattern.finditer((folder / 'stderr.log').read_text())):
        fixture, index, count, p50, p95, maximum, seconds = match.groups()
        start, end = starts[i]['time'], ends[i]['time']
        measured = [s['load1'] for s in loads if start + offset <= s['epoch'] <= end + offset]
        if not measured:
            measured = [min(loads, key=lambda s: abs(s['epoch'] - (start + offset)))['load1']]
        fixture_loads.setdefault(fixture, []).extend(measured)
        exports.append(dict(fixture=fixture, index=int(index), spans=int(count), p50_ms=float(p50),
                            p95_ms=float(p95), max_ms=float(maximum), wall_s=float(seconds),
                            load1_min=min(measured), load1_median=statistics.median(measured),
                            load1_max=max(measured), load_samples=len(measured)))
    named = {}
    for event in events:
        if event['name'].startswith('export_flat_') and event.get('durationMs') is not None:
            row = named.setdefault(event['name'], dict(count=0, max_ms=0, main_thread=event['mainThread']))
            row['count'] += 1
            row['max_ms'] = max(row['max_ms'], event['durationMs'])
    gaps = []
    for start, end in zip(starts, ends):
        ticks = [e['time'] for e in events if e['name'] == 'export_flat_progress_start'
                 and start['time'] <= e['time'] <= end['time']]
        gaps.extend((b - a) * 1000 for a, b in zip(ticks, ticks[1:]))
    snapshots = [e for e in events if e['name'] == 'export_flat_snapshot_start']
    runs.append(dict(run=name, launch_load1=loads[0]['load1'],
                     all_load1_min=min(s['load1'] for s in loads), all_load1_max=max(s['load1'] for s in loads),
                     zero_failures='done, 0 failure(s)' in (folder / 'stderr.log').read_text(),
                     dropped_events=trace['dropped'], exports=exports, named=named,
                     snapshot_count=len(snapshots), main_snapshot_count=sum(e['mainThread'] for e in snapshots),
                     minimum_progress_interval_ms=min(gaps) if gaps else None))
summary = {}
for fixture in sorted({e['fixture'] for r in runs for e in r['exports']}):
    per_run = [[e for e in r['exports'] if e['fixture'] == fixture] for r in runs]
    worst = [max(e['max_ms'] for e in group) for group in per_run if group]
    values = [e for group in per_run for e in group]
    summary[fixture] = dict(run_maxima_ms=worst, median_run_max_ms=statistics.median(worst),
                            worst_observed_ms=max(worst), median_export_s=statistics.median(e['wall_s'] for e in values),
                            load1_min=min(e['load1_min'] for e in values), load1_max=max(e['load1_max'] for e in values),
                            load1_sample_median=statistics.median(fixture_loads[fixture]),
                            load1_sample_p95=statistics.quantiles(fixture_loads[fixture], n=20, method='inclusive')[18])
print(json.dumps(dict(aggregation='Median of three launch-wise worst spans; both exports retained per fixture per launch. Wall time is median of all exports.',
                      load_clock='1 Hz samples; mach_absolute_time to wall-clock anchor, same host session',
                      fixtures=summary, runs=runs), indent=2))
