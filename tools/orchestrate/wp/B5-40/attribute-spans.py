#!/usr/bin/env python3
"""Resolve xctrace interned stacks and match samples to measured busy intervals."""
import collections
import ctypes
import datetime
import json
import pathlib
import sys
import time
import xml.etree.ElementTree as ET

folder = pathlib.Path(sys.argv[1])
lib = ctypes.CDLL('/usr/lib/libSystem.B.dylib')
class Timebase(ctypes.Structure):
    _fields_ = [('numer', ctypes.c_uint32), ('denom', ctypes.c_uint32)]
base = Timebase()
lib.mach_timebase_info(ctypes.byref(base))
lib.mach_absolute_time.restype = ctypes.c_uint64
anchor = dict(mach_seconds=lib.mach_absolute_time() * base.numer / base.denom / 1e9, epoch=time.time())
anchor_path = folder / 'clock-anchor.json'
if anchor_path.exists():
    anchor = json.loads(anchor_path.read_text())
else:
    anchor_path.write_text(json.dumps(anchor, indent=2) + '\n')
wall_offset = anchor['epoch'] - anchor['mach_seconds']
toc = ET.parse(folder / 'toc.xml')
start = datetime.datetime.fromisoformat(toc.findtext('.//summary/start-date')).timestamp()
events = json.loads((folder / 'spans.json').read_text())['events']
intervals = []
for event in events:
    if event['name'].startswith('export_flat_main_busy'):
        end = event['time'] + wall_offset - start
        intervals.append(dict(name=event['name'], duration_ms=event['durationMs'],
                              start_s=end - event['durationMs'] / 1000, end_s=end))
root = ET.parse(folder / 'time-profile.xml').getroot()
refs = {node.attrib['id']: node for node in root.iter() if 'id' in node.attrib}
def resolve(node):
    return refs[node.attrib['ref']] if node is not None and 'ref' in node.attrib else node
samples = []
for row in root.iter('row'):
    thread = resolve(row.find('thread'))
    if thread is None or 'Main Thread' not in thread.attrib.get('fmt', ''):
        continue
    stamp = resolve(row.find('sample-time'))
    backtrace = resolve(row.find('backtrace'))
    if stamp is None or backtrace is None:
        continue
    names = tuple(resolve(f).attrib.get('name', '') for f in backtrace.findall('frame'))
    samples.append((float(stamp.text) / 1e9, names))
load_path = folder / 'load-during.jsonl'
loads = [json.loads(line) for line in load_path.read_text().splitlines()] if load_path.exists() else []
for interval in intervals:
    # TOC start-date is rounded to milliseconds; retain 1 ms at both boundaries.
    selected = [(t, stack) for t, stack in samples if interval['start_s'] - .001 <= t <= interval['end_s'] + .001]
    inclusive = collections.Counter(name for _, stack in selected for name in set(stack))
    stacks = collections.Counter(stack for _, stack in selected)
    interval['main_samples'] = len(selected)
    if loads:
        midpoint = start + (interval['start_s'] + interval['end_s']) / 2
        nearest = min(loads, key=lambda row: abs(datetime.datetime.fromisoformat(row['time']).timestamp() - midpoint))
        interval['nearest_load_sample'] = nearest
    interval['inclusive'] = inclusive.most_common(35)
    interval['ui_symbols'] = [(name, count) for name, count in inclusive.most_common()
                              if any(token in name for token in ('NSView', 'NSWindow', 'NSText', 'NSButton', 'NSProgress', 'ViewGraph', 'FlatExport', 'DocumentWorkspace', 'CA::'))][:35]
    interval['stacks'] = [dict(samples=count, stack=list(stack)) for stack, count in stacks.most_common(6)]
print(json.dumps(dict(main_samples=len(samples), clock_tolerance_ms=1, intervals=intervals), indent=2))
