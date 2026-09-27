#!/usr/bin/env python3
"""Resolve a board.json merge conflict: union of cards by id.
Ours (main) wins for ids on both sides unless ours is not 'merged' and theirs is further along."""
import json, subprocess, sys
path = 'tools/orchestrate/board.json'
def side(n):
    return json.loads(subprocess.check_output(['git', 'show', f':{n}:{path}']))
ours, theirs = side(2), side(3)
def cards(d):
    if isinstance(d, list): return d, None
    k = next(k for k in ('items', 'packages', 'cards', 'wps') if k in d)
    return d[k], k
o, k = cards(ours); t, _ = cards(theirs)
rank = {'queued': 0, 'running': 1, 'fail': 2, 'escalate': 2, 'pass': 3, 'merged': 4}
by_id = {c['id']: c for c in o}
for c in t:
    mine = by_id.get(c['id'])
    if mine is None:
        o.append(c); by_id[c['id']] = c
    elif rank.get(c.get('status'), 0) > rank.get(mine.get('status'), 0):
        mine.update(c)
out = o if k is None else {**ours, k: o}
json.dump(out, open(path, 'w'), indent=2); open(path, 'a').write('\n')
subprocess.check_call(['git', 'add', path])
print(f'board: {len(o)} cards')
