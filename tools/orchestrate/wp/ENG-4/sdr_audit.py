#!/usr/bin/env python3
"""Replay pinned CPU tone through the actual SDR renderer and verify every changed byte."""
import argparse
import os
import json
import hashlib
from pathlib import Path
import subprocess

BASE = 'f6b572ba'
ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
source = subprocess.check_output(['git', 'show', f'{BASE}:crates/pipeline-cpu/src/lib.rs'], cwd=ROOT, text=True)
a = source.index('pub fn tone(')
b = source.index('\nfn luminance(', a)
old_tone = source[a:b].replace('pub fn tone(', 'fn baseline_tone(', 1)
test = ROOT / 'crates/pipeline-gpu/tests/eng4_sdr_audit.rs'
assert not test.exists(), f'Refusing to overwrite {test}'
test.write_text((HERE / 'sdr_audit.rs').read_text().replace('/* BASELINE_TONE */', old_tone))
try:
    env = os.environ.copy()
    env['ENG4_AUDIT_DIR'] = str(args.output.resolve())
    subprocess.run(['cargo', 'test', '--locked', '--release', '-p', 'pipeline-gpu', '--test', 'eng4_sdr_audit', '--', '--nocapture'], cwd=ROOT, env=env, check=True)
finally:
    test.unlink()

report_path = args.output / 'sdr-report.json'
report = json.loads(report_path.read_text())
for row in report:
    for side in ['before', 'after']:
        capture = args.output / f"sdr-{row['case']}-{side}.planar-tiles.u8"
        row[f'{side}_sha256'] = hashlib.sha256(capture.read_bytes()).hexdigest()
report_path.write_text(json.dumps(report, indent=2) + '\n')
