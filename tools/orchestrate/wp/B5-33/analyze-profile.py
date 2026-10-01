#!/usr/bin/env python3
"""Summarize exported xctrace time-profile XML, resolving its interned references."""
import collections
import json
import sys
import xml.etree.ElementTree as ET

frames, threads, stacks = {}, {}, {}
counts = collections.Counter()
main_names = collections.Counter()
main_own = collections.Counter()
display_stacks = collections.Counter()
for _, row in ET.iterparse(sys.argv[1], events=('end',)):
    if row.tag != 'row':
        continue
    for frame in row.iter('frame'):
        if 'id' in frame.attrib:
            frames[frame.attrib['id']] = frame.attrib.get('name', '')
    thread = row.find('thread')
    if thread is None:
        row.clear()
        continue
    if 'id' in thread.attrib:
        threads[thread.attrib['id']] = thread.attrib.get('fmt', '')
    label = threads.get(thread.attrib.get('ref', thread.attrib.get('id')), '')
    backtrace = row.find('backtrace')
    if backtrace is None:
        row.clear()
        continue
    if 'id' in backtrace.attrib:
        stacks[backtrace.attrib['id']] = tuple(frames.get(f.attrib.get('ref', f.attrib.get('id')), '')
                                               for f in backtrace.findall('frame'))
    names = stacks.get(backtrace.attrib.get('ref', backtrace.attrib.get('id')), ())
    if '(Tessera, pid:' not in label:
        row.clear()
        continue
    main = 'Main Thread' in label
    counts['all_samples'] += 1
    counts['main_samples' if main else 'worker_samples'] += 1
    if main:
        main_names.update(set(names))
        if names:
            main_own[names[0]] += 1
        if any('NSViewBackingLayer display' in n for n in names):
            display_stacks[names] += 1
    if any('docflatexport_run' in n or 'DocumentFlatExport.run' in n for n in names):
        counts['export_run_main_samples' if main else 'export_run_worker_samples'] += 1
    row.clear()
print(json.dumps({'counts': dict(counts), 'main_inclusive': main_names.most_common(60),
                  'main_own': main_own.most_common(40),
                  'main_export_ui_symbols': [(n, c) for n, c in main_names.most_common()
                                             if any(s in n for s in ['FlatExport', 'DocumentWorkspace', 'NSProgressIndicator'])],
                  'main_display_stacks': [{'samples': c, 'stack': n} for n, c in display_stacks.most_common(5)]}, indent=2))
