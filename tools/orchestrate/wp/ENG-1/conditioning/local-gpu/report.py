#!/usr/bin/env python3
"""Report every Tone CPU/GPU pixel delta and the ENG-1b support predicate."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import struct


def image(path):
    data = path.read_bytes()
    w, h = struct.unpack('<II', data[:8])
    values = struct.unpack(f'<{3*w*h}f', data[8:])
    assert all(__import__('math').isfinite(v) for v in values)
    return w, h, values


def analyze(capture, seeds, output):
    w, h, cpu = image(capture / 'cpu.f32')
    wg, hg, gpu = image(capture / 'gpu.f32')
    wi, hi, src = image(capture / 'input.f32')
    assert (w, h) == (wg, hg) == (wi, hi)
    n = w * h
    failing = []
    max_diff = 0
    # Deterministic gzip header; all pixels, including exact matches, are dumped.
    with output.open('wb') as raw:
        with gzip.GzipFile(filename='', fileobj=raw, mode='wb', mtime=0) as gz:
            gz.write(b'pixel,x,y,gpu_minus_cpu_r,gpu_minus_cpu_g,gpu_minus_cpu_b\n')
            for i in range(n):
                delta = [gpu[c*n+i]-cpu[c*n+i] for c in range(3)]
                peak = max(map(abs, delta))
                max_diff = max(max_diff, peak)
                gz.write((f'{i},{i%w},{i//w},' + ','.join(map(repr, delta)) + '\n').encode())
                if peak > 1e-4:
                    distance = min((max(abs(i%w-s%w), abs(i//w-s//w)) for s in seeds), default=None)
                    failing.append(dict(pixel=i, x=i%w, y=i//w,
                        input_rgb=[src[c*n+i] for c in range(3)],
                        cpu_rgb=[cpu[c*n+i] for c in range(3)],
                        gpu_rgb=[gpu[c*n+i] for c in range(3)],
                        max_absolute_delta=peak, nearest_seed_distance=distance,
                        predicate=distance is not None and distance <= 11))
    return dict(width=w, height=h, pixels=n, max_absolute_delta=max_diff,
                pixels_above_1e_4=len(failing), failing_pixels=failing,
                outside_predicate=[r['pixel'] for r in failing if not r['predicate']],
                capture_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(capture.glob('*.f32'))},
                diff_file=output.name, diff_sha256=hashlib.sha256(output.read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--trace', type=Path, required=True)
    parser.add_argument('--after', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    seeds = {}
    globals_ = []
    for line in args.trace.read_text().splitlines():
        fields = line.split('\t')
        if fields[0] == 'seed':
            _, w, h, i, bits = fields
            assert (int(w), int(h)) == image(args.before / 'input.f32')[:2]
            lum = struct.unpack('<f', struct.pack('<I', int(bits, 16)))[0]
            assert 0 < abs(lum) < 1e-3
            seeds.setdefault(int(i), dict(pixel=int(i), pre_fix_luminance=lum, bits=bits))
        elif fields[0] == 'global':
            globals_.append(fields[1:])
    report = dict(bound=1e-4, support_radius=11, seeds=list(seeds.values()),
                  pre_fix_dehaze_globals=globals_,
                  before=analyze(args.before, seeds, args.output / 'before-diff.csv.gz'))
    if args.after:
        report['after'] = analyze(args.after, seeds, args.output / 'after-diff.csv.gz')
    (args.output / 'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
