#!/usr/bin/env python3
"""Resolve a READY.md merge conflict: keep every entry once, prefer the '(merged)' form."""
import re, subprocess
p = 'tools/orchestrate/wp/READY.md'
lines = open(p).read().splitlines()
out, index = [], {}
for line in lines:
    if re.match(r'^(<<<<<<<|=======|>>>>>>>)', line):
        continue
    key = re.sub(r'^- \(merged\) ', '- ', line)
    if line.strip() and key in index:
        if line.startswith('- (merged) '):
            out[index[key]] = line
        continue
    if line.strip():
        index[key] = len(out)
    out.append(line)
open(p, 'w').write('\n'.join(out) + '\n')
subprocess.check_call(['git', 'add', p])
print('READY.md resolved')
