#!/usr/bin/env python3
import json,sys
from pathlib import Path
DB=Path(__file__).with_name('board.json')
def load(): return json.loads(DB.read_text()) if DB.exists() else []
def save(rows): DB.write_text(json.dumps(rows,indent=2)+'\n')
def main():
 a=sys.argv[1:]; rows=load()
 if not a or a[0]=='list':
  print(f"{'ID':<14} {'STATUS':<10} {'OWNER':<8} {'ATTEMPTS':<9} TITLE / NOTES")
  for r in rows: print(f"{r['id']:<14} {r['status']:<10} {r['owner']:<8} {r['attempts']:<9} {r['title']} {r.get('notes','')}")
 elif a[0]=='set' and len(a)==4:
  r=next((x for x in rows if x['id']==a[1]),None)
  if r is None: raise SystemExit('unknown id')
  if a[2] not in {'title','owner','status','attempts','notes'}: raise SystemExit('unknown field')
  r[a[2]]=int(a[3]) if a[2]=='attempts' else a[3]; save(rows)
 elif a[0]=='add' and len(a)==4:
  if any(x['id']==a[1] for x in rows): raise SystemExit('duplicate id')
  rows.append(dict(id=a[1],owner=a[2],title=a[3],status='todo',attempts=0,notes='')); save(rows)
 else: raise SystemExit('Usage: board.py {list|set <id> <field> <value>|add <id> <owner> "<title>"}')
if __name__=='__main__': main()
